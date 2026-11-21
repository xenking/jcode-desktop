use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::io::Read;

use crate::fleet::{Fleet, Host, HostTarget, Runtime, Seat};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Advisor,
    Operator,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Advisor => "Advisor",
            Self::Operator => "Operator",
        }
    }

    fn logical_id(self) -> &'static str {
        match self {
            Self::Advisor => "advisor.lead",
            Self::Operator => "operator.agent",
        }
    }
}

pub struct ChatHost<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub seats: [ChatSeat<'a>; 2],
}

pub struct ChatSeat<'a> {
    pub role: Role,
    pub status: Option<&'a str>,
    pub runtime: Option<&'a Runtime>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub access: Access<'a>,
}

pub enum Access<'a> {
    Native {
        target: &'a HostTarget,
        session_id: &'a str,
    },
    Relay {
        reason: Unavailable,
        recipient: Recipient,
    },
    Unavailable(Unavailable),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recipient {
    host: HostTarget,
    rig_id: String,
    node_id: String,
    logical_id: String,
    session: String,
    runtime: String,
}

const DELIVERY_UNKNOWN: &str =
    "Delivery could not be confirmed. Check the seat before sending again.";

impl Recipient {
    pub fn send(&self, message: &str) -> Result<String, String> {
        let message = message.trim();
        if message.is_empty() || message.len() > 65_536 || message.contains('\0') {
            return Err(
                "Enter a message between 1 and 65,536 bytes without NUL characters.".into(),
            );
        }
        let route = format!(
            "/api/rigs/{}/nodes/{}",
            crate::encode(&self.rig_id),
            crate::encode(&self.logical_id)
        );
        let not_sent = |error: String| {
            let detail: String = error.chars().take(360).collect();
            format!("Seat could not be checked. No message was sent. {detail}")
        };
        let seat = crate::get_target(&self.host, &route).map_err(not_sent)?;
        if seat["nodeId"] != self.node_id
            || seat["canonicalSessionName"] != self.session
            || seat["runtime"] != self.runtime
            || seat["sessionStatus"] != "running"
            || self.runtime == "jcode"
                && seat["nativeRef"].as_str().is_some_and(|id| !id.is_empty())
        {
            return Err(
                "The seat changed while composing. No message was sent. Reopen the latest seat."
                    .into(),
            );
        }

        let bearer = self.host.bearer(false).map_err(not_sent)?;
        let request = self
            .host
            .request("POST", "/api/transport/send", 30, bearer.as_deref())
            .map_err(not_sent)?;
        let result =
            request.send_json(json!({"session": self.session, "text": message, "verify": true}));
        let (status, response) = match result {
            Ok(response) => (response.status(), response),
            Err(ureq::Error::Status(status, response)) => (status, response),
            Err(_) => return Err(DELIVERY_UNKNOWN.into()),
        };
        if matches!(status, 401 | 403) {
            return Err(
                "OpenRig refused authorization. Check the daemon's terminal bearer configuration."
                    .into(),
            );
        }
        let mut body = Vec::new();
        response
            .into_reader()
            .take(65_537)
            .read_to_end(&mut body)
            .map_err(|_| DELIVERY_UNKNOWN)?;
        let receipt: Value = serde_json::from_slice(&body).map_err(|_| DELIVERY_UNKNOWN)?;
        let explain = |summary, keys: &[&str]| {
            if let Some(detail) = keys.iter().find_map(|key| receipt[key].as_str()) {
                let detail = crate::host::redact(detail, bearer.as_deref());
                format!("{summary} {}", detail.chars().take(360).collect::<String>())
            } else {
                String::from(summary)
            }
        };
        if (200..300).contains(&status)
            && receipt["ok"] == true
            && receipt["sent"] == true
            && receipt["sessionName"] == self.session
        {
            let summary = if receipt["verified"] == true {
                "Sent via OpenRig."
            } else {
                "OpenRig reports it sent the message, but delivery was not verified."
            };
            Ok(explain(summary, &["warning"]))
        } else if receipt["sent"] == false || (400..500).contains(&status) && receipt["ok"] == false
        {
            let summary = match receipt["outcome"].as_str() {
                Some("retained" | "held") => "OpenRig did not deliver the message immediately.",
                _ => "OpenRig did not send the message.",
            };
            Err(explain(summary, &["error", "warning", "reason"]))
        } else {
            Err(DELIVERY_UNKNOWN.into())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unavailable {
    Inventory,
    MissingSeat,
    AmbiguousSeat,
    Stopped,
    UnknownState,
    MissingNativeRef,
    UnsupportedRuntime,
    UnknownRuntime,
}

impl Unavailable {
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Inventory => {
                "Seat inventory is unavailable. Check this host in Fleet and try again."
            }
            Self::MissingSeat => {
                "No kernel seat is registered for this role. Configure it in OpenRig."
            }
            Self::AmbiguousSeat => {
                "Multiple kernel seats match this role. Check their identities in Fleet."
            }
            Self::Stopped => {
                "This seat is not running. Start or resume it in OpenRig, then return here."
            }
            Self::UnknownState => {
                "OpenRig did not report whether the seat is running. Check it in Fleet."
            }
            Self::MissingNativeRef => {
                "OpenRig has not reported a Jcode session ID. Check the seat in Fleet or reconnect its Jcode runtime."
            }
            Self::UnsupportedRuntime => {
                "This runtime has no native Jcode chat. Use the seat's terminal in OpenRig."
            }
            Self::UnknownRuntime => {
                "OpenRig did not report a runtime. Check the seat configuration in Fleet."
            }
        }
    }
}

pub fn roster(fleet: &Fleet) -> impl Iterator<Item = ChatHost<'_>> {
    fleet.hosts.iter().map(|host| ChatHost {
        id: &host.id,
        name: &host.name,
        seats: [Role::Advisor, Role::Operator].map(|role| chat_seat(host, role)),
    })
}

