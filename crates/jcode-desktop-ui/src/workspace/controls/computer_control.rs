use super::{error, ok};
use crate::desktop_control::Pending;
use gpui::{App, AsyncWindowContext, PromptButton, PromptLevel, Window};
use jcode_computer::{Engine, Request};
use jcode_sdk::desktop_control::{DesktopAction, DesktopResponse, DesktopTarget};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, atomic::Ordering};
use std::time::Instant;

#[derive(Default)]
struct Broker {
    // Serialise execution and prompts so an approved tool call cannot send
    // input while another session is asking the owner for permission.
    busy: bool,
    decisions: HashMap<String, Decision>,
}

#[derive(Clone)]
enum Decision {
    Allow(Arc<Engine>),
    Deny,
}

static BROKER: LazyLock<Mutex<Broker>> = LazyLock::new(Mutex::default);

struct Lease;

impl Lease {
    fn begin(session: &str) -> Result<(Self, Option<Decision>), DesktopResponse> {
        let mut broker = BROKER.lock().unwrap_or_else(|e| e.into_inner());
        if broker.busy {
            return Err(error(
                "desktop_busy",
                "Desktop is handling another computer-use request",
            ));
        }
        broker.busy = true;
        Ok((Self, broker.decisions.get(session).cloned()))
    }

    fn remember(&self, session: String, decision: Decision) {
        let mut broker = BROKER.lock().unwrap_or_else(|e| e.into_inner());
        for decision in broker.decisions.values() {
            if let Decision::Allow(engine) = decision {
                engine.discard_expired();
            }
        }
        broker.decisions.insert(session, decision);
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        BROKER.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
    }
}

pub(super) fn require_local_session(target: &DesktopTarget) -> Result<(), DesktopResponse> {
    if crate::harness::is_remote_session(&target.session_id) {
        return Err(error(
            "desktop_unavailable",
            "computer use requires a local Jcode session",
        ));
    }
    Ok(())
}

pub(super) fn handle(
    pending: Pending,
    cx: &mut AsyncWindowContext,
    route: impl Fn(&DesktopTarget, &Window, &App) -> Result<(), DesktopResponse> + 'static,
) {
    let DesktopAction::ComputerUse { target, input } = pending.request.action else {
        unreachable!("computer broker only accepts computer-use requests");
    };
    let cancelled = pending.cancelled;
    let deadline = pending.deadline;
    let alive = move || !cancelled.load(Ordering::Acquire) && Instant::now() < deadline;

    cx.spawn(async move |cx| {
        let response = async {
            let validate = |cx: &mut AsyncWindowContext| {
                cx.update(|window, cx| {
                    route(&target, window, cx)?;
                    require_local_session(&target)
                }).map_err(|_| error("desktop_unavailable", "Desktop window closed"))?
            };
            validate(cx)?;
            let request = Request::parse(input).map_err(|e| error("invalid_input", &e.to_string()))?;
            let operation = match request {
                Request::Local(output) => return serde_json::to_value(output).map(ok).map_err(|e| error("desktop_response", &e.to_string())),
                Request::Desktop(operation) => operation,
            };
            let (lease, decision) = Lease::begin(&target.session_id)?;
            let expired = || error("desktop_timeout", "Computer use cancelled before execution");
            if !alive() {
                return Err(expired());
            }
            let decision = if let Some(decision) = decision {
                decision
            } else {
                let detail = format!(
                    "Allow Jcode to read and control this Mac for session {}? Screen and window contents may be sent to the model. Input can affect the shared keyboard, pointer and clipboard.",
                    target.session_id
                );
                let answer = cx.update(|window, cx| window.prompt(
                    PromptLevel::Warning,
                    "Computer use",
                    Some(&detail),
                    &[
                        PromptButton::cancel("Not now"),
                        PromptButton::new("Allow for this session"),
                        PromptButton::new("Deny"),
                    ],
                    cx,
                )).map_err(|_| error("desktop_unavailable", "Desktop window closed"))?;
                let answer = answer.await;
                if !alive() {
                    return Err(expired());
                }
                validate(cx)?;
                let decision = match answer {
                    Ok(1) => Decision::Allow(Arc::default()),
                    Ok(2) => Decision::Deny,
                    _ => return Err(error("consent_required", "Computer use was not approved")),
                };
                lease.remember(target.session_id.clone(), decision.clone());
                decision
            };
            let Decision::Allow(engine) = decision else {
                return Err(error("consent_denied", "The user denied computer use for this session"));
            };
            cx.background_executor().spawn(async move {
                let _lease = lease;
                if !alive() {
                    return Err(expired());
                }
                let mut output = engine.execute(operation).map_err(|e| error("computer_use", &e.to_string()))?;
                let metadata = output.metadata.get_or_insert_with(|| json!({}));
                if let Some(metadata) = metadata.as_object_mut() {
                    metadata.insert("executor_pid".into(), json!(std::process::id()));
                }
                serde_json::to_value(output).map(ok).map_err(|e| error("desktop_response", &e.to_string()))
            }).await
        }.await;
        let _ = pending.reply.send(response.unwrap_or_else(|error| error));
    }).detach();
}
