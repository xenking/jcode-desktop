use serde::Deserialize;
use serde_json::{Value, json};

use crate::{HostTarget, encode, host::redact};

const TERMINAL_ACTOR: &str = "human@host";
const MISSION_ACTION: &str = "/api/mission-control/action";

/// What needs a human, as the queue and the fleet name it.
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

/// Why a seat asks for attention.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

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
}