fn chat_seat(host: &Host, role: Role) -> ChatSeat<'_> {
    let seat = find_seat(host, role);
    let (status, runtime, last_activity_at, access) = match seat {
        Ok((rig, seat)) => (
            seat.status.as_deref().or(seat.session_status.as_deref()),
            seat.runtime.as_ref(),
            seat.last_activity_at,
            seat_access(host, rig, seat).unwrap_or_else(Access::Unavailable),
        ),
        Err(reason) => (None, None, None, Access::Unavailable(reason)),
    };
    ChatSeat {
        role,
        status,
        runtime,
        last_activity_at,
        access,
    }
}

fn find_seat(host: &Host, role: Role) -> Result<(&crate::fleet::Rig, &Seat), Unavailable> {
    let rigs = host.rigs.as_ref().ok_or(Unavailable::Inventory)?;
    let mut kernel = rigs.iter().filter(|rig| rig.name == "kernel");
    let rig = kernel.next().ok_or(Unavailable::MissingSeat)?;
    if kernel.next().is_some() {
        return Err(Unavailable::AmbiguousSeat);
    }
    let mut matches = rig
        .pods
        .as_ref()
        .ok_or(Unavailable::Inventory)?
        .iter()
        .flat_map(|pod| &pod.seats)
        .filter(|seat| seat.logical_id == role.logical_id());
    let seat = matches.next().ok_or(Unavailable::MissingSeat)?;
    if matches.next().is_some() {
        return Err(Unavailable::AmbiguousSeat);
    }
    Ok((rig, seat))
}

