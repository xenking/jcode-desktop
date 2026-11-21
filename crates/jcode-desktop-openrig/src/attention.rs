use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{encode, fleet::Fleet, get, inbox::Inbox, token};

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
        host: String,
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

    pub(crate) fn human(host: &str, id: &str, revision: Option<&str>, verb: HumanVerb) -> Self {
        Self(Operation::Human {
            host: host.into(),
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
            Operation::Human {
                host,
                id,
                revision,
                verb,
            } => {
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
                if host == "local" {
                    body["authority"] = json!("primary-user");
                } else {
                    body["hostId"] = json!(host);
                }
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
    pub fn execute(self, base: &str) -> Result<(), String> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(20))
            .build();
        let mut request = agent.post(&format!("{}{}", base.trim_end_matches('/'), self.route));
        if let Some(token) = token(self.route == MISSION_ACTION) {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
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
                    return Err(daemon_error(&result)
                        .unwrap_or("OpenRig refused the action.")
                        .to_owned());
                }
                if self.route.ends_with("/decide")
                    && !matches!(result["state"].as_str(), Some("answered" | "refused"))
                {
                    return Err(format!(
                        "No decision was recorded. Request state: {}",
                        result["state"].as_str().unwrap_or("–")
                    ));
                }
                Ok(())
            }
            Ok(response) | Err(ureq::Error::Status(_, response)) => {
                let status = response.status();
                let raw = response.into_string().unwrap_or_default();
                let body = serde_json::from_str(&raw).unwrap_or(Value::Null);
                Err(daemon_error(&body).map(str::to_owned).unwrap_or_else(|| {
                    if raw.is_empty() {
                        format!("OpenRig returned HTTP {status}.")
                    } else {
                        raw
                    }
                }))
            }
            Err(error) => Err(error.to_string()),
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

fn read<T: serde::de::DeserializeOwned>(base: &str, route: &str) -> Result<T, String> {
    serde_json::from_value(get(base, route)?).map_err(|error| format!("{route}: {error}"))
}

fn parked(base: &str, host: &str, rig: &str) -> Result<Vec<ParkedSeat>, String> {
    let remote = if host == "local" {
        String::new()
    } else {
        format!("&host={}", encode(host))
    };
    let model: ParkedRead = read(
        base,
        &format!("/api/activity/parked?rig={}{remote}", encode(rig)),
    )?;
    Ok(model.rig.seats)
}

fn held(base: &str, host: &str, session: &str) -> Result<Vec<HeldMessage>, String> {
    let remote = if host == "local" {
        String::new()
    } else {
        format!("&host={}", encode(host))
    };
    let mut result = Vec::new();
    loop {
        let page: HeldPage = read(
            base,
            &format!(
                "/api/seat/held-messages/{}?limit=100&offset={}{remote}",
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

pub fn load_waits(base: &str, fleet: &Fleet, inbox: &mut Inbox) {
    for host in &fleet.hosts {
        let Some(rigs) = &host.rigs else { continue };
        let mut parked_unavailable = false;
        let mut held_unavailable = false;
        for rig in rigs {
            if !parked_unavailable {
                match parked(base, host.queue_host_id(), &rig.name) {
                    Ok(seats) => {
                        for seat in seats {
                            match seat.parked {
                                ParkedVerdict::Known(true) => inbox.include_parked(host, rig, seat),
                                ParkedVerdict::Known(false) => {}
                                ParkedVerdict::Unknown(_) => inbox.issues.push(format!(
                                    "{} / {}: work status indeterminate for {}",
                                    host.name, rig.name, seat.session_name
                                )),
                            }
                        }
                    }
                    Err(error) => {
                        parked_unavailable = host.queue_host_id() != "local"
                            && error.contains("read_through_path_not_allowed");
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
                if held_unavailable {
                    break;
                }
                match held(base, host.queue_host_id(), session) {
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
                        held_unavailable = host.queue_host_id() != "local"
                            && error.contains("read_through_path_not_allowed");
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
    fn parses_captured_parked_and_held_gets() {
        let parked: ParkedRead =
            serde_json::from_str(include_str!("../tests/fixtures/attention_parked_get.json"))
                .unwrap();
        assert!(matches!(
            parked.rig.seats[0].parked,
            ParkedVerdict::Known(false)
        ));
        assert!(matches!(
            parked.rig.seats[1].parked,
            ParkedVerdict::Unknown(_)
        ));
        let held: HeldPage =
            serde_json::from_str(include_str!("../tests/fixtures/attention_held_get.json"))
                .unwrap();
        assert!(held.items.is_empty() && !held.truncated);
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
                Action::human("local", "request-7", Some("dr1-123"), HumanVerb::Resolve),
                "Proceed",
                MISSION_ACTION,
                None,
                json!({"verb":"resolve","qitemId":"request-7","actorSession":"human@host","decision":"Proceed","authority":"primary-user","expectedRevision":"dr1-123"}),
            ),
            (
                Action::human("ai-core", "request-8", None, HumanVerb::Approve),
                "",
                MISSION_ACTION,
                None,
                json!({"verb":"approve","qitemId":"request-8","actorSession":"human@host","hostId":"ai-core"}),
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
                        .execute(&format!("http://{addr}")),
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
        let local = snapshot
            .fleet
            .hosts
            .iter()
            .find(|host| host.queue_host_id() == "local")
            .expect("local host");
        let rigs = local.rigs.as_ref().expect("local rig inventory");
        let mut parked_reads = 0;
        let mut held_reads = 0;
        for rig in rigs {
            parked(&base, "local", &rig.name).expect("local parked read");
            parked_reads += 1;
            for seat in rig.pods.iter().flatten().flat_map(|pod| &pod.seats) {
                if let Some(session) = &seat.canonical_session_name {
                    held(&base, "local", session).expect("local held read");
                    held_reads += 1;
                }
            }
        }
        snapshot.load_attention(&base);
        let inbox = snapshot.inbox.expect("attention aggregate");
        assert!(inbox.waits_loaded());
        assert!(
            inbox
                .hosts
                .iter()
                .any(|host| host.host_id == "local" && host.status == "ok")
        );
        assert!(
            inbox
                .issues
                .iter()
                .all(|issue| !issue.starts_with("Local human actions:")
                    && !issue.starts_with("Local runner permissions:")),
            "{:?}",
            inbox.issues
        );
        eprintln!(
            "Attention: {parked_reads} local parked and {held_reads} local held GETs succeeded; {} items, {} queue hosts, {} source issues",
            inbox.items.len(),
            inbox.hosts.len(),
            inbox.issues.len()
        );
        for issue in inbox.issues {
            eprintln!("  {issue}");
        }
    }
}
