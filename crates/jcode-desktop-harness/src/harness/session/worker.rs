//! The session worker: attach, stream events, serve commands and reconnect.

use std::collections::VecDeque;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use jcode_sdk::{ApiEvent, JcodeClient, SessionInfo};

use std::cell::Cell;

use crate::harness::local_runtime::{GracefulDetach, connect};
use crate::harness::protocol::{
    AskResponseError, BoundedHistoryPage, Command, SESSION_IMAGE_LIST_KEY, SessionOperation, Update,
};
use crate::harness::remote;
use crate::harness::remote::transport;
use crate::harness::session::events::{
    ReconnectWait, collect_disconnected_commands, collection_reminder, event_session_id,
    is_daemon_connection_closed, load_skill_catalog, namespace_event, queue_wrong_session_retry,
    recover_async_attachment_mismatch, recover_async_busy, retry_countdown_phase, send_retry_phase,
    turn_retry_policy, unsupported_attachments, update_turn_activity, wait_for_reconnect,
    watch_background_tasks,
};
use crate::harness::session::{
    SessionCommand, WorkerCommand, disconnect, history, next_worker_command, private_route,
    recovery, tab_wait,
};
use crate::harness::updates::UpdateSender;

#[cfg(test)]
pub(in crate::harness) fn session_worker(
    session_id: String,
    commands: Receiver<SessionCommand>,
    updates: UpdateSender,
    initial_client: Option<JcodeClient>,
) {
    session_worker_with_transports(
        session_id,
        commands,
        updates,
        initial_client,
        transport::RemoteTransports::default(),
    );
}

/// One session's dedicated API connection, including an adopted client's reconnect.
pub(super) fn session_worker_with_transports(
    session_id: String,
    commands: Receiver<SessionCommand>,
    updates: UpdateSender,
    initial_client: Option<JcodeClient>,
    transports: transport::RemoteTransports,
) {
    #[cfg(unix)]
    let mut private_route = private_route::PrivateRoute::default();
    // Keep the shared master alive across this worker's API reconnect, but
    // never after the worker stops. Adoption preserves the creator's owner.
    #[cfg(unix)]
    let mut transport_lease = initial_client
        .as_ref()
        .and_then(JcodeClient::shared_ssh_transport);
    session_worker_with_connector(session_id, commands, updates, initial_client, |address| {
        match &address.host {
            Some(host) => {
                let result = transports.connect(host);
                #[cfg(unix)]
                {
                    transport_lease = result
                        .as_ref()
                        .ok()
                        .and_then(JcodeClient::shared_ssh_transport);
                }
                result
            }
            None => {
                #[cfg(unix)]
                if let Some(client) = private_route.connect(&address.session_id)? {
                    return Ok(client);
                }
                connect("panel")
            }
        }
    });
}

/// Longest wait between reconnect attempts for a session that keeps failing.
pub(in crate::harness) const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Exponential backoff for a failing session reconnect.
///
/// A panel whose session the daemon no longer knows (`unknown_session`) used to
/// retry every 300 ms forever. Four such panels made ~40k attach attempts in an
/// hour, each one a fresh bridge connection with its own translation state and
/// daemon subscribe. Transport failures still retry promptly at first, so a
/// restarting runtime reconnects fast, but everything backs off to 30 s.
pub(in crate::harness) fn next_reconnect_delay(
    current: Duration,
    error: &jcode_sdk::Error,
) -> Duration {
    let unknown_session = matches!(
        error.kind,
        jcode_sdk::ErrorKind::Harness(jcode_sdk::api::ErrorCode::UnknownSession)
    );
    let next = if unknown_session {
        current.saturating_mul(4)
    } else {
        current.saturating_mul(2)
    };
    next.min(MAX_RECONNECT_DELAY)
}

/// Log the first few failures, then only occasionally, so a permanently
/// missing session cannot fill the diagnostics log.
pub(in crate::harness) fn should_log_reconnect(failed_attempts: u32) -> bool {
    failed_attempts < 3 || failed_attempts.is_power_of_two()
}

pub(super) fn runtime_owner_is_busy(error: &jcode_sdk::Error) -> bool {
    matches!(
        error.kind,
        jcode_sdk::ErrorKind::Harness(jcode_sdk::api::ErrorCode::Internal)
    ) && error.message.contains("Failed to restore session:")
        && error.message.contains("still has a runtime owner")
}

pub(in crate::harness) fn saved_session_identity(
    home: &std::path::Path,
    real_id: &str,
    session_id: &str,
) -> Option<Update> {
    let meta = jcode_sdk::worker_meta(home, real_id)?;
    if meta.model.is_none() && meta.effort.is_none() {
        return None;
    }
    Some(Update::Event {
        session_id: session_id.to_owned(),
        event: ApiEvent::ModelInfo {
            session_id: session_id.to_owned(),
            provider: meta.provider,
            model: meta.model,
            reasoning_effort: meta.effort,
            auth_method: None,
            account_label: None,
        },
    })
}

pub(super) struct RemoteIdentityLease<'a> {
    session_id: &'a str,
    updates: &'a UpdateSender,
}

impl Drop for RemoteIdentityLease<'_> {
    fn drop(&mut self) {
        let _ = self.updates.send(Update::OpenrigHostIdentity {
            session_id: self.session_id.to_owned(),
            self_host_id: None,
        });
    }
}

/// A `/btw` fork inherits the whole agent prompt, tools included. Steer it to
/// answer from the conversation instead of starting work of its own, the way a
/// side question reads. Sent as a hidden system reminder, never as transcript
/// text.
pub(in crate::harness) const SIDE_QUESTION_REMINDER: &str = "This turn is a side question about the conversation above. Answer briefly and directly from that context. Do not call tools, do not edit files, and do not start a task.";

pub(super) static NEXT_ASK_ATTACHMENT_ID: AtomicU64 = AtomicU64::new(1);

