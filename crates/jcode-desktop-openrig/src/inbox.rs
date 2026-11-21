use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{fleet::Fleet, request};

const ROUTE: &str = "/api/queue/attention-aggregate";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemId {
    Queue {
        host: String,
        item: String,
    },
    Seat {
        host: String,
        rig: String,
        node: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    HumanTask,
    Approval,
    Held,
    Permission,
    SeatAttention,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::HumanTask => "Action required",
            Self::Approval => "Approval",
            Self::Held => "Held",
            Self::Permission => "Permission",
            Self::SeatAttention => "Seat attention",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: ItemId,
    pub kind: Kind,
    pub host: String,
    pub source: Option<String>,
    pub destination: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub state: Option<String>,
    pub blocked_on: Option<String>,
    pub evidence: Option<String>,
    pub questions: Option<Vec<serde_json::Value>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostRead {
    pub host_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inbox {
    pub hosts: Vec<HostRead>,
    pub items: Vec<Item>,
    pub issues: Vec<String>,
}

#[derive(Deserialize)]
struct Aggregate {
    hosts: Vec<HostRead>,
    items: Vec<QueueItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueItem {
    host_id: String,
    qitem_id: String,
    source_session: Option<String>,
    destination_session: Option<String>,
    summary: Option<String>,
    body: Option<String>,
    ts_created: Option<String>,
    ts_updated: Option<String>,
    state: Option<String>,
    tier: Option<String>,
    blocked_on: Option<String>,
    evidence_ref: Option<String>,
    human_questions: Option<Vec<serde_json::Value>>,
}

fn present(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

pub(crate) fn timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?).ok().map(Into::into)
}

impl From<QueueItem> for Item {
    fn from(item: QueueItem) -> Self {
        Self {
            id: ItemId::Queue {
                host: item.host_id.clone(),
                item: item.qitem_id,
            },
            kind: match (item.tier.as_deref(), item.state.as_deref()) {
                (Some("human-gate"), _) => Kind::Approval,
                (_, Some("blocked")) => Kind::Held,
                _ => Kind::HumanTask,
            },
            host: item.host_id,
            source: present(item.source_session),
            destination: present(item.destination_session),
            title: present(item.summary),
            body: present(item.body),
            created_at: timestamp(item.ts_created.as_deref()),
            updated_at: timestamp(item.ts_updated.as_deref()),
            state: present(item.state),
            blocked_on: present(item.blocked_on),
            evidence: present(item.evidence_ref),
            questions: item
                .human_questions
                .filter(|questions| !questions.is_empty()),
        }
    }
}

impl Inbox {
    fn parse(value: serde_json::Value) -> Result<Self, String> {
        let Aggregate { hosts, items } =
            serde_json::from_value(value).map_err(|error| format!("{ROUTE}: {error}"))?;
        if !hosts.iter().any(|host| host.host_id == "local")
            || hosts
                .iter()
                .any(|host| host.host_id.trim().is_empty() || host.status.trim().is_empty())
            || items.iter().any(|item| {
                item.qitem_id.trim().is_empty()
                    || !hosts
                        .iter()
                        .any(|host| host.host_id == item.host_id && host.status == "ok")
            })
        {
            return Err(format!("{ROUTE}: invalid item or host identity"));
        }
        let issues = hosts
            .iter()
            .filter(|host| host.status != "ok")
            .map(|host| match &host.error {
                Some(error) if !error.trim().is_empty() => {
                    format!("{}: {} ({error})", host.host_id, host.status)
                }
                _ => format!("{}: {}", host.host_id, host.status),
            })
            .collect();
        let mut inbox = Self {
            hosts,
            items: items.into_iter().map(Item::from).collect(),
            issues,
        };
        inbox.sort();
        Ok(inbox)
    }

    fn sort(&mut self) {
        self.items
            .sort_by_key(|item| std::cmp::Reverse(item.updated_at.or(item.created_at)));
    }

    pub(crate) fn include_seat_attention(&mut self, fleet: &Fleet) {
        if fleet.hosts.is_empty() {
            self.issues.push("Seat inventory unavailable".into());
        }
        for host in &fleet.hosts {
            let Some(rigs) = &host.rigs else {
                self.issues
                    .push(format!("Seat inventory unavailable on {}", host.name));
                continue;
            };
            for rig in rigs {
                let Some(pods) = &rig.pods else {
                    self.issues.push(format!(
                        "Seat inventory unavailable on {}/{}",
                        host.name, rig.name
                    ));
                    continue;
                };
                for seat in pods.iter().flat_map(|pod| &pod.seats) {
                    let Some(signal) = &seat.attention else {
                        continue;
                    };
                    self.items.push(Item {
                        id: ItemId::Seat {
                            host: host.id.clone(),
                            rig: rig.id.clone(),
                            node: seat.id.clone(),
                        },
                        kind: signal.kind,
                        host: host.name.clone(),
                        source: Some(seat.logical_id.clone()),
                        destination: None,
                        title: None,
                        body: signal.reason.clone(),
                        created_at: None,
                        updated_at: signal.observed_at,
                        state: seat.status.clone(),
                        blocked_on: None,
                        evidence: signal.evidence.clone(),
                        questions: None,
                    });
                }
            }
        }
        self.sort();
    }

    pub fn fixture() -> Self {
        Self::parse(serde_json::from_str(include_str!("../tests/fixtures/inbox.json")).unwrap())
            .unwrap()
    }
}

pub fn fetch_inbox(base: &str) -> Result<Inbox, String> {
    // The daemon's own per-host read deadline is five seconds.
    Inbox::parse(request(base, ROUTE, None, false, 8)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inbox_preserves_queue_identity_missing_fields_and_partial_host_failure() {
        let inbox = Inbox::fixture();
        assert_eq!(inbox.items.len(), 3);
        let [approval, task, held] = inbox.items.as_slice() else {
            unreachable!()
        };
        assert_eq!(approval.kind, Kind::Approval);
        assert_eq!(approval.source.as_deref(), Some("advisor-lead@kernel"));
        assert_eq!(
            approval.body.as_deref(),
            Some("The migration passed the staging checks. Can it ship?")
        );
        assert_eq!(approval.questions.as_ref().map(Vec::len), Some(1));
        assert_eq!(task.kind, Kind::HumanTask);
        assert_ne!(task.id, approval.id, "queue IDs may repeat across hosts");
        assert_eq!(held.kind, Kind::Held);
        assert_eq!(held.created_at, None);
        assert_eq!(held.updated_at, None);
        assert_eq!(held.source, None);
        assert_eq!(held.body, None);
        assert_eq!(inbox.issues, ["offline: unreachable (read timed out)"]);
    }

    #[test]
    fn inbox_rejects_malformed_empty_responses() {
        assert!(Inbox::parse(json!({"items": [], "hosts": []})).is_err());
        assert!(Inbox::parse(json!({"hosts": [{"hostId":"local", "status":"ok"}]})).is_err());
        assert!(
            Inbox::parse(json!({"items": [], "hosts": [{"hostId":"local", "status":"ok"}]}))
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn inbox_get_reports_unavailable_route() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let (mut client, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&client).read_line(&mut line).unwrap();
                assert_eq!(line.trim_end(), format!("GET {ROUTE} HTTP/1.1"));
                client.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            });
            assert!(
                fetch_inbox(&format!("http://{addr}"))
                    .unwrap_err()
                    .contains("HTTP 404")
            );
        });
    }

    #[test]
    #[ignore = "needs a running OpenRig daemon"]
    fn live_daemon_reads_inbox() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("set OPENRIG_LIVE_BASE");
        let inbox = fetch_inbox(&base).expect("load attention queue");
        assert!(
            inbox
                .hosts
                .iter()
                .any(|host| host.host_id == "local" && host.status == "ok")
        );
        let local =
            crate::get(&base, "/api/queue/list?attention=1").expect("read local attention queue");
        let mut local_ids: Vec<_> = local
            .as_array()
            .expect("local attention array")
            .iter()
            .map(|item| item["qitemId"].as_str().expect("queue item ID"))
            .collect();
        let mut aggregate_ids: Vec<_> = inbox
            .items
            .iter()
            .filter_map(|item| match &item.id {
                ItemId::Queue { host, item } if host == "local" => Some(item.as_str()),
                _ => None,
            })
            .collect();
        local_ids.sort_unstable();
        aggregate_ids.sort_unstable();
        assert_eq!(aggregate_ids, local_ids);
        eprintln!(
            "Inbox: {} queue items ({} local); hosts: {}",
            inbox.items.len(),
            local_ids.len(),
            inbox
                .hosts
                .iter()
                .map(|h| format!("{}={}", h.host_id, h.status))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
