use serde::Deserialize;
use serde_json::{Value, json};

use crate::{HostTarget, encode, fleet::Fleet, get_target, host::redact, inbox::Inbox};

const TERMINAL_ACTOR: &str = "human@host";
const MISSION_ACTION: &str = "/api/mission-control/action";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearEvidence {
    Reconcile,
    AttestPromptEnded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct OfferedOption {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Question {
    pub id: String,
    pub question: String,
    pub options: Vec<OfferedOption>,
}

/// An offered action carries the exact item and, for a question, the exact offered choice.
/// Only the server projections can construct these capabilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Action(Operation);

#[derive(Clone, Debug, Eq, PartialEq)]
enum Operation {
    Clear {
        session: String,
        evidence: ClearEvidence,
        observed_at: Option<chrono::DateTime<chrono::Utc>>,
    },
    Retire {
        session: String,
        id: String,
    },
    Decide {
        id: String,
        option: OfferedOption,
    },
    Human {
        id: String,
        revision: Option<String>,
        verb: HumanVerb,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HumanVerb {
    Approve,
    Resolve,
    Answer {
        question_id: String,
        question: String,
        option: OfferedOption,
    },
}

impl Action {
    pub(crate) fn clear(
        session: &str,
        evidence: ClearEvidence,
        observed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Self {
        Self(Operation::Clear {
            session: session.into(),
            evidence,
            observed_at: (evidence == ClearEvidence::AttestPromptEnded)
                .then_some(observed_at)
                .flatten(),
        })
    }

    pub(crate) fn retire(session: &str, id: String) -> Self {
        Self(Operation::Retire {
            session: session.into(),
            id,
        })
    }

    pub(crate) fn decide(id: &str, option: OfferedOption) -> Self {
        Self(Operation::Decide {
            id: id.into(),
            option,
        })
    }

    pub(crate) fn human(id: &str, revision: Option<&str>, verb: HumanVerb) -> Self {
        Self(Operation::Human {
            id: id.into(),
            revision: revision.map(str::to_owned),
            verb,
        })
    }

    pub fn label(&self) -> &str {
        match &self.0 {
            Operation::Clear { .. } => "Clear attention",
            Operation::Retire { .. } => "Retire held message",
            Operation::Decide { option, .. }
            | Operation::Human {
                verb: HumanVerb::Answer { option, .. },
                ..
            } => &option.label,
            Operation::Human {
                verb: HumanVerb::Approve,
                ..
            } => "Acknowledge and close",
            Operation::Human {
                verb: HumanVerb::Resolve,
                ..
            } => "Record decision",
        }
    }

    pub fn effect(&self) -> String {
        match &self.0 {
            Operation::Clear { evidence: ClearEvidence::Reconcile, .. } => "Ask OpenRig to verify the seat and clear attention. This does not restart it.".into(),
            Operation::Clear { evidence: ClearEvidence::AttestPromptEnded, .. } => "Only confirm if you saw the prompt answered or dismissed. Say what you observed.".into(),
            Operation::Retire { .. } => "Remove this message from the active hold queue. Its record is kept and nothing is delivered.".into(),
            Operation::Decide { option, .. } => format!("Record the choice “{}” on the runner’s permission request.", option.label),
            Operation::Human { verb: HumanVerb::Answer { question, option, .. }, .. } => format!("Answer “{question}” with “{}”. OpenRig returns the answer to the asking agent.", option.label),
            Operation::Human { verb: HumanVerb::Approve, .. } => "Acknowledge this request and close it without sending a reply.".into(),
            Operation::Human { verb: HumanVerb::Resolve, .. } => "Send your decision to the owner and return the blocked work to in progress.".into(),
        }
    }

    pub fn input_label(&self) -> Option<&'static str> {
        match &self.0 {
            Operation::Retire { .. } => Some("Reason for retiring"),
            Operation::Clear {
                evidence: ClearEvidence::AttestPromptEnded,
                ..
            } => Some("What you observed"),
            Operation::Human {
                verb: HumanVerb::Resolve,
                ..
            } => Some("Your decision"),
            _ => None,
        }
    }

    /// A confirmed action owns a validated payload. No URL or option can be supplied by the UI.
    pub fn confirm(&self, input: &str) -> Option<ConfirmedAction> {
        let input = input.trim();
        if self.input_label().is_some() && input.is_empty() {
            return None;
        }
        let (route, actor, mut body) = match &self.0 {
            Operation::Clear { session, .. } => (
                format!("/api/sessions/{}/clear-attention", encode(session)),
                None,
                if input.is_empty() {
                    json!({})
                } else {
                    json!({"reason":input})
                },
            ),
            Operation::Retire { session, id } => (
                format!(
                    "/api/seat/retire-held-message/{}/{}",
                    encode(session),
                    encode(id)
                ),
                Some(TERMINAL_ACTOR),
                json!({"reason":input}),
            ),
            Operation::Decide { id, option } => (
                format!("/api/activity/permission-requests/{}/decide", encode(id)),
                None,
                json!({"optionId":option.id,"reason":input}),
            ),
            Operation::Human { id, revision, verb } => {
                let mut body = match verb {
                    HumanVerb::Approve => json!({"verb":"approve"}),
                    HumanVerb::Resolve => json!({"verb":"resolve","decision":input}),
                    HumanVerb::Answer {
                        question_id,
                        option,
                        ..
                    } => json!({"verb":"answer","questionId":question_id,"optionId":option.id}),
                };
                body["qitemId"] = json!(id);
                body["actorSession"] = json!(TERMINAL_ACTOR);
                body["authority"] = json!("primary-user");
                if let Some(revision) = revision {
                    body["expectedRevision"] = json!(revision);
                }
                (MISSION_ACTION.into(), None, body)
            }
        };
        if route == MISSION_ACTION && body["verb"] == "approve" && !input.is_empty() {
            body["reason"] = json!(input);
        }
        Some(ConfirmedAction { route, actor, body })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedAction {
    route: String,
    actor: Option<&'static str>,
    body: Value,
}

impl ConfirmedAction {
    pub fn execute(self, target: &HostTarget) -> Result<(), String> {
        let bearer = target.bearer(self.route == MISSION_ACTION)?;
        let safe = |value: &str| redact(value, bearer.as_deref());
        let mut request = target.request("POST", &self.route, 20, bearer.as_deref())?;
        if let Some(actor) = self.actor {
            request = request.set("X-OpenRig-Session", actor);
        }
        let response = request.send_json(self.body);
        match response {
            Ok(response) if (200..300).contains(&response.status()) => {
                let result: Value = response
                    .into_json()
                    .map_err(|_| "OpenRig returned an invalid action response.".to_owned())?;
                if result["ok"] == false {
                    return Err(safe(
                        daemon_error(&result).unwrap_or("OpenRig refused the action."),
                    ));
                }
                if self.route.ends_with("/decide")
                    && !matches!(result["state"].as_str(), Some("answered" | "refused"))
                {
                    return Err(format!(
                        "No decision was recorded. Request state: {}",
                        safe(result["state"].as_str().unwrap_or("–"))
                    ));
                }
                Ok(())
            }
            Ok(response) | Err(ureq::Error::Status(_, response)) => {
                let status = response.status();
                let raw = response.into_string().unwrap_or_default();
                let body = serde_json::from_str(&raw).unwrap_or(Value::Null);
                Err(daemon_error(&body).map(safe).unwrap_or_else(|| {
                    if raw.is_empty() {
                        format!("OpenRig returned HTTP {status}.")
                    } else {
                        safe(&raw)
                    }
                }))
            }
            Err(_) => Err("Connection failed or timed out".into()),
        }
    }
}

fn daemon_error(body: &Value) -> Option<&str> {
    // clear-attention and the remote forwarding envelope use detail for the daemon's actual error.
    ["detail", "message", "error"]
        .into_iter()
        .find_map(|key| body[key].as_str())
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
    pub activity: ParkedActivity,
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
pub(crate) struct ParkedActivity {
    pub since: Option<String>,
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

pub(crate) fn load_waits(fleet: &Fleet, inbox: &mut Inbox) {
    for host in &fleet.hosts {
        let Some(rigs) = &host.rigs else { continue };
        for rig in rigs {
            {
                match parked(&host.target, &rig.name) {
                    Ok(seats) => {
                        for seat in seats {
                            match seat.parked {
                                ParkedVerdict::Known(true) => inbox.include_parked(host, rig, seat),
                                ParkedVerdict::Known(false) => {}
                                ParkedVerdict::Unknown(_) if seat.nothing_to_park_on() => {}
                                ParkedVerdict::Unknown(_) => inbox.issues.push(format!(
                                    "{} / {}: work status indeterminate for {}",
                                    host.name, rig.name, seat.session_name
                                )),
                            }
                        }
                    }
                    Err(error) => {
                        inbox.issues.push(format!(
                            "{} / {} · Parked work: {error}",
                            host.name, rig.name
                        ));
                    }
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
                                inbox.include_held(host, rig, seat, session, message);
                            } else {
                                inbox.issues.push(format!(
                                    "{} / {session}: held message has a different seat binding",
                                    host.name
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        inbox.issues.push(format!(
                            "{} / {session} · Held messages: {error}",
                            host.name
                        ));
                    }
                }
            }
        }
    }
    inbox.sort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
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
    fn confirmed_actions_send_exact_requests_and_keep_daemon_refusal_verbatim() {
        let cases = [
            (
                Action::clear("seat@rig", ClearEvidence::Reconcile, None),
                "",
                "/api/sessions/seat%40rig/clear-attention",
                None,
                json!({}),
            ),
            (
                Action::retire("seat@rig", "msg/7".into()),
                "Duplicate",
                "/api/seat/retire-held-message/seat%40rig/msg%2F7",
                Some("human@host"),
                json!({"reason":"Duplicate"}),
            ),
            (
                Action::decide(
                    "ask/7",
                    OfferedOption {
                        id: "exact-id".into(),
                        label: "Presented option".into(),
                    },
                ),
                "",
                "/api/activity/permission-requests/ask%2F7/decide",
                None,
                json!({"optionId":"exact-id","reason":""}),
            ),
            (
                Action::human("request-7", Some("dr1-123"), HumanVerb::Resolve),
                "Proceed",
                MISSION_ACTION,
                None,
                json!({"verb":"resolve","qitemId":"request-7","actorSession":"human@host","decision":"Proceed","authority":"primary-user","expectedRevision":"dr1-123"}),
            ),
            (
                Action::human("request-8", None, HumanVerb::Approve),
                "",
                MISSION_ACTION,
                None,
                json!({"verb":"approve","qitemId":"request-8","actorSession":"human@host","authority":"primary-user"}),
            ),
        ];
        for (action, input, route, actor, payload) in cases {
            if action.input_label().is_some() {
                assert!(action.confirm(" ").is_none());
            }
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let refusal = "Binding changed: leave this seat untouched.\nTry again after checking the inspector.";
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    assert_eq!(line.trim_end(), format!("POST {route} HTTP/1.1"));
                    let mut length = 0;
                    let mut actual_actor = None;
                    loop {
                        line.clear();
                        reader.read_line(&mut line).unwrap();
                        if line.trim().is_empty() { break; }
                        if let Some((key, value)) = line.split_once(':') {
                            match key.to_ascii_lowercase().as_str() {
                                "content-length" => length = value.trim().parse().unwrap(),
                                "x-openrig-session" => actual_actor = Some(value.trim().to_owned()),
                                _ => {},
                            }
                        }
                    }
                    assert_eq!(actual_actor.as_deref(), actor);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), payload);
                    let body = json!({"ok":false,"code":"remapped","detail":refusal}).to_string();
                    write!(stream, "HTTP/1.1 409 Conflict\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                });
                assert_eq!(
                    action
                        .confirm(input)
                        .unwrap()
                        .execute(&HostTarget::primary(&format!("http://{addr}"), "local")),
                    Err(refusal.into())
                );
            });
        }
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
