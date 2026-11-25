//! Classifying and namespacing the events a session worker streams.

use std::collections::{HashMap, VecDeque};

use std::sync::mpsc::Receiver;
use std::time::Duration;

use jcode_sdk::{ApiEvent, JcodeClient};

use crate::harness::remote;

use crate::harness::protocol::Update;
use crate::harness::session::SessionCommand;
use crate::harness::updates::UpdateSender;

/// Desktop uses the SDK's shared retry policy. Tests shrink the delays.
pub(super) fn turn_retry_policy() -> jcode_sdk::RetryPolicy {
    #[cfg(test)]
    if let Some(policy) = TEST_RETRY_POLICY.with(|policy| *policy.borrow()) {
        return policy;
    }
    jcode_sdk::RetryPolicy::default()
}

#[cfg(test)]
thread_local! {
    pub(in crate::harness) static TEST_RETRY_POLICY: std::cell::RefCell<Option<jcode_sdk::RetryPolicy>> =
        const { std::cell::RefCell::new(None) };
}

pub(in crate::harness) fn retry_countdown_phase(
    retry: &jcode_sdk::TurnRetry,
    now: std::time::Instant,
) -> Option<String> {
    let remaining = retry.pending()?.saturating_duration_since(now);
    let seconds = remaining
        .as_secs()
        .saturating_add(u64::from(remaining.subsec_nanos() > 0))
        .max(1);
    Some(format!(
        "retrying in {seconds}s ({}/{})",
        retry.attempts(),
        retry.policy().max_attempts
    ))
}

pub(super) fn watch_background_tasks(
    client: &JcodeClient,
    address: &remote::SessionAddress,
    session_id: &str,
    task_id: Option<String>,
    updates: &UpdateSender,
) {
    let reply = client.request(jcode_sdk::api::ApiRequest::WatchBackgroundTasks {
        session_id: address.session_id.clone(),
        task_id,
    });
    // The bridge retries transient census errors on the subscription. Do not
    // turn a failed activity refresh into a failed agent turn.
    if let Ok(frame) = reply
        && matches!(&frame.event, ApiEvent::BackgroundTasks { .. })
        && event_session_id(&frame.event) == Some(address.session_id.as_str())
    {
        let _ = updates.send(Update::Event {
            session_id: session_id.into(),
            event: namespace_event(frame.event, address),
        });
    }
}

/// Surface automatic retry progress in the panel status line. An empty phase
/// clears it.
pub(super) fn send_retry_phase(updates: &UpdateSender, session_id: &str, phase: String) {
    let _ = updates.send(Update::Event {
        session_id: session_id.to_string(),
        event: ApiEvent::ConnectionPhase {
            session_id: session_id.to_string(),
            phase,
        },
    });
}