pub(in crate::harness) fn session_worker_with_connector(
    session_id: String,
    commands: Receiver<SessionCommand>,
    updates: UpdateSender,
    mut initial_client: Option<JcodeClient>,
    mut connector: impl FnMut(&remote::SessionAddress) -> jcode_sdk::Result<JcodeClient>,
) {
    let lost = |reason: String| {
        eprintln!("jcode desktop: session {session_id} lost: {reason}");
        let _ = updates.send(Update::SessionLost {
            session_id: session_id.clone(),
            reason,
        });
    };

    let address = match remote::SessionAddress::parse(&session_id) {
        Ok(address) => address,
        Err(error) => {
            let _ = updates.send(Update::Status(error.clone()));
            lost(error);
            return;
        }
    };
    let real_id = address.session_id.as_str();
    if address.host.is_none()
        && let Ok(home) = jcode_base::storage::jcode_dir()
        && let Some(identity) = saved_session_identity(&home, real_id, &session_id)
    {
        let _ = updates.send(identity);
    }
    let base_reconnect_delay = if address.host.is_some() {
        Duration::from_secs(2)
    } else {
        Duration::from_millis(300)
    };
    let mut reconnect_delay = base_reconnect_delay;
    let mut failed_attempts: u32 = 0;
    let mut reported_attaching = false;

    let mut pending = VecDeque::new();
    let mut reported_disconnected_events = false;
    // Survives reconnects: a retry scheduled before a dropped connection
    // fires once the replacement transport is attached.
    let mut turn_retry = jcode_sdk::TurnRetry::new(turn_retry_policy());
    let mut background_output = None;

    // Reconnect in this same worker. The window and workspace stay resident,
    // and history refreshes the panel after the replacement runtime is ready.
    'reconnect: loop {
        let identity_lease = RemoteIdentityLease {
            session_id: &session_id,
            updates: &updates,
        };
        let _ = updates.send(Update::OpenrigHostIdentity {
            session_id: session_id.clone(),
            self_host_id: None,
        });
        if collect_disconnected_commands(&commands, &mut pending) {
            return;
        }
        let already_attached = initial_client.is_some();
        let client = match initial_client
            .take()
            .map(Ok)
            .unwrap_or_else(|| connector(&address))
        {
            Ok(client) => client,
            Err(error) => {
                reported_attaching = false;
                if disconnect::report_protocol_failure(&error, &session_id, &updates) {
                    return;
                }
                if should_log_reconnect(failed_attempts) {
                    lost(format!("{error}; reconnecting"));
                }
                if collect_disconnected_commands(&commands, &mut pending) {
                    return;
                }
                failed_attempts = failed_attempts.saturating_add(1);
                match wait_for_reconnect(&commands, &mut pending, reconnect_delay) {
                    ReconnectWait::Stop => return,
                    ReconnectWait::Woken => reconnect_delay = base_reconnect_delay,
                    ReconnectWait::Elapsed => {
                        reconnect_delay = next_reconnect_delay(reconnect_delay, &error);
                    }
                }
                continue;
            }
        };
        let events = client.events(None);
        let mut pending_ask_ids = None;
        if !already_attached
            && let Err(error) = client
                .attach_session_with_pending_asks(real_id)
                .map(|(_, ids)| pending_ask_ids = ids)
        {
            if disconnect::report_protocol_failure(&error, &session_id, &updates) {
                return;
            }
            if runtime_owner_is_busy(&error) {
                if !reported_attaching {
                    let _ = updates.send(Update::SessionAttaching {
                        session_id: session_id.clone(),
                    });
                    reported_attaching = true;
                }
            } else {
                reported_attaching = false;
                if should_log_reconnect(failed_attempts) {
                    lost(format!("{error}; reconnecting"));
                }
            }
            if collect_disconnected_commands(&commands, &mut pending) {
                return;
            }
            failed_attempts = failed_attempts.saturating_add(1);
            match wait_for_reconnect(&commands, &mut pending, reconnect_delay) {
                ReconnectWait::Stop => return,
                ReconnectWait::Woken => reconnect_delay = base_reconnect_delay,
                ReconnectWait::Elapsed => {
                    reconnect_delay = if runtime_owner_is_busy(&error) {
                        next_reconnect_delay(reconnect_delay, &error).min(Duration::from_secs(2))
                    } else {
                        next_reconnect_delay(reconnect_delay, &error)
                    };
                }
            }
            continue;
        }
        let ask_attachment_id = NEXT_ASK_ATTACHMENT_ID.fetch_add(1, Ordering::Relaxed);
        reconnect_delay = base_reconnect_delay;
        failed_attempts = 0;
        let processing = Cell::new(false);
        let _detach = GracefulDetach::new(&client, real_id, &processing);
        // Activity from the dead transport must not suppress recovery on its
        // replacement. The new attachment supplies its own live state.
        let mut turn_active = false;
        let mut recovery = recovery::Recovery::default();
        // Acceptance and rejection belong to this transport. A lost ack on
        // the previous connection must not block server-directed recovery.
        // Explicit attachment-mismatch retries already live in `pending`.
        let mut unaccepted_sends = VecDeque::new();
        let mut retry_countdown = None;
        eprintln!("jcode desktop: session {session_id} connected");
        // Publish the capability before history can unblock composer sends.
        // The authoritative snapshot follows history recovery below.
        let _ = updates.send(Update::QueueMutationCapability {
            session_id: session_id.clone(),
            supported: client.supports("queued_prompt_mutations_v1"),
        });
        let _ = updates.send(Update::QueueEditingCapability {
            session_id: session_id.clone(),
            supported: client.supports("queued_prompt_editing"),
        });
        if client.supports("queued_prompts") {
            let _ = updates.send(Update::Event {
                session_id: session_id.clone(),
                event: ApiEvent::QueuedPrompts {
                    session_id: session_id.clone(),
                    prompts: Vec::new(),
                },
            });
        }
        let _ = updates.send(Update::SessionConnected {
            session_id: session_id.clone(),
        });
        let _ = updates.send(Update::OpenrigHostIdentity {
            session_id: session_id.clone(),
            self_host_id: client.openrig_self_host_id().map(str::to_owned),
        });

        if client.supports(jcode_sdk::api::BACKGROUND_TASKS_CAPABILITY) {
            watch_background_tasks(
                &client,
                &address,
                &session_id,
                background_output.clone(),
                &updates,
            );
        } else {
            let _ = updates.send(Update::Event {
                session_id: session_id.clone(),
                event: ApiEvent::BackgroundTasks {
                    session_id: session_id.clone(),
                    snapshot: jcode_sdk::api::BackgroundTasksSnapshot {
                        tasks: Vec::new(),
                        output: None,
                    },
                },
            });
        }

        let mut skills_loaded = !client.supports("skill_message_v1")
            || load_skill_catalog(&client, real_id, &session_id, &updates);
        let mut history = history::HistoryRecovery::default();
        history.load(&client, real_id, &session_id, &updates);
        let _ = updates.send(Update::AskAttachment {
            session_id: session_id.clone(),
            attachment_id: ask_attachment_id,
            pending_ask_ids,
        });
        if client.supports("queued_prompts")
            && let Ok(prompts) = client.get_queued_prompts(real_id)
        {
            let _ = updates.send(Update::Event {
                session_id: session_id.clone(),
                event: ApiEvent::QueuedPrompts {
                    session_id: session_id.clone(),
                    prompts,
                },
            });
        }

        // Identity for the panel's status footer: which model and provider are
        // serving this session, and through which credential route. Delivered
        // as a normal event so the panel has one place that absorbs identity,
        // whether it arrives by request (here) or unsolicited (model switches).
        if let Ok(info) = client.get_runtime_info(real_id) {
            let _ = updates.send(Update::Event {
                session_id: session_id.clone(),
                event: ApiEvent::RuntimeInfo {
                    speed_mode: info.speed_mode,
                    speed_options: info.speed_options,
                    session_id: session_id.clone(),
                    provider: info.provider,
                    model: info.model,
                    routes: info.routes,
                    reasoning_effort: info.reasoning_effort,
                    auth_method: info.auth_method,
                    account_label: info.account_label,
                },
            });
        }

        let mut tab_wait = tab_wait::State::default();
        tab_wait.refresh(&client, real_id, &session_id, &updates);
        loop {
            loop {
                let command = match next_worker_command(&commands, &mut pending) {
                    WorkerCommand::Command(command) => command,
                    WorkerCommand::Idle => break,
                    // Hot reload drops the old bridge and all of its command
                    // senders. Do not leave one busy event loop behind for
                    // every panel from every retained UI generation.
                    WorkerCommand::Disconnected => return,
                };
                match command {
                    SessionCommand::WatchBackgroundTaskOutput(task_id) => {
                        if background_output != task_id {
                            background_output = task_id;
                            if client.supports(jcode_sdk::api::BACKGROUND_TASKS_CAPABILITY) {
                                watch_background_tasks(
                                    &client,
                                    &address,
                                    &session_id,
                                    background_output.clone(),
                                    &updates,
                                );
                            }
                        }
                    }
                    SessionCommand::CancelBackgroundTask(task_id) => {
                        let error =
                            match client.request(jcode_sdk::api::ApiRequest::CancelBackgroundTask {
                                session_id: real_id.into(),
                                task_id,
                            }) {
                                Ok(frame) => match frame.event {
                                    ApiEvent::Ok => None,
                                    ApiEvent::Error { message, .. } => Some(message),
                                    _ => Some("Unexpected cancellation response".into()),
                                },
                                Err(error) => Some(error.to_string()),
                            };
                        if let Some(reason) = error {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason,
                            });
                        }
                    }
                    SessionCommand::GetCompactedHistory(visible_messages) => {
                        let result = client
                            .get_compacted_history(real_id, visible_messages)
                            .map_err(|error| error.to_string());
                        let _ = updates.send(Update::HistoryPage {
                            session_id: session_id.clone(),
                            result,
                        });
                    }
                    SessionCommand::GetHistoryPage(before) => {
                        let result = client
                            .get_history_page(real_id, before)
                            .map_err(|error| error.to_string());
                        let _ = updates.send(Update::BoundedHistoryPage {
                            session_id: session_id.clone(),
                            result,
                        });
                    }
                    SessionCommand::GetHistoryImage(key) => {
                        let result = client
                            .get_history_image(real_id, &key)
                            .map_err(|error| error.to_string());
                        let _ = updates.send(Update::HistoryImage {
                            session_id: session_id.clone(),
                            key,
                            result,
                        });
                    }
                    SessionCommand::ListSessionImages => {
                        let result = client
                            .request(jcode_sdk::ApiRequest::ListSessionImages {
                                session_id: real_id.to_string(),
                            })
                            .map_err(|error| error.to_string())
                            .and_then(|frame| match frame.event {
                                ApiEvent::SessionImages { session_id, images }
                                    if session_id == real_id =>
                                {
                                    serde_json::to_string(&images)
                                        .map_err(|error| error.to_string())
                                }
                                ApiEvent::Error { message, .. } => Err(message),
                                _ => Err("Unexpected session images reply".into()),
                            });
                        let _ = updates.send(Update::HistoryImage {
                            session_id: session_id.clone(),
                            key: SESSION_IMAGE_LIST_KEY.into(),
                            result,
                        });
                    }
                    SessionCommand::RefreshRuntime => {
                        if let Ok(info) = client.get_runtime_info(real_id) {
                            let _ = updates.send(Update::Event {
                                session_id: session_id.clone(),
                                event: ApiEvent::RuntimeInfo {
                                    speed_mode: info.speed_mode,
                                    speed_options: info.speed_options,
                                    session_id: session_id.clone(),
                                    provider: info.provider,
                                    model: info.model,
                                    routes: info.routes,
                                    reasoning_effort: info.reasoning_effort,
                                    auth_method: info.auth_method,
                                    account_label: info.account_label,
                                },
                            });
                        }
                    }
                    SessionCommand::TodoState => {
                        let state = client
                            .supports("todo_state")
                            .then(|| client.todo_state(real_id).ok())
                            .flatten();
                        let _ = updates.send(Update::TodoState {
                            session_id: session_id.clone(),
                            state,
                        });
                    }
                    SessionCommand::AckTodoFollowUp(effects) => {
                        if client.supports("todo_state") {
                            let _ = client.ack_todo_follow_up(real_id, effects);
                        }
                    }
                    SessionCommand::AuthChanged(provider) => {
                        // Refresh credentials and model routes for this session.
                        if let Err(error) = client.notify_auth_changed(&provider) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!(
                                    "Signed in, but this session could not refresh its model routes: {error}"
                                ),
                            });
                        }
                    }
                    SessionCommand::QueuePrompt(prompt) => {
                        if prompt.mode == jcode_sdk::api::QueuedPromptMode::Queue
                            && !client.supports("queued_prompt_fifo_v1")
                        {
                            let _ = updates.send(Update::QueuePromptFailed {
                                session_id: session_id.clone(),
                                client_id: prompt.client_id,
                                reason: "This runtime does not support FIFO queued prompts".into(),
                            });
                            continue;
                        }
                        if let Some(reason) = unsupported_attachments(&client, &prompt.attachments)
                        {
                            let _ = updates.send(Update::QueuePromptFailed {
                                session_id: session_id.clone(),
                                client_id: prompt.client_id,
                                reason,
                            });
                            continue;
                        }
                        recovery.supersede();
                        if turn_retry.user_sent() == jcode_sdk::RetryDecision::Cancelled {
                            send_retry_phase(&updates, &session_id, String::new());
                        }
                        let reminder = collection_reminder(&session_id);
                        let client_id = prompt.client_id.clone();
                        if let Err(error) =
                            client.queue_prompt_with_reminder(real_id, prompt, reminder.as_deref())
                        {
                            let _ = updates.send(Update::QueuePromptFailed {
                                session_id: session_id.clone(),
                                client_id,
                                reason: error.to_string(),
                            });
                        }
                    }
                    SessionCommand::RemoveQueuedPrompt(prompt, edit) => {
                        let result = if client.supports("queued_prompt_mutations_v1") {
                            client
                                .remove_queued_prompt(real_id, "jcode-desktop", &prompt.client_id)
                                .map_err(|error| error.to_string())
                        } else {
                            Err("Queue editing is not supported by this runtime".into())
                        };
                        let _ = updates.send(Update::QueueRemovalSettled {
                            session_id: session_id.clone(),
                            prompt,
                            edit,
                            result,
                        });
                    }
                    SessionCommand::SetQueuedPromptEditing(prompt_id, editing) => {
                        let result = if client.supports("queued_prompt_editing") {
                            client
                                .set_queued_prompt_editing(
                                    real_id,
                                    "jcode-desktop",
                                    &prompt_id,
                                    editing,
                                )
                                .map_err(|error| error.to_string())
                        } else {
                            Err("Queue editing is not supported by this runtime".into())
                        };
                        if let Err(reason) = result {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason,
                            });
                        }
                    }
                    SessionCommand::EditQueuedPrompt(prompt_id, content, images, attachments) => {
                        let result =
                            if let Some(reason) = unsupported_attachments(&client, &attachments) {
                                Err(reason)
                            } else if client.supports("queued_prompt_editing") {
                                client
                                    .edit_queued_prompt_with_attachments(
                                        real_id,
                                        "jcode-desktop",
                                        &prompt_id,
                                        content,
                                        images,
                                        attachments,
                                    )
                                    .map_err(|error| error.to_string())
                            } else {
                                Err("Queue editing is not supported by this runtime".into())
                            };
                        let _ = updates.send(Update::QueuedPromptEdited {
                            session_id: session_id.clone(),
                            prompt_id,
                            result,
                        });
                    }
                    SessionCommand::ReorderQueuedPrompts(order) => {
                        let result = if client.supports("queued_prompt_mutations_v1") {
                            client
                                .reorder_queued_prompts(real_id, "jcode-desktop", order)
                                .map_err(|error| error.to_string())
                        } else {
                            Err("Queue reordering is not supported by this runtime".into())
                        };
                        if let Err(reason) = result {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason,
                            });
                        }
                    }
                    SessionCommand::SetQueuedPromptMode(client_id, mode) => {
                        if let Err(error) = client.set_queued_prompt_mode(real_id, &client_id, mode)
                        {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: error.to_string(),
                            });
                        }
                    }
                    SessionCommand::InterruptQueuedPrompt(client_id) => {
                        if let Err(error) = client.interrupt_queued_prompt(real_id, &client_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: error.to_string(),
                            });
                        }
                    }
                    SessionCommand::InterruptQueuedPrompts(ids) => {
                        let result = if client.supports("queued_prompt_batch_interrupt_v1") {
                            client
                                .interrupt_queued_prompts(real_id, ids.clone())
                                .map_err(|error| error.to_string())
                        } else if ids.len() == 1 {
                            client
                                .interrupt_queued_prompt(real_id, &ids[0])
                                .map_err(|error| error.to_string())
                        } else {
                            Err("This runtime cannot interrupt a queue batch. Update the runtime and retry.".to_string())
                        };
                        if let Err(reason) = result {
                            for client_id in ids {
                                let _ = updates.send(Update::QueuePromptFailed {
                                    session_id: session_id.clone(),
                                    client_id,
                                    reason: reason.clone(),
                                });
                            }
                        }
                    }
                    SessionCommand::CancelQueuedPrompt(client_id) => {
                        if let Err(error) = client.cancel_soft_interrupt(real_id, &client_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: error.to_string(),
                            });
                        } else if let Ok(prompts) = client.get_queued_prompts(real_id) {
                            let _ = updates.send(Update::Event {
                                session_id: session_id.clone(),
                                event: ApiEvent::QueuedPrompts {
                                    session_id: session_id.clone(),
                                    prompts,
                                },
                            });
                        }
                    }
                    SessionCommand::SendSkill {
                        content,
                        images,
                        attachments,
                        skill,
                    } => {
                        if let Some(reason) = unsupported_attachments(&client, &attachments) {
                            let _ = updates.send(Update::SendFailed {
                                session_id: session_id.clone(),
                                reason,
                            });
                            continue;
                        }
                        if !client.supports("skill_message_v1") {
                            let _ = updates.send(Update::SendFailed {
                                session_id: session_id.clone(),
                                reason: "This runtime cannot activate skills from Desktop. Update the runtime and reconnect.".into(),
                            });
                            continue;
                        }
                        if turn_active || !unaccepted_sends.is_empty() {
                            let _ = updates.send(Update::SendFailed {
                                session_id: session_id.clone(),
                                reason: "A skill cannot be activated while a turn is running. Try again after it finishes.".into(),
                            });
                            continue;
                        }
                        let reminder = collection_reminder(&session_id);
                        match client.notify(jcode_sdk::api::ApiRequest::SendSkillMessage {
                            session_id: real_id.to_owned(),
                            content: content.clone(),
                            active_skill: skill.clone(),
                            images: images.clone(),
                            attachments: attachments.clone(),
                            system_reminder: reminder,
                        }) {
                            Ok(()) => {
                                unaccepted_sends.push_back(SessionCommand::SendSkill {
                                    content,
                                    images,
                                    attachments,
                                    skill,
                                });
                                let _ = updates.send(Update::MessageSubmitted {
                                    session_id: session_id.clone(),
                                });
                                turn_active = true;
                                processing.set(true);
                            }
                            Err(error) => {
                                let _ = updates.send(Update::SendFailed {
                                    session_id: session_id.clone(),
                                    reason: error.to_string(),
                                });
                            }
                        }
                    }
                    SessionCommand::Send {
                        content,
                        images,
                        attachments,
                    } => {
                        if let Some(reason) = unsupported_attachments(&client, &attachments) {
                            let _ = updates.send(Update::SendFailed {
                                session_id: session_id.clone(),
                                reason,
                            });
                            continue;
                        }
                        recovery.supersede();
                        // New input starts its own request and retry budget.
                        if turn_retry.user_sent() == jcode_sdk::RetryDecision::Cancelled {
                            send_retry_phase(&updates, &session_id, String::new());
                        }
                        let retry_content = content.clone();
                        let retry_images = images.clone();
                        let retry_attachments = attachments.clone();
                        // Only one ordinary send can await acceptance. An old
                        // idle event must not let another send overwrite it.
                        let steering = turn_active || !unaccepted_sends.is_empty();
                        let result = if steering && !attachments.is_empty() {
                            static NEXT_STEER_ID: std::sync::atomic::AtomicU64 =
                                std::sync::atomic::AtomicU64::new(0);
                            let sequence =
                                NEXT_STEER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let time = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos();
                            client.queue_prompt(
                                real_id,
                                jcode_sdk::api::QueuedPrompt {
                                    client_id: format!(
                                        "desktop-steer-{}-{time}-{sequence}",
                                        std::process::id()
                                    ),
                                    content: content.clone(),
                                    images,
                                    attachments,
                                    mode: jcode_sdk::api::QueuedPromptMode::Steer,
                                    active_skill: None,
                                    editing: false,
                                },
                            )
                        } else if steering {
                            client.soft_interrupt_with_images(real_id, &content, images, true)
                        } else {
                            // Collection instructions ride along hidden on every
                            // new turn, read fresh so edits apply immediately.
                            let reminder = collection_reminder(&session_id);
                            client.notify(jcode_sdk::api::ApiRequest::SendMessage {
                                session_id: real_id.to_owned(),
                                content: content.clone(),
                                images,
                                attachments,
                                system_reminder: reminder
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned),
                                no_reply: false,
                            })
                        };
                        if let Err(error) = result {
                            // A daemon reload can briefly hand this SDK socket
                            // the state of another legacy subscription. Never
                            // surface that transport mix-up as a failed user
                            // message: replace the connection and retry the
                            // original submission on the correctly attached
                            // session worker.
                            if queue_wrong_session_retry(
                                &error.to_string(),
                                &mut pending,
                                retry_content,
                                retry_images,
                                retry_attachments,
                            ) {
                                lost("session connection changed; reconnecting".into());
                                continue 'reconnect;
                            }
                            let _ = updates.send(Update::SendFailed {
                                session_id: session_id.clone(),
                                reason: error.to_string(),
                            });
                        } else {
                            // Steering is acknowledged by the synchronous SDK
                            // reply, not a MessageAccepted stream event. Keeping
                            // it here would replay an already-delivered message
                            // when a later ordinary send gets rejected.
                            if !steering {
                                unaccepted_sends.push_back(SessionCommand::Send {
                                    content: retry_content,
                                    images: retry_images,
                                    attachments: retry_attachments,
                                });
                            }
                            let _ = updates.send(Update::MessageSubmitted {
                                session_id: session_id.clone(),
                            });
                            // Mark active immediately instead of waiting for a
                            // streamed status event, so two rapidly submitted
                            // prompts cannot both take the SendMessage path.
                            turn_active = true;
                            processing.set(true);
                        }
                    }
                    SessionCommand::SendSideQuestion {
                        question,
                        legacy_archive,
                    } => {
                        recovery.supersede();
                        // New runtimes create marked, hidden forks atomically.
                        // Only older runtimes need the compatibility archive.
                        if legacy_archive && let Err(error) = client.archive_session(real_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Could not hide the side question: {error}"),
                            });
                            continue;
                        }
                        // The child fork inherits the full agent prompt, tools
                        // included. Steer it to answer from the conversation
                        // without starting work, and keep that steer hidden so
                        // the promoted transcript reads as the question alone.
                        let result = client.send_message_with_reminder(
                            real_id,
                            &question,
                            Vec::new(),
                            Some(SIDE_QUESTION_REMINDER),
                            None,
                        );
                        match result {
                            Ok(()) => {
                                turn_active = true;
                                processing.set(true);
                            }
                            Err(error) => {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id: session_id.clone(),
                                    reason: format!("Could not ask the side question: {error}"),
                                });
                            }
                        }
                    }
                    SessionCommand::ContinueSideQuestion { question } => {
                        recovery.supersede();
                        let result = if turn_active {
                            Err(jcode_sdk::Error::new(
                                jcode_sdk::ErrorKind::InvalidOption,
                                "This side question is already answering.",
                            ))
                        } else {
                            client.send_message_with_reminder(
                                real_id,
                                &question,
                                Vec::new(),
                                Some(SIDE_QUESTION_REMINDER),
                                None,
                            )
                        };
                        match result {
                            Ok(()) => {
                                turn_active = true;
                                processing.set(true);
                            }
                            Err(error) => {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id: session_id.clone(),
                                    reason: format!("Could not send the follow-up: {error}"),
                                });
                            }
                        }
                    }
                    SessionCommand::ReplaySideQuestionHistory => {
                        let _ = updates.send(Update::SessionConnected {
                            session_id: session_id.clone(),
                        });
                        history.load(&client, real_id, &session_id, &updates);
                    }
                    SessionCommand::Cancel => {
                        recovery.supersede();
                        if turn_retry.user_cancelled() == jcode_sdk::RetryDecision::Cancelled {
                            send_retry_phase(&updates, &session_id, String::new());
                        }
                        let client = client.clone();
                        let real_id = real_id.to_string();
                        std::thread::spawn(move || {
                            let _ = client.cancel(&real_id);
                        });
                    }
                    SessionCommand::ForceStopTurn => {
                        recovery.supersede();
                        let client = client.clone();
                        let real_id = real_id.to_string();
                        let updates = updates.clone();
                        let session_id = session_id.clone();
                        std::thread::spawn(move || {
                            let result = if client.supports("force_stop_turn_v1") {
                                client.force_stop_turn(&real_id)
                            } else {
                                Err(jcode_sdk::Error::new(
                                    jcode_sdk::ErrorKind::UnsupportedCapability,
                                    "This runtime does not support force stop. Update the runtime and reconnect.",
                                ))
                            };
                            if let Err(error) = result
                                && error.kind != jcode_sdk::ErrorKind::Timeout
                            {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id,
                                    reason: format!("Could not force stop: {error}"),
                                });
                            }
                        });
                    }
                    SessionCommand::AskResponse {
                        request_id,
                        attachment_id,
                        attempt_id,
                        response,
                    } => {
                        let result = if attachment_id != ask_attachment_id {
                            Err(AskResponseError::ConnectionChanged)
                        } else {
                            client
                                .respond_to_ask(real_id, &request_id, response)
                                .map_err(|error| {
                                    if error.kind == jcode_sdk::ErrorKind::AskNotPending {
                                        AskResponseError::Stale
                                    } else {
                                        AskResponseError::Failed(format!(
                                            "Could not send your answer: {error}"
                                        ))
                                    }
                                })
                        };
                        let _ = updates.send(Update::AskResponseSettled {
                            session_id: session_id.clone(),
                            request_id,
                            attempt_id,
                            result,
                        });
                    }
                    SessionCommand::RoutingGateAnswer { gate_id, choice } => {
                        let result = client
                            .answer_routing_gate(real_id, &gate_id, choice)
                            .map_err(|error| error.message);
                        let _ = updates.send(Update::RoutingGateAnswerSettled {
                            session_id: session_id.clone(),
                            gate_id,
                            result,
                        });
                    }
                    SessionCommand::RoutingIntent(action) => {
                        let result = client
                            .routing_intent(real_id, action)
                            .map_err(|error| error.message);
                        let _ = updates.send(Update::RoutingIntentReply {
                            session_id: session_id.clone(),
                            result,
                        });
                    }
                    SessionCommand::BackgroundTool => {
                        if let Err(error) = client.background_tool(real_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!(
                                    "Could not move the running tool to the background: {error}"
                                ),
                            });
                        }
                    }
                    SessionCommand::ForkWithPrompt { prompt, internal } => {
                        let fork = (|| -> jcode_sdk::Result<(SessionInfo, JcodeClient)> {
                            let session = client.fork_session(real_id)?;
                            let child_address = remote::SessionAddress {
                                host: address.host.clone(),
                                session_id: session.session_id.clone(),
                            };
                            let child = connector(&child_address)?;
                            child.attach_session(&session.session_id)?;
                            Ok((address.session_info(session), child))
                        })();
                        match fork {
                            Ok((session, child)) => {
                                let _ = internal.send(Command::ForkedInternal {
                                    session,
                                    client: child,
                                    prompt,
                                });
                            }
                            Err(error) => {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id: session_id.clone(),
                                    reason: format!("Failed to fork session with prompt: {error}"),
                                });
                            }
                        }
                    }
                    SessionCommand::SideQuestion { question, internal } => {
                        let fork = (|| -> jcode_sdk::Result<(SessionInfo, JcodeClient)> {
                            let legacy_archive =
                                !client.supports(jcode_sdk::SIDE_QUESTIONS_CAPABILITY);
                            let child_id = if legacy_archive {
                                client.fork_session(real_id)?.session_id
                            } else {
                                client.fork_side_question(real_id)?.session_id
                            };
                            let child_address = remote::SessionAddress {
                                host: address.host.clone(),
                                session_id: child_id,
                            };
                            let child = connector(&child_address)?;
                            let mut session = child.attach_session(&child_address.session_id)?;
                            session.archived = legacy_archive;
                            Ok((address.session_info(session), child))
                        })();
                        match fork {
                            Ok((session, child)) => {
                                let _ = internal.send(Command::SideQuestionInternal {
                                    parent_session_id: session_id.clone(),
                                    session,
                                    client: child,
                                    question,
                                });
                            }
                            Err(error) => {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id: session_id.clone(),
                                    reason: format!("Failed to ask a side question: {error}"),
                                });
                            }
                        }
                    }
                    SessionCommand::RestoreSession => {
                        if let Err(error) = client.restore_session(real_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Could not restore the session: {error}"),
                            });
                        }
                    }
                    SessionCommand::Transfer => match client.transfer_session(real_id) {
                        Ok(session) => {
                            let _ = updates.send(Update::SessionForked {
                                session: address.session_info(session),
                                prompt: None,
                            });
                        }
                        Err(error) => {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Failed to transfer session: {error}"),
                            });
                        }
                    },
                    SessionCommand::Fork => match client.fork_session(real_id) {
                        Ok(session) => {
                            let session = address.session_info(session);
                            let _ = updates.send(Update::SessionForked {
                                session,
                                prompt: None,
                            });
                        }
                        Err(error) => {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Failed to fork session: {error}"),
                            });
                        }
                    },
                    SessionCommand::SetModel(model) => {
                        if let Err(error) = client.set_model(real_id, &model) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Failed to switch model: {error}"),
                            });
                        }
                    }
                    SessionCommand::CancelPendingModel => {
                        if let Err(error) = client.cancel_pending_model(real_id) {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Failed to cancel the model switch: {error}"),
                            });
                        }
                    }
                    SessionCommand::RuntimeCommand(command, args) => {
                        // Route through the session's own client and native id,
                        // including remote transports, never the local runtime.
                        match client.runtime_command(real_id, command, args) {
                            Ok(mut output) => {
                                output.session_id = session_id.clone();
                                let _ = updates.send(Update::Event {
                                    session_id: session_id.clone(),
                                    event: ApiEvent::RuntimeCommandResult {
                                        session_id: output.session_id,
                                        command: output.command,
                                        data: output.data,
                                        message: output.message,
                                    },
                                });
                            }
                            Err(error) => {
                                let _ = updates.send(Update::CommandFailed {
                                    session_id: session_id.clone(),
                                    reason: format!("Runtime command failed: {error}"),
                                });
                            }
                        }
                    }
                    SessionCommand::Operation(operation) => {
                        if matches!(
                            operation,
                            SessionOperation::Clear
                                | SessionOperation::Rewind(_)
                                | SessionOperation::RewindUndo
                        ) {
                            recovery.supersede();
                            if turn_retry.user_cancelled() == jcode_sdk::RetryDecision::Cancelled {
                                send_retry_phase(&updates, &session_id, String::new());
                            }
                        }
                        // Effort settles the panel's optimistic label either
                        // way, so its outcome is reported rather than only a
                        // failure. The identity broadcast that follows carries
                        // the level the provider actually applied.
                        if let SessionOperation::SetEffort(effort) = &operation {
                            let error = client
                                .set_reasoning_effort(real_id, effort)
                                .err()
                                .map(|error| error.to_string());
                            let _ = updates.send(Update::EffortSettled {
                                session_id: session_id.clone(),
                                effort: effort.clone(),
                                error,
                            });
                            continue;
                        }
                        if let SessionOperation::SetSpeed { serial, mode } = operation {
                            let result = client
                                .set_speed_mode(mode)
                                .map_err(|error| error.to_string());
                            let _ = updates.send(Update::SpeedSettled {
                                session_id: session_id.clone(),
                                serial,
                                result,
                            });
                            continue;
                        }
                        if let SessionOperation::SetModelWithSpeed {
                            ref model,
                            ref effort,
                            serial,
                            mode,
                        } = operation
                        {
                            let result = client
                                .set_model_switch(real_id, model)
                                .map_err(|error| format!("Failed to switch model: {error}"))
                                .and_then(|switched| {
                                    // A switch queued behind a running turn has
                                    // not happened yet. Effort and speed meant
                                    // for the new model would land on the one
                                    // still running, so they are not sent.
                                    if switched == jcode_sdk::api::ModelSwitchState::Queued {
                                        return Err(
                                            "The model switch is queued until this turn ends. \
                                             Choose its effort and speed again afterwards."
                                                .to_string(),
                                        );
                                    }
                                    if let Some(effort) = effort
                                        && let Err(error) =
                                            client.set_reasoning_effort(real_id, effort)
                                    {
                                        let _ = updates.send(Update::CommandFailed {
                                            session_id: session_id.clone(),
                                            reason: format!(
                                                "Failed to set reasoning effort: {error}"
                                            ),
                                        });
                                    }
                                    client
                                        .set_speed_mode(mode)
                                        .map_err(|error| error.to_string())
                                });
                            let _ = updates.send(Update::SpeedSettled {
                                session_id: session_id.clone(),
                                serial,
                                result,
                            });
                            continue;
                        }
                        if let SessionOperation::SetSaved(saved, label) = &operation {
                            match client.set_session_saved(real_id, *saved, label.clone()) {
                                Ok(()) => {
                                    let _ = updates.send(Update::SessionSaved {
                                        session_id: session_id.clone(),
                                        saved: *saved,
                                        label: label.clone(),
                                    });
                                }
                                Err(error) => {
                                    let _ = updates.send(Update::SessionSaveFailed {
                                        session_id: session_id.clone(),
                                        saved: *saved,
                                        error: error.to_string(),
                                    });
                                }
                            }
                            continue;
                        }
                        let rewinds = matches!(
                            &operation,
                            SessionOperation::Rewind(_)
                                | SessionOperation::RewindTranscript(_)
                                | SessionOperation::RewindUndo
                        );
                        let mut result = match operation {
                            SessionOperation::Clear => client.clear(real_id),
                            SessionOperation::Compact => {
                                let event = match client.compact(real_id) {
                                    Ok(message) => ApiEvent::Compacted {
                                        session_id: session_id.clone(),
                                        message,
                                    },
                                    Err(error) => ApiEvent::BackgroundProgress {
                                        session_id: session_id.clone(),
                                        task_id: "context-compaction".into(),
                                        label: "Context compaction".into(),
                                        percent: None,
                                        summary: format!("Compaction failed: {error}"),
                                        detail: None,
                                        done: true,
                                    },
                                };
                                let _ = updates.send(Update::Event {
                                    session_id: session_id.clone(),
                                    event,
                                });
                                Ok(())
                            }
                            SessionOperation::SetEffort(_)
                            | SessionOperation::SetSpeed { .. }
                            | SessionOperation::SetModelWithSpeed { .. }
                            | SessionOperation::SetSaved(..) => Ok(()),
                            SessionOperation::AppletAction {
                                instance,
                                action,
                                state,
                                source_key,
                            } => {
                                client.applet_action(real_id, &instance, action, state, source_key)
                            }
                            SessionOperation::CloseApplet(instance) => {
                                client.close_applet(real_id, &instance)
                            }
                            SessionOperation::Rename(title) => {
                                client.rename_session(real_id, title)
                            }
                            SessionOperation::Rewind(index) => client.rewind(real_id, index),
                            SessionOperation::RewindTranscript(target) => {
                                client.rewind_transcript(real_id, target)
                            }
                            SessionOperation::RewindUndo => client.rewind_undo(real_id),
                        };
                        if rewinds && result.is_ok() {
                            let bounded = client.supports("bounded_history_v1");
                            let page = if bounded {
                                client.get_history_page(real_id, None)
                            } else {
                                client
                                    .get_history_with_images(real_id)
                                    .map(|(messages, images)| BoundedHistoryPage {
                                        messages,
                                        images,
                                        before: None,
                                        prompt_offset: None,
                                    })
                            };
                            result = page.map(|page| {
                                let _ = updates.send(Update::RewoundHistory {
                                    session_id: session_id.clone(),
                                    page,
                                    bounded,
                                });
                            });
                        }
                        if let Err(error) = result {
                            let _ = updates.send(Update::CommandFailed {
                                session_id: session_id.clone(),
                                reason: format!("Command failed: {error}"),
                            });
                        }
                    }
                    SessionCommand::Stop => return,
                }
            }

            // Update the visible countdown while waiting, not just when the
            // error arrives. A frozen "in 3s" label looks like a stalled retry.
            if let Some(phase) = retry_countdown_phase(&turn_retry, std::time::Instant::now()) {
                if retry_countdown.as_ref() != Some(&phase) {
                    send_retry_phase(&updates, &session_id, phase.clone());
                    retry_countdown = Some(phase);
                }
            } else {
                retry_countdown = None;
            }

            // A transient turn failure scheduled an automatic continuation.
            // The failed prompt is already in the transcript, so resume the
            // session with a hidden reminder rather than resending it.
            if !turn_active
                && unaccepted_sends.is_empty()
                && turn_retry.take_due(std::time::Instant::now())
            {
                recovery.supersede();
                match client
                    .send_system_reminder(real_id, jcode_sdk::turn_retry::CONTINUATION_REMINDER)
                {
                    Ok(()) => {
                        eprintln!(
                            "jcode desktop: session {session_id} auto-retry {}/{}",
                            turn_retry.attempts(),
                            turn_retry.policy().max_attempts
                        );
                        turn_active = true;
                        processing.set(true);
                        send_retry_phase(
                            &updates,
                            &session_id,
                            format!(
                                "retrying ({}/{})",
                                turn_retry.attempts(),
                                turn_retry.policy().max_attempts
                            ),
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "jcode desktop: session {session_id} auto-retry send failed: {error}"
                        );
                        turn_retry.send_failed(std::time::Instant::now());
                    }
                }
            }

            // A failed bootstrap read is not a turn failure. Retry just the
            // read with a bounded budget, leaving commands/events live between
            // attempts instead of silently keeping an empty transcript forever.
            if history.due(std::time::Instant::now()) && !client.is_closed() {
                history.load(&client, real_id, &session_id, &updates);
            }

            if !turn_active && tab_wait.refresh_due() {
                tab_wait.refresh(&client, real_id, &session_id, &updates);
            }
            let event_wait_started = std::time::Instant::now();
            if let Some(event) = events.next_timeout(Duration::from_millis(100)) {
                reported_disconnected_events = false;
                if recover_async_attachment_mismatch(&event, &mut unaccepted_sends, &mut pending) {
                    lost("session connection changed; reconnecting".into());
                    continue 'reconnect;
                }
                // The API bridge emits this event immediately before closing its
                // stream when the legacy daemon connection disappears. It is a
                // transport lifecycle notification, not a failed model turn.
                // Reconnect now rather than rendering a scary transcript error
                // and waiting for the socket reader to notice EOF separately.
                if is_daemon_connection_closed(&event) {
                    lost("runtime connection closed; reconnecting".into());
                    break;
                }
                // The API socket broadcasts streaming events for every live
                // session. A busy TUI session must not make a newly-created
                // desktop panel look busy: doing so routes its first prompt
                // through soft_interrupt, where it waits forever because that
                // new session has no active turn to interrupt.
                // Some bridge frames (including AskRequest) omit the identity
                // when the connection owns the request but has not named it.
                if event_session_id(&event).is_some_and(|id| id != real_id) {
                    continue;
                }
                if matches!(event, ApiEvent::History { .. }) {
                    history.loaded();
                }
                if let ApiEvent::SessionRecovery {
                    continuation_message,
                    ..
                } = &event
                {
                    if recovery.notice(continuation_message) {
                        let _ = updates.send(Update::Event {
                            session_id: session_id.clone(),
                            event: namespace_event(event, &address),
                        });
                    }
                    continue;
                }
                if matches!(
                    event,
                    ApiEvent::TextDelta { .. }
                        | ApiEvent::ReasoningDelta { .. }
                        | ApiEvent::ToolStart { .. }
                        | ApiEvent::AskRequest { .. }
                        | ApiEvent::TurnDone { .. }
                        | ApiEvent::TurnStopped { .. }
                ) || matches!(&event, ApiEvent::SessionStatus { status, .. }
                        if matches!(status.as_str(), "cancelled" | "canceled" | "interrupted" | "crashed" | "error" | "failed"))
                {
                    recovery.supersede();
                }
                if recover_async_busy(&event, &mut unaccepted_sends, &mut pending) {
                    // send_message is fire-and-forget, so its busy rejection
                    // arrives here, not in its Result. Replay the intact prompt
                    // through steering before accepting another composer send.
                    turn_active = true;
                    processing.set(true);
                    continue;
                }
                if matches!(event, ApiEvent::MessageAccepted { .. }) {
                    unaccepted_sends.pop_front();
                } else if matches!(event, ApiEvent::Error { .. }) {
                    // An unrelated terminal failure must not leave an old
                    // payload eligible for recovery on a later busy error.
                    unaccepted_sends.clear();
                }
                let was_active = turn_active;
                update_turn_activity(&event, &mut turn_active);
                processing.set(turn_active);
                let retry_decision = turn_retry.observe(&event, std::time::Instant::now());
                let retry_failure = match &event {
                    ApiEvent::Error { message, .. }
                        if matches!(
                            retry_decision,
                            jcode_sdk::RetryDecision::GaveUp {
                                exhausted: true,
                                ..
                            }
                        ) =>
                    {
                        Some(message.clone())
                    }
                    _ => None,
                };
                let closes_turn =
                    matches!(event, ApiEvent::Error { .. } | ApiEvent::TurnDone { .. });
                if closes_turn && !skills_loaded {
                    skills_loaded = load_skill_catalog(&client, real_id, &session_id, &updates);
                }
                let _ = updates.send(Update::Event {
                    session_id: session_id.clone(),
                    event: namespace_event(event, &address),
                });
                if was_active && !turn_active {
                    tab_wait.refresh(&client, real_id, &session_id, &updates);
                }
                // The panel clears its phase when a turn ends, so announce
                // the pending retry after forwarding the closing event.
                match retry_decision {
                    jcode_sdk::RetryDecision::GaveUp {
                        attempts,
                        exhausted: true,
                    } if attempts > 0 => {
                        send_retry_phase(&updates, &session_id, String::new());
                        let _ = updates.send(Update::CommandFailed {
                            session_id: session_id.clone(),
                            reason: format!("Automatic recovery paused after {attempts} retries. Check the connection or provider account, choose another model if needed, then send a message to continue. Last failure: {}", retry_failure.as_deref().unwrap_or("unknown provider failure")),
                        });
                    }
                    jcode_sdk::RetryDecision::Cancelled => {
                        send_retry_phase(&updates, &session_id, String::new());
                    }
                    _ if closes_turn && turn_retry.pending().is_some() => {
                        let phase = retry_countdown_phase(&turn_retry, std::time::Instant::now())
                            .expect("pending retry has a countdown");
                        retry_countdown = Some(phase.clone());
                        send_retry_phase(&updates, &session_id, phase);
                    }
                    _ => {}
                }
            } else if client.is_closed() {
                if let Some(error) = client.disconnect_reason()
                    && disconnect::report_protocol_failure(&error, &session_id, &updates)
                {
                    return;
                }
                lost("runtime connection closed; reconnecting".into());
                break;
            } else if event_wait_started.elapsed() < Duration::from_millis(10) {
                // `EventStream::next_timeout` also returns `None` when its
                // receiver disconnects. In that case it returns immediately,
                // and an otherwise healthy client can turn this loop into a
                // full-core spin. Keep command latency low while placing a
                // firm ceiling on CPU use until the transport reconnects.
                if !reported_disconnected_events {
                    eprintln!(
                        "jcode desktop: event stream for {session_id} returned immediately; backing off"
                    );
                    reported_disconnected_events = true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        drop(identity_lease);
        if address.host.is_some() {
            // A reachable SSH server whose API immediately closes should not
            // produce an unbounded rapid reconnect storm either.
            if wait_for_reconnect(&commands, &mut pending, reconnect_delay) == ReconnectWait::Stop {
                return;
            }
        }
    }
}