fn seat_access<'a>(
    host: &'a Host,
    rig: &crate::fleet::Rig,
    seat: &'a Seat,
) -> Result<Access<'a>, Unavailable> {
    match seat.session_status.as_deref().or(seat.status.as_deref()) {
        Some("running") => {}
        None | Some("unknown" | "attention_required") => return Err(Unavailable::UnknownState),
        Some(_) => return Err(Unavailable::Stopped),
    }
    let (reason, runtime) = match &seat.runtime {
        Some(Runtime::Jcode) => {
            if let Some(session_id) = &seat.jcode_session_id {
                return Ok(Access::Native {
                    target: &host.target,
                    session_id,
                });
            }
            (Unavailable::MissingNativeRef, "jcode")
        }
        Some(Runtime::Other(runtime)) if runtime != "terminal" => {
            (Unavailable::UnsupportedRuntime, runtime.as_str())
        }
        Some(Runtime::Other(_)) => return Err(Unavailable::UnsupportedRuntime),
        None => return Err(Unavailable::UnknownRuntime),
    };
    let (Some(session), Some(_)) = (&seat.canonical_session_name, host.target.endpoint()) else {
        return Err(reason);
    };
    let Some((name, rig_name)) = session.split_once('@') else {
        return Err(reason);
    };
    if name.is_empty()
        || rig_name != rig.name
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(reason);
    }
    Ok(Access::Relay {
        reason,
        recipient: Recipient {
            host: host.target.clone(),
            rig_id: rig.id.clone(),
            node_id: seat.id.clone(),
            logical_id: seat.logical_id.clone(),
            session: session.clone(),
            runtime: runtime.into(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advisor(fleet: &mut Fleet) -> &mut Seat {
        fleet.hosts[0].rigs.as_mut().unwrap()[0]
            .pods
            .as_mut()
            .unwrap()
            .iter_mut()
            .flat_map(|pod| &mut pod.seats)
            .find(|seat| seat.logical_id == "advisor.lead")
            .unwrap()
    }

    #[test]
    fn chat_lists_agent_roles_per_host_and_opens_exact_native_session() {
        let fleet = Fleet::fixture();
        let hosts: Vec<_> = roster(&fleet).collect();
        assert_eq!(hosts.len(), 2);
        for (index, host) in hosts.iter().enumerate() {
            assert_eq!(
                host.seats.each_ref().map(|seat| seat.role),
                [Role::Advisor, Role::Operator]
            );
            assert_eq!(host.id, fleet.hosts[index].id);
            let Access::Native { target, session_id } = host.seats[1].access else {
                panic!("the operator agent must have a native session");
            };
            assert_eq!(target, &fleet.hosts[index].target);
            assert_eq!(session_id, "session_fox_1700000000000_0123456789abcdef");
        }
    }

    #[test]
    fn chat_distinguishes_unavailable_seats_without_guessing_a_target() {
        let mut fleet = Fleet::fixture();
        let reason = |fleet: &Fleet| match roster(fleet).next().unwrap().seats[0].access {
            Access::Unavailable(reason) => reason,
            Access::Native { .. } | Access::Relay { .. } => panic!("expected an unavailable seat"),
        };
        assert_eq!(reason(&fleet), Unavailable::MissingNativeRef);
        advisor(&mut fleet).runtime = Some(Runtime::Other("codex".into()));
        assert_eq!(reason(&fleet), Unavailable::UnsupportedRuntime);
        advisor(&mut fleet).runtime = None;
        assert_eq!(reason(&fleet), Unavailable::UnknownRuntime);
        advisor(&mut fleet).session_status = Some("stopped".into());
        assert_eq!(reason(&fleet), Unavailable::Stopped);
        advisor(&mut fleet).session_status = None;
        assert_eq!(reason(&fleet), Unavailable::UnknownState);
        advisor(&mut fleet).logical_id = "not-advisor".into();
        assert_eq!(reason(&fleet), Unavailable::MissingSeat);
        fleet.hosts[0].rigs.as_mut().unwrap()[0].pods = None;
        assert_eq!(reason(&fleet), Unavailable::Inventory);
        let duplicate = fleet.hosts[0].rigs.as_ref().unwrap()[0].clone();
        fleet.hosts[0].rigs.as_mut().unwrap().push(duplicate);
        assert_eq!(reason(&fleet), Unavailable::AmbiguousSeat);
    }

    #[test]
    fn chat_relay_checks_recipient_and_never_sends_when_native_session_appears() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        let mut fleet = Fleet::fixture();
        advisor(&mut fleet).canonical_session_name = Some("advisor-lead@kernel".into());
        for (remote, current_native) in [(false, false), (true, false), (true, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            fleet.hosts[0].target = if remote {
                let registry: crate::host::HostRegistry =
                    serde_json::from_value(json!({"hosts":[{"id":"remote", "url":base}]})).unwrap();
                registry.into_targets().next().unwrap().0
            } else {
                HostTarget::primary(&base, "local")
            };
            let [
                ChatSeat {
                    access: Access::Relay { recipient, reason },
                    ..
                },
                _,
            ] = roster(&fleet).next().unwrap().seats
            else {
                panic!("running Advisor without nativeRef must offer direct-host relay");
            };
            assert_eq!(reason, Unavailable::MissingNativeRef);
            assert_eq!(recipient.host.is_local_machine(), !remote);
            let worker = std::thread::spawn(move || {
                for step in 0..if current_native { 1 } else { 2 } {
                    let deadline = std::time::Instant::now() + Duration::from_secs(3);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "fixture request missing"
                                );
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            Err(error) => panic!("{error}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    assert!(line.starts_with(if step == 0 {
                        "GET /api/rigs/local-kernel/nodes/advisor.lead "
                    } else {
                        "POST /api/transport/send "
                    }));
                    let mut len = 0;
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            len = value.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0; len];
                    reader.read_exact(&mut body).unwrap();
                    let response = if step == 0 {
                        json!({"nodeId": "local-advisor.lead", "canonicalSessionName": "advisor-lead@kernel", "runtime":"jcode", "sessionStatus":"running", "nativeRef": current_native.then_some("session_now_native")})
                    } else {
                        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), json!({"session":"advisor-lead@kernel","text":"private fixture message","verify":true}));
                        json!({"ok":true,"sent":true,"verified":true,"sessionName":"advisor-lead@kernel"})
                    }.to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                }
            });
            let result = recipient.send("private fixture message");
            worker.join().unwrap();
            if current_native {
                assert!(result.unwrap_err().contains("No message was sent"));
            } else {
                assert_eq!(result.unwrap(), "Sent via OpenRig.");
            }
        }
    }
}