pub(in crate::harness) fn event_session_id(event: &ApiEvent) -> Option<&str> {
    match event {
        ApiEvent::RuntimeCommandResult { session_id, .. } => Some(session_id.as_str()),
        ApiEvent::TextDelta { session_id, .. }
        | ApiEvent::TextDone { session_id, .. }
        | ApiEvent::TextReplace { session_id, .. }
        | ApiEvent::MemoryInjected { session_id, .. }
        | ApiEvent::ReasoningDelta { session_id, .. }
        | ApiEvent::ReasoningDone { session_id, .. }
        | ApiEvent::ToolStart { session_id, .. }
        | ApiEvent::ToolInputDelta { session_id, .. }
        | ApiEvent::ToolExec { session_id, .. }
        | ApiEvent::ToolDone { session_id, .. }
        | ApiEvent::SidePaneImages { session_id, .. }
        | ApiEvent::SidePanelState { session_id, .. }
        | ApiEvent::AppletState { session_id, .. }
        | ApiEvent::WakeRequested { session_id, .. }
        | ApiEvent::FocusRequested { session_id, .. }
        | ApiEvent::SessionRecovery { session_id, .. }
        | ApiEvent::TokenUsage { session_id, .. }
        | ApiEvent::RequestUsage { session_id, .. }
        | ApiEvent::KvCacheMiss { session_id, .. }
        | ApiEvent::ContextOccupancy { session_id, .. }
        | ApiEvent::ContextCapChanged { session_id, .. }
        | ApiEvent::TurnDone { session_id, .. }
        | ApiEvent::SoftInterruptInjected { session_id, .. }
        | ApiEvent::TurnStopped { session_id, .. }
        | ApiEvent::ProviderGuardrailRetry { session_id, .. }
        | ApiEvent::BackgroundProgress { session_id, .. }
        | ApiEvent::BackgroundTasks { session_id, .. }
        | ApiEvent::PeerUserMessage { session_id, .. }
        | ApiEvent::AgentMessage { session_id, .. }
        | ApiEvent::SwarmWorkerNotice { session_id, .. }
        | ApiEvent::MessageAccepted { session_id }
        | ApiEvent::AskRequest { session_id, .. }
        | ApiEvent::RoutingDecision { session_id, .. }
        | ApiEvent::RoutingGate { session_id, .. }
        | ApiEvent::RoutingGateClosed { session_id, .. }
        | ApiEvent::PermissionRequest { session_id, .. }
        | ApiEvent::SessionStatus { session_id, .. }
        | ApiEvent::SwarmStatus { session_id, .. }
        | ApiEvent::ConnectionPhase { session_id, .. }
        | ApiEvent::ModelInfo { session_id, .. }
        | ApiEvent::Models { session_id, .. }
        | ApiEvent::RuntimeInfo { session_id, .. }
        | ApiEvent::SpeedModeChanged { session_id, .. }
        | ApiEvent::SpeedModeReported { session_id, .. }
        | ApiEvent::FileContent { session_id, .. }
        | ApiEvent::Files { session_id, .. }
        | ApiEvent::TextMatches { session_id, .. }
        | ApiEvent::FileStatus { session_id, .. }
        | ApiEvent::Compacted { session_id, .. }
        | ApiEvent::SessionRenamed { session_id, .. }
        | ApiEvent::History { session_id, .. } => Some(session_id.as_str()),
        _ => None,
    }
    .filter(|id| !id.is_empty())
}

pub(in crate::harness) fn namespace_event(
    mut event: ApiEvent,
    address: &remote::SessionAddress,
) -> ApiEvent {
    if address.host.is_none() {
        return event;
    }
    match &mut event {
        ApiEvent::SwarmStatus {
            session_id,
            members,
            ..
        } => {
            *session_id = address.ui_id(session_id);
            for member in members {
                member.session_id = address.ui_id(&member.session_id);
                member.parent_session_id = member
                    .parent_session_id
                    .as_deref()
                    .map(|id| address.ui_id(id));
            }
        }
        ApiEvent::AgentMessage {
            session_id,
            from_session,
            ..
        } => {
            *session_id = address.ui_id(session_id);
            *from_session = address.ui_id(from_session);
        }
        ApiEvent::TextDelta { session_id, .. }
        | ApiEvent::TextDone { session_id, .. }
        | ApiEvent::TextReplace { session_id, .. }
        | ApiEvent::MemoryInjected { session_id, .. }
        | ApiEvent::ReasoningDelta { session_id, .. }
        | ApiEvent::ReasoningDone { session_id, .. }
        | ApiEvent::ToolStart { session_id, .. }
        | ApiEvent::ToolInputDelta { session_id, .. }
        | ApiEvent::ToolExec { session_id, .. }
        | ApiEvent::ToolDone { session_id, .. }
        | ApiEvent::SidePaneImages { session_id, .. }
        | ApiEvent::SidePanelState { session_id, .. }
        | ApiEvent::AppletState { session_id, .. }
        | ApiEvent::WakeRequested { session_id, .. }
        | ApiEvent::FocusRequested { session_id, .. }
        | ApiEvent::SessionRecovery { session_id, .. }
        | ApiEvent::TokenUsage { session_id, .. }
        | ApiEvent::RequestUsage { session_id, .. }
        | ApiEvent::KvCacheMiss { session_id, .. }
        | ApiEvent::ContextOccupancy { session_id, .. }
        | ApiEvent::ContextCapChanged { session_id, .. }
        | ApiEvent::TurnDone { session_id, .. }
        | ApiEvent::SoftInterruptInjected { session_id, .. }
        | ApiEvent::TurnStopped { session_id, .. }
        | ApiEvent::ProviderGuardrailRetry { session_id, .. }
        | ApiEvent::BackgroundProgress { session_id, .. }
        | ApiEvent::BackgroundTasks { session_id, .. }
        | ApiEvent::PeerUserMessage { session_id, .. }
        | ApiEvent::SwarmWorkerNotice { session_id, .. }
        | ApiEvent::MessageAccepted { session_id }
        | ApiEvent::AskRequest { session_id, .. }
        | ApiEvent::RoutingDecision { session_id, .. }
        | ApiEvent::RoutingGate { session_id, .. }
        | ApiEvent::RoutingGateClosed { session_id, .. }
        | ApiEvent::PermissionRequest { session_id, .. }
        | ApiEvent::SessionStatus { session_id, .. }
        | ApiEvent::ConnectionPhase { session_id, .. }
        | ApiEvent::ModelInfo { session_id, .. }
        | ApiEvent::Models { session_id, .. }
        | ApiEvent::RuntimeInfo { session_id, .. }
        | ApiEvent::SpeedModeChanged { session_id, .. }
        | ApiEvent::SpeedModeReported { session_id, .. }
        | ApiEvent::FileContent { session_id, .. }
        | ApiEvent::Files { session_id, .. }
        | ApiEvent::TextMatches { session_id, .. }
        | ApiEvent::FileStatus { session_id, .. }
        | ApiEvent::Compacted { session_id, .. }
        | ApiEvent::SessionRenamed { session_id, .. }
        | ApiEvent::History { session_id, .. } => *session_id = address.ui_id(session_id),
        ApiEvent::RuntimeCommandResult { session_id, .. } => {
            *session_id = address.ui_id(session_id);
        }
        ApiEvent::Attached { session, .. } | ApiEvent::SessionForked { session } => {
            session.session_id = address.ui_id(&session.session_id);
        }
        ApiEvent::Sessions { sessions, workers } => {
            for session in sessions {
                session.session_id = address.ui_id(&session.session_id);
            }
            // Facts are keyed by the same session ids, so a remote session's
            // renamed ids must be rewritten with them.
            if !workers.is_empty() {
                let renamed = workers
                    .iter()
                    .map(|(id, meta)| (address.ui_id(id), meta.clone()))
                    .collect::<HashMap<_, _>>();
                *workers = renamed;
            }
        }
        _ => {}
    }
    event
}

