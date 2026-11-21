use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{
    attention::{Action, HeldMessage, HumanVerb, ParkedSeat, Question},
    encode,
    fleet::{Fleet, Host, HostTarget, Rig, Seat},
    get_target,
    sections::section_get,
};

pub(crate) use crate::attention::ClearEvidence;

const ROUTE: &str = "/api/queue/list?attention=1";

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
    Parked {
        host: String,
        rig: String,
        node: String,
    },
    HeldMessage {
        host: String,
        rig: String,
        node: String,
        message: String,
    },
}

impl ItemId {
    pub fn host_id(&self) -> &str {
        match self {
            Self::Queue { host, .. }
            | Self::Seat { host, .. }
            | Self::Parked { host, .. }
            | Self::HeldMessage { host, .. } => host,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    HumanTask,
    Approval,
    Held,
    Permission,
    SeatAttention,
    Parked,
    Startup,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::HumanTask => "Action required",
            Self::Approval => "Approval",
            Self::Held => "Held",
            Self::Permission => "Permission",
            Self::SeatAttention => "Seat attention",
            Self::Parked => "Parked work",
            Self::Startup => "Startup attention",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeatReasonClass {
    Startup,
    Identity,
    Restore,
    Input,
    LaunchHold,
    Reported,
}

impl SeatReasonClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Identity => "Seat identity",
            Self::Restore => "Restore outcome",
            Self::Input => "Waiting for input",
            Self::LaunchHold => "Launch held",
            Self::Reported => "Unclassified seat attention",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: ItemId,
    kind: Kind,
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
    pub rig: Option<String>,
    pub seat: Option<String>,
    pub reason: Option<String>,
    pub reason_class: Option<SeatReasonClass>,
    pub session: Option<SessionLink>,
    actions: Vec<Action>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLink {
    pub target: HostTarget,
    pub native_id: String,
}

impl Item {
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    fn locate(&mut self, host: &Host, rig: &Rig, seat: &Seat) {
        self.rig = Some(rig.name.clone());
        self.seat = seat
            .canonical_session_name
            .clone()
            .or_else(|| Some(seat.logical_id.clone()));
        self.session = seat.jcode_session_id.as_ref().map(|native_id| SessionLink {
            target: host.target.clone(),
            native_id: native_id.clone(),
        });
    }
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
    pub(crate) waits_loaded: bool,
}

#[derive(Deserialize)]
struct Aggregate {
    hosts: Vec<HostRead>,
    items: Vec<QueueItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueItem {
    #[serde(default)]
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
    tags: Option<Vec<String>>,
    human_intent: Option<String>,
    handed_off_from: Option<String>,
}

impl QueueItem {
    fn permission_origin(&self) -> Option<(&str, &str)> {
        let tags = self.tags.as_ref()?;
        if !tags.iter().any(|tag| tag == "runner-permission") {
            return None;
        }
        let value = |prefix| {
            tags.iter()
                .find_map(|tag| tag.strip_prefix(prefix).filter(|value| !value.is_empty()))
        };
        Some((value("permission-node:")?, value("permission-generation:")?))
    }
}

#[derive(Deserialize)]
struct FeedRead {
    items: Vec<FeedItem>,
    sources: Vec<FeedSource>,
}

#[derive(Deserialize)]
struct FeedSource {
    source: String,
    state: String,
    detail: String,
}

#[derive(Deserialize)]
struct FeedItem {
    id: String,
    answer: Option<FeedAnswer>,
    revision: Option<String>,
}

#[derive(Deserialize)]
struct FeedAnswer {
    verb: FeedVerb,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum FeedVerb {
    Approve,
    Resolve,
    Answer,
    #[serde(other)]
    Unknown,
}

fn present(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

pub(crate) fn timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?).ok().map(Into::into)
}

impl From<QueueItem> for Item {
    fn from(item: QueueItem) -> Self {
        let is_permission = item.permission_origin().is_some();
        let questions: Vec<Question> = item
            .human_questions
            .iter()
            .flatten()
            .filter_map(|question| serde_json::from_value(question.clone()).ok())
            .collect();
        let mut actions = Vec::new();
        if matches!(
            item.state.as_deref(),
            Some("pending" | "in-progress" | "blocked")
        ) && item.human_intent.as_deref() != Some("update")
            && is_permission
            && let Some(question) = questions
                .iter()
                .find(|question| question.id == "permission")
        {
            actions.extend(
                question
                    .options
                    .iter()
                    .filter(|option| !option.id.is_empty() && !option.label.is_empty())
                    .cloned()
                    .map(|option| Action::decide(&item.qitem_id, option)),
            );
        }
        let seat = present(item.source_session.clone());
        let reason = present(item.blocked_on.clone());
        Self {
            id: ItemId::Queue {
                host: item.host_id.clone(),
                item: item.qitem_id,
            },
            kind: match (item.tier.as_deref(), item.state.as_deref()) {
                _ if is_permission => Kind::Permission,
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
            rig: None,
            seat,
            reason,
            reason_class: None,
            session: None,
            actions,
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
            waits_loaded: false,
        };
        inbox.sort();
        Ok(inbox)
    }

    pub(crate) fn sort(&mut self) {
        self.items
            .sort_by_key(|item| std::cmp::Reverse(item.updated_at.or(item.created_at)));
    }

    pub(crate) fn include_seat_attention(&mut self, fleet: &Fleet) {
        for host in &fleet.hosts {
            if !self
                .hosts
                .iter()
                .any(|read| read.host_id == host.queue_host_id())
            {
                self.issues.push(format!(
                    "{} · Queue: not included in the attention aggregate",
                    host.name
                ));
            }
        }
        self.locate_queue_items(fleet);
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
                    let mut item = Item {
                        id: ItemId::Seat {
                            host: host.queue_host_id().to_owned(),
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
                        rig: None,
                        seat: None,
                        reason: signal.reason.clone(),
                        reason_class: Some(signal.class),
                        session: None,
                        actions: match (seat.canonical_session_name.as_deref(), signal.clear) {
                            (Some(session), Some(evidence)) => {
                                vec![Action::clear(session, evidence, signal.observed_at)]
                            }
                            _ => Vec::new(),
                        },
                    };
                    item.locate(host, rig, seat);
                    self.items.push(item);
                }
            }
        }
        self.sort();
    }

    pub(crate) fn locate_queue_items(&mut self, fleet: &Fleet) {
        for item in &mut self.items {
            let ItemId::Queue { host, .. } = &item.id else {
                continue;
            };
            let Some(host) = fleet
                .hosts
                .iter()
                .find(|entry| entry.queue_host_id() == host)
            else {
                continue;
            };
            if let Some((rig, seat)) = host.rigs.iter().flatten().find_map(|rig| {
                let mut seats = rig.pods.iter().flatten().flat_map(|pod| &pod.seats);
                seats
                    .find(|seat| {
                        seat.canonical_session_name.is_some()
                            && seat.canonical_session_name == item.source
                    })
                    .map(|seat| (rig, seat))
            }) {
                item.locate(host, rig, seat);
            }
        }
    }

    pub fn waits_loaded(&self) -> bool {
        self.waits_loaded
    }

    pub(crate) fn include_parked(&mut self, host: &Host, rig: &Rig, parked: ParkedSeat) {
        let mut item = Item {
            id: ItemId::Parked {
                host: host.queue_host_id().to_owned(),
                rig: rig.id.clone(),
                node: parked.seat_node_id.clone(),
            },
            kind: Kind::Parked,
            host: host.name.clone(),
            source: Some(parked.session_name.clone()),
            destination: None,
            title: None,
            body: Some(parked.reason.clone()),
            reason: Some(parked.reason),
            reason_class: None,
            created_at: None,
            updated_at: timestamp(parked.activity.since.as_deref()),
            state: Some("parked".into()),
            blocked_on: None,
            evidence: None,
            questions: None,
            rig: Some(rig.name.clone()),
            seat: Some(parked.session_name),
            session: None,
            actions: Vec::new(),
        };
        if let Some(seat) = rig
            .pods
            .iter()
            .flatten()
            .flat_map(|pod| &pod.seats)
            .find(|seat| seat.id == parked.seat_node_id)
        {
            item.locate(host, rig, seat);
        }
        self.items.push(item);
    }

    pub(crate) fn include_held(
        &mut self,
        host: &Host,
        rig: &Rig,
        seat: &Seat,
        session: &str,
        message: HeldMessage,
    ) {
        let reason = message
            .delivery
            .as_ref()
            .and_then(|delivery| present(delivery.reason.clone()));
        let mut item = Item {
            id: ItemId::HeldMessage {
                host: host.queue_host_id().to_owned(),
                rig: rig.id.clone(),
                node: seat.id.clone(),
                message: message.outbox_id.clone(),
            },
            kind: Kind::Held,
            host: host.name.clone(),
            source: present(Some(message.sender_session)),
            destination: present(Some(message.destination_session)),
            title: Some("Held message".into()),
            body: present(Some(message.body)),
            reason,
            reason_class: None,
            created_at: timestamp(Some(&message.ts_dispatched)),
            updated_at: None,
            state: Some(message.delivery_state),
            blocked_on: None,
            evidence: None,
            questions: None,
            rig: None,
            seat: None,
            session: None,
            actions: vec![Action::retire(session, message.outbox_id)],
        };
        item.locate(host, rig, seat);
        self.items.push(item);
    }

    fn apply_feed(&mut self, host_id: &str, feed: FeedRead) {
        self.issues.extend(
            feed.sources
                .into_iter()
                .filter(|source| source.source == "queue" && source.state != "available")
                .map(|source| format!("{host_id} · Human actions: {}", source.detail)),
        );
        for item in &mut self.items {
            let ItemId::Queue { host, item: id } = &item.id else {
                continue;
            };
            if host != host_id
                || item
                    .evidence
                    .as_deref()
                    .is_some_and(|value| value.starts_with("pair-request:"))
            {
                continue;
            }
            let feed_id = format!("queue:{id}");
            let Some(FeedItem {
                answer: Some(answer),
                revision,
                ..
            }) = feed.items.iter().find(|row| row.id == feed_id)
            else {
                continue;
            };
            let action = |verb| Action::human(id, revision.as_deref(), verb);
            match answer.verb {
                FeedVerb::Approve if item.questions.is_none() => {
                    item.actions.push(action(HumanVerb::Approve))
                }
                FeedVerb::Resolve => item.actions.push(action(HumanVerb::Resolve)),
                FeedVerb::Answer => {
                    // A runner's answered-path requests have a dedicated decision control.
                    if item.kind == Kind::Permission && !item.actions.is_empty() {
                        continue;
                    }
                    for question in item.questions.iter().flatten().filter_map(|question| {
                        serde_json::from_value::<Question>(question.clone()).ok()
                    }) {
                        for option in question.options {
                            if !question.id.is_empty()
                                && !option.id.is_empty()
                                && !option.label.is_empty()
                            {
                                item.actions.push(action(HumanVerb::Answer {
                                    question_id: question.id.clone(),
                                    question: question.question.clone(),
                                    option,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn load_human_actions(&mut self, host: &Host) {
        let host_id = host.queue_host_id();
        const FEED: &str = "/api/attention?kind=action";
        match get_target(&host.target, FEED).and_then(|value| {
            serde_json::from_value::<FeedRead>(value).map_err(|error| format!("{FEED}: {error}"))
        }) {
            Ok(feed) => {
                let missing: Vec<_> = feed.items.iter().filter_map(|row| row.id.strip_prefix("queue:")).filter(|id| {
                    !self.items.iter().any(|item| matches!(&item.id, ItemId::Queue {host, item} if host == host_id && item == id))
                }).collect();
                if missing.len() > 20 {
                    self.issues.push(format!(
                        "{host_id}: showing details for at most 20 additional requests."
                    ));
                }
                for id in missing.into_iter().take(20) {
                    let route = format!("/api/queue/{}", encode(id));
                    match get_target(&host.target, &route).and_then(|value| {
                        serde_json::from_value::<QueueItem>(value)
                            .map_err(|error| format!("{route}: {error}"))
                    }) {
                        Ok(mut row)
                            if row.qitem_id == id
                                && matches!(
                                    row.state.as_deref(),
                                    Some("pending" | "blocked" | "in-progress")
                                ) =>
                        {
                            row.host_id = host_id.into();
                            self.items.push(row.into());
                        }
                        Ok(_) => {}
                        Err(error) => self.issues.push(error),
                    }
                }
                self.apply_feed(host_id, feed);
            }
            Err(error) => self
                .issues
                .push(format!("{host_id} · Human actions: {error}")),
        }
    }

    pub(crate) fn load_permissions(&mut self, host: &Host) {
        let host_id = host.queue_host_id();
        const ROUTE: &str =
            "/api/queue/list?tag=runner-permission&state=pending,in-progress,blocked&limit=100";
        let rows = get_target(&host.target, ROUTE).and_then(|value| {
            serde_json::from_value::<Vec<QueueItem>>(value)
                .map_err(|error| format!("{ROUTE}: {error}"))
        });
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                self.issues
                    .push(format!("{host_id} · Runner permissions: {error}"));
                return;
            }
        };
        if rows.len() == 100 {
            self.issues.push(format!(
                "{host_id}: showing up to 100 open runner permissions."
            ));
        }
        for mut row in rows {
            let Some((node, generation)) = row.permission_origin() else {
                continue;
            };
            let origin = (node.to_owned(), generation.to_owned());
            let mut previous = row.handed_off_from.clone();
            for _ in 0..10 {
                if row
                    .human_questions
                    .as_ref()
                    .is_some_and(|questions| !questions.is_empty())
                {
                    break;
                }
                let Some(id) = previous else { break };
                let route = format!("/api/queue/{}", encode(&id));
                let parent = get_target(&host.target, &route).and_then(|value| {
                    serde_json::from_value::<QueueItem>(value)
                        .map_err(|error| format!("{route}: {error}"))
                });
                match parent {
                    Ok(parent)
                        if parent.qitem_id == id
                            && parent.permission_origin() == Some((&origin.0, &origin.1)) =>
                    {
                        row.human_questions = parent.human_questions;
                        row.source_session = parent.source_session;
                        previous = parent.handed_off_from;
                    }
                    Ok(_) => break,
                    Err(error) => {
                        self.issues
                            .push(format!("{host_id} · Runner permissions: {error}"));
                        break;
                    }
                }
            }
            row.host_id = host_id.into();
            let item = Item::from(row);
            if let Some(index) = self.items.iter().position(|current| current.id == item.id) {
                self.items[index] = item;
            } else {
                self.items.push(item);
            }
        }
    }

    pub fn fixture() -> Self {
        let mut inbox = Self::parse(
            serde_json::from_str(include_str!("../tests/fixtures/inbox.json")).unwrap(),
        )
        .unwrap();
        inbox.apply_feed("local", serde_json::from_value(serde_json::json!({"items":[{"id":"queue:q1","answer":{"verb":"answer"},"revision":"fixture-revision"},{"id":"queue:q2","answer":{"verb":"resolve"},"revision":"fixture-revision"}],"sources":[]})).unwrap());
        inbox.waits_loaded = true;
        inbox
    }
}

pub(crate) fn fetch_inbox(fleet: &Fleet) -> Inbox {
    let reads = std::thread::scope(|scope| {
        fleet
            .hosts
            .iter()
            .map(|host| {
                scope.spawn(|| {
                    (
                        host.queue_host_id(),
                        section_get::<Vec<QueueItem>>(&host.target, ROUTE),
                    )
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|read| read.join().expect("Inbox reader panicked"))
            .collect::<Vec<_>>()
    });
    let mut inbox = Inbox::default();
    for (id, read) in reads {
        let rows = read.and_then(|rows| {
            if rows.iter().any(|row| row.qitem_id.trim().is_empty()) {
                return Err("Invalid queue item identity".into());
            }
            Ok(rows)
        });
        let error = rows.as_ref().err().cloned();
        if let Some(error) = &error {
            inbox
                .issues
                .push(format!("{id} · Queue unavailable: {error}"));
        }
        inbox.hosts.push(HostRead {
            host_id: id.into(),
            status: if error.is_some() { "unavailable" } else { "ok" }.into(),
            error,
        });
        if let Ok(rows) = rows {
            inbox.items.extend(rows.into_iter().map(|mut row| {
                row.host_id = id.into();
                row.into()
            }));
        }
    }
    inbox
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn captured_attention_get_offers_confirmable_acknowledgement() {
        let mut inbox = Inbox::parse(json!({
            "hosts":[{"hostId":"local","status":"ok"}],
            "items":[{"hostId":"local","qitemId":"qitem-desktop-isolated-ack","destinationSession":"human@host","state":"pending"}]
        })).unwrap();
        assert!(inbox.items[0].actions().is_empty());
        let feed = serde_json::from_str(include_str!(
            "../tests/fixtures/attention_isolated_get.json"
        ))
        .unwrap();
        inbox.apply_feed("local", feed);
        let [action] = inbox.items[0].actions() else {
            panic!("expected the daemon's acknowledgement action")
        };
        assert_eq!(
            action,
            &Action::human(
                "qitem-desktop-isolated-ack",
                Some("dr1-c2c3a05a96be5d8005096e88"),
                HumanVerb::Approve
            )
        );
    }

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
    fn inbox_reads_own_queue_and_offers_actions_per_host() {
        let (remote, remote_server) = crate::sections::serve_fixture(2, |route| {
            match route {
            ROUTE => (200, r#"[{"qitemId":"same","hostId":"local","state":"pending"}]"#.into()),
            "/api/attention?kind=action" => (200, r#"{"sources":[],"items":[{"id":"queue:same","answer":{"verb":"approve"},"revision":"remote-revision"}]}"#.into()),
            _ => panic!("unexpected remote route {route}"),
        }
        });
        let (base, server) = crate::sections::serve_fixture(1, |route| {
            assert_eq!(route, ROUTE);
            (404, "{}".into())
        });
        let health = json!({"selfHostId":"me"});
        let registry = json!({"hosts":[{"id":"another", "url":remote}]});
        let fleet = Fleet::load(
            &base,
            Some(&health),
            Some(&registry),
            Some(&json!([])),
            &mut Vec::new(),
            |_, _| Ok(json!([])),
        );
        let mut inbox = fetch_inbox(&fleet);
        inbox.load_human_actions(&fleet.hosts[1]);
        server.join().unwrap();
        remote_server.join().unwrap();
        assert_eq!(inbox.hosts[0].error.as_deref(), Some("HTTP 404"));
        assert_eq!(inbox.hosts[1].status, "ok");
        let [item] = inbox.items.as_slice() else {
            panic!("expected remote queue item");
        };
        assert_eq!(item.id.host_id(), "another");
        assert_eq!(
            item.actions(),
            &[Action::human(
                "same",
                Some("remote-revision"),
                HumanVerb::Approve
            )]
        );
    }

    #[test]
    #[ignore = "needs a running OpenRig daemon"]
    fn live_daemon_reads_inbox() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("set OPENRIG_LIVE_BASE");
        let inbox = crate::fetch_snapshot(&base)
            .inbox
            .expect("load attention queue");
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
