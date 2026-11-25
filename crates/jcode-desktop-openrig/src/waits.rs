//! Work the daemon reports as waiting on a seat: parked work and held messages.
//!
//! Reading is separate from the inbox that presents it. A read returns the waits
//! together with the fleet entries that located them, so this module depends on the
//! fleet and the inbox depends on this module, never the other way round.
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{
    HostTarget, encode,
    fleet::{Fleet, Host, Rig, Seat},
    get_target,
    util::timestamp,
};

/// One wait and the host, rig and seat it was read for.
pub(crate) enum Wait<'fleet> {
    Parked {
        host: &'fleet Host,
        rig: &'fleet Rig,
        seat: ParkedSeat,
    },
    Held {
        host: &'fleet Host,
        rig: &'fleet Rig,
        seat: &'fleet Seat,
        session: &'fleet str,
        message: HeldMessage,
    },
}

/// Every wait found, in reading order, and the reads that failed or were inconclusive.
#[derive(Default)]
pub(crate) struct Waits<'fleet> {
    pub waits: Vec<Wait<'fleet>>,
    pub issues: Vec<String>,
}

#[derive(Deserialize)]
struct ParkedRead {
    rig: ParkedRig,
}

#[derive(Deserialize)]
struct ParkedRig {
    seats: Vec<ParkedSeat>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParkedSeat {
    pub seat_node_id: String,
    pub session_name: String,
    parked: ParkedVerdict,
    pub reason: String,
    activity: ParkedActivity,
    obligations: Option<ParkedObligations>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParkedObligations {
    open_count: u64,
    unhealthy_held_count: u64,
    complete: bool,
}

impl ParkedSeat {
    /// A seat parks on open or unhealthy held obligations. With a complete read
    /// of none, unknown activity (a terminal seat has no activity hook) cannot
    /// hide parked work.
    fn nothing_to_park_on(&self) -> bool {
        self.obligations.as_ref().is_some_and(|read| {
            read.complete && read.open_count == 0 && read.unhealthy_held_count == 0
        })
    }

    /// When the seat's activity stopped, if the daemon said.
    pub(crate) fn since(&self) -> Option<DateTime<Utc>> {
        timestamp(self.activity.since.as_deref())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ParkedVerdict {
    Known(bool),
    Unknown(Indeterminate),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Indeterminate {
    Indeterminate,
}

#[derive(Deserialize)]
struct ParkedActivity {
    since: Option<String>,
}

#[derive(Deserialize)]
struct HeldPage {
    items: Vec<HeldMessage>,
    truncated: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeldMessage {
    pub outbox_id: String,
    pub sender_session: String,
    pub destination_session: String,
    pub body: String,
    pub ts_dispatched: String,
    pub delivery_state: String,
    pub guard_binding: HeldBinding,
    pub delivery: Option<HeldDelivery>,
}

#[derive(Deserialize)]
pub(crate) struct HeldDelivery {
    pub reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeldBinding {
    pub node_id: String,
}

fn read<T: serde::de::DeserializeOwned>(target: &HostTarget, route: &str) -> Result<T, String> {
    serde_json::from_value(get_target(target, route)?).map_err(|error| format!("{route}: {error}"))
}

fn parked(target: &HostTarget, rig: &str) -> Result<Vec<ParkedSeat>, String> {
    let model: ParkedRead = read(target, &format!("/api/activity/parked?rig={}", encode(rig)))?;
    Ok(model.rig.seats)
}

fn held(target: &HostTarget, session: &str) -> Result<Vec<HeldMessage>, String> {
    let mut result = Vec::new();
    loop {
        let page: HeldPage = read(
            target,
            &format!(
                "/api/seat/held-messages/{}?limit=100&offset={}",
                encode(session),
                result.len()
            ),
        )?;
        if page
            .items
            .iter()
            .any(|item| item.outbox_id.is_empty() || item.delivery_state != "retained")
        {
            return Err("OpenRig returned an invalid held message.".into());
        }
        if page.truncated && page.items.is_empty() {
            return Err("OpenRig returned an incomplete held message page.".into());
        }
        result.extend(page.items);
        if !page.truncated {
            return Ok(result);
        }
        if result.len() >= 1000 {
            return Err("OpenRig returned more than 1000 held messages for this seat.".into());
        }
    }
}

pub(crate) fn load(fleet: &Fleet) -> Waits<'_> {
    let mut found = Waits::default();
    for host in &fleet.hosts {
        let Some(rigs) = &host.rigs else { continue };
        for rig in rigs {
            match parked(&host.target, &rig.name) {
                Ok(seats) => {
                    for seat in seats {
                        match seat.parked {
                            ParkedVerdict::Known(true) => {
                                found.waits.push(Wait::Parked { host, rig, seat })
                            }
                            ParkedVerdict::Known(false) => {}
                            ParkedVerdict::Unknown(_) if seat.nothing_to_park_on() => {}
                            ParkedVerdict::Unknown(_) => found.issues.push(format!(
                                "{} / {}: work status indeterminate for {}",
                                host.name, rig.name, seat.session_name
                            )),
                        }
                    }
                }
                Err(error) => {
                    found.issues.push(format!(
                        "{} / {} · Parked work: {error}",
                        host.name, rig.name
                    ));
                }
            }
            let Some(pods) = &rig.pods else { continue };
            for seat in pods.iter().flat_map(|pod| &pod.seats) {
                let Some(session) = seat.canonical_session_name.as_deref() else {
                    continue;
                };
                match held(&host.target, session) {
                    Ok(messages) => {
                        for message in messages {
                            if message.guard_binding.node_id == seat.id {
                                found.waits.push(Wait::Held {
                                    host,
                                    rig,
                                    seat,
                                    session,
                                    message,
                                });
                            } else {
                                found.issues.push(format!(
                                    "{} / {session}: held message has a different seat binding",
                                    host.name
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        found.issues.push(format!(
                            "{} / {session} · Held messages: {error}",
                            host.name
                        ));
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[test]
    fn parked_and_held_loaders_read_the_registered_host_directly() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let registry: crate::host::HostRegistry = serde_json::from_value(json!({"hosts":[{
            "id":"remote", "url":format!("http://{}", listener.local_addr().unwrap())
        }]}))
        .unwrap();
        let (target, _) = registry.into_targets().next().unwrap();
        let cases = [
            (
                "/api/activity/parked?rig=rig%2Fone",
                include_str!("../tests/fixtures/attention_parked_get.json"),
            ),
            (
                "/api/seat/held-messages/seat%40rig?limit=100&offset=0",
                include_str!("../tests/fixtures/attention_held_get.json"),
            ),
        ];
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for (route, body) in cases {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                        .unwrap();
                    let mut lines = BufReader::new(&stream).lines();
                    assert_eq!(
                        lines.next().unwrap().unwrap(),
                        format!("GET {route} HTTP/1.1")
                    );
                    for line in lines {
                        if line.unwrap().is_empty() {
                            break;
                        }
                    }
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
            });
            let seats = parked(&target, "rig/one").unwrap();
            assert!(matches!(seats[0].parked, ParkedVerdict::Known(false)));
            assert!(matches!(seats[1].parked, ParkedVerdict::Unknown(_)));
            assert!(seats[1].nothing_to_park_on());
            assert!(matches!(seats[2].parked, ParkedVerdict::Unknown(_)));
            assert!(!seats[2].nothing_to_park_on());
            assert!(held(&target, "seat@rig").unwrap().is_empty());
        });
    }

    #[test]
    #[ignore = "needs a running OpenRig daemon; GET only"]
    fn live_daemon_reads_attention_sources() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("set OPENRIG_LIVE_BASE");
        let mut snapshot = crate::fetch_snapshot(&base);
        let host_id = std::env::var("OPENRIG_LIVE_HOST").unwrap_or_else(|_| "local".into());
        let host = snapshot
            .fleet
            .hosts
            .iter()
            .find(|host| host.queue_host_id() == host_id)
            .expect("registered live host");
        let rigs = host.rigs.as_ref().expect("host rig inventory");
        let mut parked_reads = 0;
        let mut held_reads = 0;
        for rig in rigs {
            parked(&host.target, &rig.name).expect("host parked read");
            parked_reads += 1;
            for seat in rig.pods.iter().flatten().flat_map(|pod| &pod.seats) {
                if let Some(session) = &seat.canonical_session_name {
                    held(&host.target, session).expect("host held read");
                    held_reads += 1;
                }
            }
        }
        assert!(
            parked_reads > 0 && held_reads > 0,
            "no live rig or seat on {host_id}"
        );
        snapshot.load_attention();
        let inbox = snapshot.inbox.expect("attention queue");
        assert!(inbox.waits_loaded());
        assert!(
            inbox
                .hosts
                .iter()
                .any(|host| host.host_id == host_id && host.status == "ok")
        );
        assert!(
            inbox.issues.iter().all(|issue| !issue
                .starts_with(&format!("{host_id} · Human actions:"))
                && !issue.starts_with(&format!("{host_id} · Runner permissions:"))),
            "{:?}",
            inbox.issues
        );
        eprintln!(
            "Attention {host_id}: {parked_reads} parked and {held_reads} held GETs succeeded; {} items, {} queue hosts, {} source issues",
            inbox.items.len(),
            inbox.hosts.len(),
            inbox.issues.len()
        );
    }
}