pub(in crate::harness) fn is_daemon_connection_closed(event: &ApiEvent) -> bool {
    matches!(
        event,
        ApiEvent::Error { message, .. }
            if message.eq_ignore_ascii_case("daemon connection closed")
    )
}

pub(super) fn is_wrong_session_attachment_error(message: &str) -> bool {
    message.contains("this connection is attached to `")
        && message.contains("; attach to it first or use another connection")
}

pub(in crate::harness) fn recover_async_attachment_mismatch(
    event: &ApiEvent,
    unaccepted: &mut VecDeque<SessionCommand>,
    pending: &mut VecDeque<SessionCommand>,
) -> bool {
    let ApiEvent::Error { message, .. } = event else {
        return false;
    };
    if !is_wrong_session_attachment_error(message) {
        return false;
    }
    let Some(submission) = unaccepted.pop_front() else {
        return false;
    };
    pending.push_front(submission);
    true
}

/// Hidden instructions from the collection a session belongs to. Keyed by the
/// desktop session id, which also identifies SSH sessions uniquely. A missing
/// or unreadable store simply sends no reminder.
pub fn collection_reminder(session_id: &str) -> Option<String> {
    jcode_sdk::collections::load()
        .ok()?
        .instructions_for(session_id)
}

pub(super) fn unsupported_attachments(
    client: &JcodeClient,
    attachments: &[jcode_sdk::api::Attachment],
) -> Option<String> {
    (!attachments.is_empty() && !client.supports(jcode_sdk::api::FILE_ATTACHMENTS_CAPABILITY)).then(
        || "This runtime cannot receive file attachments. Update the runtime and reconnect.".into(),
    )
}

pub(in crate::harness) fn queue_wrong_session_retry(
    error: &str,
    pending: &mut VecDeque<SessionCommand>,
    content: String,
    images: Vec<(String, String)>,
    attachments: Vec<jcode_sdk::api::Attachment>,
) -> bool {
    if !is_wrong_session_attachment_error(error) {
        return false;
    }
    pending.push_front(SessionCommand::Send {
        content,
        images,
        attachments,
    });
    true
}

pub(in crate::harness) fn is_already_processing_error(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("already processing a message")
}

pub(super) fn load_skill_catalog(
    client: &JcodeClient,
    real_id: &str,
    session_id: &str,
    updates: &UpdateSender,
) -> bool {
    let Ok(output) =
        client.runtime_command(real_id, jcode_sdk::api::RuntimeCommand::Skills, Vec::new())
    else {
        return false;
    };
    let _ = updates.send(Update::Event {
        session_id: session_id.to_owned(),
        event: ApiEvent::RuntimeCommandResult {
            session_id: session_id.to_owned(),
            command: output.command,
            data: output.data,
            message: String::new(),
        },
    });
    true
}

pub(in crate::harness) fn recover_async_busy(
    event: &ApiEvent,
    unaccepted: &mut VecDeque<SessionCommand>,
    pending: &mut VecDeque<SessionCommand>,
) -> bool {
    if !matches!(event, ApiEvent::Error { message, .. } if is_already_processing_error(message)) {
        return false;
    }
    let Some(submission) = unaccepted.pop_front() else {
        return false;
    };
    pending.push_front(submission);
    true
}

pub(in crate::harness) fn update_turn_activity(event: &ApiEvent, turn_active: &mut bool) {
    match event {
        ApiEvent::SoftInterruptInjected { point, .. } if point == "turn_start" => {
            *turn_active = true
        }
        ApiEvent::MessageAccepted { .. }
        | ApiEvent::TextDelta { .. }
        | ApiEvent::ReasoningDelta { .. }
        | ApiEvent::ToolStart { .. } => *turn_active = true,
        ApiEvent::TurnDone { .. } | ApiEvent::TurnStopped { .. } | ApiEvent::Error { .. } => {
            *turn_active = false
        }
        // `attached` describes the transport, not a model turn. Treating every
        // non-idle status as active routed the first prompt in a fresh desktop
        // panel through `soft_interrupt`; with no turn to interrupt, the prompt
        // stayed queued forever and the panel showed only its local echo.
        ApiEvent::SessionStatus { status, .. }
            if matches!(
                status.as_str(),
                "idle" | "cancelled" | "canceled" | "interrupted" | "crashed" | "error" | "failed"
            ) =>
        {
            *turn_active = false;
        }
        ApiEvent::SessionStatus { status, .. }
            if matches!(
                status.as_str(),
                "generating"
                    | "running"
                    | "processing"
                    | "busy"
                    | "thinking"
                    | "streaming"
                    | "running_tools"
            ) =>
        {
            *turn_active = true;
        }
        _ => {}
    }
}

/// Retain user commands while the self-dev runtime is between processes.
/// Returns true when the panel was closed and the worker should stop.
pub(in crate::harness) fn collect_disconnected_commands(
    commands: &Receiver<SessionCommand>,
    pending: &mut VecDeque<SessionCommand>,
) -> bool {
    while let Ok(command) = commands.try_recv() {
        if matches!(command, SessionCommand::Stop) {
            return true;
        }
        pending.push_back(command);
    }
    false
}

/// Outcome of waiting between reconnect attempts.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::harness) enum ReconnectWait {
    /// The panel closed; end the worker.
    Stop,
    /// The user acted; retry now with the base delay.
    Woken,
    /// The delay elapsed with nothing to do.
    Elapsed,
}

/// Wait out a reconnect delay, but wake at once on any command.
///
/// Backoff can reach 30 s. A Stop must still end the worker promptly, and a
/// user action (sending a message) is a reason to retry now rather than later.
pub(in crate::harness) fn wait_for_reconnect(
    commands: &Receiver<SessionCommand>,
    pending: &mut VecDeque<SessionCommand>,
    delay: Duration,
) -> ReconnectWait {
    match commands.recv_timeout(delay) {
        Ok(SessionCommand::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ReconnectWait::Stop
        }
        Ok(command) => {
            pending.push_back(command);
            if collect_disconnected_commands(commands, pending) {
                ReconnectWait::Stop
            } else {
                ReconnectWait::Woken
            }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ReconnectWait::Elapsed,
    }
}
