//! The coordinator thread: owns the per-panel session workers and routes
//! `Command`s to them, to the local runtime, or to remote hosts.

use std::collections::{HashMap, HashSet};

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::harness::catalog::persisted_todo_snapshot;
use crate::harness::catalog::{EditWatch, SessionRefresh};
use crate::harness::local_runtime::{connect, start_local_runtime};
use crate::harness::protocol::{AskResponseError, Command, SessionOperation, Update};
use crate::harness::remote;
use crate::harness::remote::sessions::{create_remote_session, list_remote_sessions};
use crate::harness::remote::transport;
use crate::harness::session::oneshot::{read_context_breakdown, set_saved_detached, shake_tools};
use crate::harness::session::{
    SessionCommand, ensure_session_worker, send_to_session_worker, spawn_attached_session_worker,
    spawn_session_worker,
};
use crate::harness::session::{search, side_questions, spawn_profile};
use crate::harness::updates::UpdateSender;

pub(super) fn run(updates: UpdateSender, commands: Receiver<Command>, internal: Sender<Command>) {
    // Local startup must not gate SSH work. In particular a broken/missing
    // local runtime cannot strand restored remote panels or remote creates.
    let (local_lifetime, local_stopped) = channel::<()>();
    std::thread::Builder::new()
        .name("jcode-bridge-local-startup".into())
        .spawn({
            let updates = updates.clone();
            let internal = internal.clone();
            move || {
                if let Some(_client) = start_local_runtime(updates, &internal)
                    && cfg!(target_os = "macos")
                {
                    // The rollout daemon checks for a live Desktop peer for up
                    // to 60 seconds after launch. Release the startup client
                    // afterward so the bridge can still shut down when idle.
                    let _ = local_stopped.recv_timeout(Duration::from_secs(60));
                }
            }
        })
        .expect("spawn local startup thread");
    // One pool per bridge run, never a process-global/dylib-lifetime singleton.
    run_with_transports(
        updates,
        commands,
        internal,
        transport::RemoteTransports::default(),
    );
    drop(local_lifetime);
}

pub(super) fn run_with_transports(
    updates: UpdateSender,
    commands: Receiver<Command>,
    internal: Sender<Command>,
    transports: transport::RemoteTransports,
) {
    let session_refresh = SessionRefresh::spawn(updates.clone());

    let mut edit_watch = EditWatch::new();

    // Per-session workers, keyed by session id.
    let mut workers: HashMap<String, Sender<SessionCommand>> = HashMap::new();
    let mut watched_sessions = HashSet::new();

    loop {
        let now = std::time::Instant::now();
        if edit_watch.poll(now) {
            session_refresh.request(false);
        }
        let command = match match edit_watch.next_check() {
            Some(deadline) => commands.recv_timeout(deadline.saturating_duration_since(now)),
            None => commands
                .recv()
                .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
        } {
            Ok(command) => command,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match command {
            Command::Shutdown => break,
            Command::SetCatalogForeground(focused) => {
                edit_watch.set_foreground(focused);
            }
            Command::RefreshSessions => session_refresh.request(false),
            Command::RefreshSessionsInitial => session_refresh.request(true),
            Command::SearchSessions { query, generation } => {
                search::spawn_search(updates.clone(), query, generation)
            }
            Command::CreateSession {
                working_dir,
                request_id,
            } => {
                // A fresh connection per creation: an existing connection
                // returns its already-attached session instead of a new one.
                let updates = updates.clone();
                let internal = internal.clone();
                std::thread::Builder::new()
                    .name("jcode-bridge-create".into())
                    .spawn(move || {
                        loop {
                            let result =
                                spawn_profile::create(|| connect("create"), working_dir.clone());
                            match result {
                                Ok((session, client)) => {
                                    let _ = internal.send(Command::CreatedInternal {
                                        session,
                                        client,
                                        request_id,
                                    });
                                    break;
                                }
                                Err(error) => {
                                    if let Some(session_id) = &request_id
                                        && !spawn_profile::retry_startup(Some(session_id))
                                    {
                                        let _ = updates.send(Update::CommandFailed {
                                            session_id: session_id.clone(),
                                            reason: format!("Could not start session: {error}"),
                                        });
                                    }
                                    if updates
                                        .send(Update::Status(format!(
                                            "create session failed: {error}"
                                        )))
                                        .is_err()
                                        || !spawn_profile::retry_startup(request_id.as_deref())
                                    {
                                        break;
                                    }
                                    // Keep the startup draft usable across a daemon
                                    // restart without requiring a desktop restart.
                                    std::thread::sleep(Duration::from_millis(500));
                                }
                            }
                        }
                    })
                    .expect("spawn create thread");
            }
            Command::ListRemoteSessions { host, request_id } => {
                let transports = transports.clone();
                let updates = updates.clone();
                std::thread::spawn(move || {
                    let result = list_remote_sessions(&host, |host| transports.connect(host));
                    let _ = updates.send(Update::RemoteSessions { request_id, result });
                });
            }
            Command::CreateRemoteSession {
                host,
                working_dir,
                request_id,
            } => {
                let updates = updates.clone();
                let internal = internal.clone();
                let transports = transports.clone();
                std::thread::Builder::new()
                    .name("jcode-bridge-ssh-create".into())
                    .spawn(move || {
                        let progress_updates = updates.clone();
                        let progress_host = host.clone();
                        let progress_request = request_id.clone();
                        create_remote_session(
                            host,
                            working_dir,
                            request_id,
                            updates,
                            internal,
                            move |host| {
                                transports.connect_with_progress(host, &mut |message| {
                                    let _ = progress_updates.send(Update::RemoteStatus {
                                        host: progress_host.clone(),
                                        message: message.to_owned(),
                                        request_id: progress_request.clone(),
                                        failed: false,
                                        failure: None,
                                    });
                                })
                            },
                        );
                    })
                    .expect("spawn remote create thread");
            }
            Command::ForkedInternal {
                session,
                client,
                prompt,
            } => {
                let session_id = session.session_id.clone();
                let _ = updates.send(Update::SessionForked {
                    session,
                    prompt: Some(prompt.clone()),
                });
                let worker = spawn_attached_session_worker(
                    session_id.clone(),
                    client,
                    &updates,
                    &transports,
                );
                let _ = worker.send(SessionCommand::Send {
                    content: prompt,
                    images: vec![],
                    attachments: vec![],
                });
                if let Some(old) = workers.insert(session_id, worker) {
                    let _ = old.send(SessionCommand::Stop);
                }
            }
            Command::SideQuestionInternal {
                parent_session_id,
                session,
                client,
                question,
            } => {
                let child_id = session.session_id.clone();
                let legacy_archive = session.archived;
                let _ = updates.send(Update::SideQuestionStarted {
                    parent_session_id,
                    session,
                    question: question.clone(),
                });
                let worker =
                    spawn_attached_session_worker(child_id.clone(), client, &updates, &transports);
                let _ = worker.send(SessionCommand::SendSideQuestion {
                    question,
                    legacy_archive,
                });
                if let Some(old) = workers.insert(child_id, worker) {
                    let _ = old.send(SessionCommand::Stop);
                }
            }
            Command::CreatedInternal {
                session,
                client,
                request_id,
            } => {
                let session_id = session.session_id.clone();
                eprintln!("jcode desktop: adopting created session {session_id}");
                // The adopted worker can report SessionConnected immediately.
                // Publish the panel first so readiness is not drained before the
                // UI has somewhere to apply it. This loop still installs the
                // worker before it can receive the UI's later Watch command.
                let _ = updates.send(Update::SessionCreated {
                    session,
                    request_id,
                });
                let worker = spawn_attached_session_worker(
                    session_id.clone(),
                    client,
                    &updates,
                    &transports,
                );
                if let Some(old) = workers.insert(session_id, worker) {
                    let _ = old.send(SessionCommand::Stop);
                }
            }
            Command::Watch { session_id } => {
                watched_sessions.insert(session_id.clone());
                ensure_session_worker(&mut workers, session_id, &updates, &transports);
            }
            Command::GetCompactedHistory {
                session_id,
                visible_messages,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::GetCompactedHistory(visible_messages),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::GetHistoryPage { session_id, before } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::GetHistoryPage(before),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::GetHistoryImage { session_id, key } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::GetHistoryImage(key),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::ListSessionImages { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::ListSessionImages,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::Unwatch { session_id } => {
                watched_sessions.remove(&session_id);
                if let Some(worker) = workers.remove(&session_id) {
                    let _ = worker.send(SessionCommand::Stop);
                }
            }
            Command::Send {
                session_id,
                content,
                images,
                attachments,
            } => {
                let command = SessionCommand::Send {
                    content,
                    images,
                    attachments,
                };
                send_to_session_worker(&mut workers, session_id, command, |session_id| {
                    spawn_session_worker(session_id, &updates, &transports)
                });
            }
            Command::SendSkill {
                session_id,
                content,
                images,
                attachments,
                skill,
            } => {
                let command = SessionCommand::SendSkill {
                    content,
                    images,
                    attachments,
                    skill,
                };
                send_to_session_worker(&mut workers, session_id, command, |session_id| {
                    spawn_session_worker(session_id, &updates, &transports)
                });
            }
            Command::QueuePrompt { session_id, prompt } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::QueuePrompt(prompt),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::RemoveQueuedPrompt {
                session_id,
                prompt,
                edit,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::RemoveQueuedPrompt(prompt, edit),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::SetQueuedPromptEditing {
                session_id,
                prompt_id,
                editing,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::SetQueuedPromptEditing(prompt_id, editing),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::EditQueuedPrompt {
                session_id,
                prompt_id,
                content,
                images,
                attachments,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::EditQueuedPrompt(prompt_id, content, images, attachments),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::ReorderQueuedPrompts { session_id, order } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::ReorderQueuedPrompts(order),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::SetQueuedPromptMode {
                session_id,
                client_id,
                mode,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::SetQueuedPromptMode(client_id, mode),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::InterruptQueuedPrompt {
                session_id,
                client_id,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::InterruptQueuedPrompt(client_id),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::InterruptQueuedPrompts { session_id, ids } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::InterruptQueuedPrompts(ids),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::CancelQueuedPrompt {
                session_id,
                client_id,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::CancelQueuedPrompt(client_id),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::Cancel { session_id } => {
                if let Some(worker) = workers.get(&session_id) {
                    let _ = worker.send(SessionCommand::Cancel);
                }
            }
            Command::ForceStopTurn { session_id } => {
                if let Some(worker) = workers.get(&session_id) {
                    let _ = worker.send(SessionCommand::ForceStopTurn);
                }
            }
            Command::BackgroundTool { session_id } => {
                if let Some(worker) = workers.get(&session_id) {
                    let _ = worker.send(SessionCommand::BackgroundTool);
                }
            }
            Command::WatchBackgroundTaskOutput {
                session_id,
                task_id,
            } => {
                if let Some(worker) = workers.get(&session_id) {
                    let _ = worker.send(SessionCommand::WatchBackgroundTaskOutput(task_id));
                }
            }
            Command::CancelBackgroundTask {
                session_id,
                task_id,
            } => {
                if let Some(worker) = workers.get(&session_id) {
                    let _ = worker.send(SessionCommand::CancelBackgroundTask(task_id));
                }
            }
            Command::AskResponse {
                session_id,
                request_id,
                attachment_id,
                attempt_id,
                response,
            } => {
                let sent = workers.get(&session_id).is_some_and(|worker| {
                    worker
                        .send(SessionCommand::AskResponse {
                            request_id: request_id.clone(),
                            attachment_id,
                            attempt_id,
                            response,
                        })
                        .is_ok()
                });
                if !sent {
                    let _ = updates.send(Update::AskResponseSettled {
                        session_id,
                        request_id,
                        attempt_id,
                        result: Err(AskResponseError::ConnectionChanged),
                    });
                }
            }
            Command::RoutingGateAnswer {
                session_id,
                gate_id,
                choice,
            } => {
                let sent = workers.get(&session_id).is_some_and(|worker| {
                    worker
                        .send(SessionCommand::RoutingGateAnswer {
                            gate_id: gate_id.clone(),
                            choice,
                        })
                        .is_ok()
                });
                if !sent {
                    let _ = updates.send(Update::RoutingGateAnswerSettled {
                        session_id,
                        gate_id,
                        result: Err("The session is not connected.".into()),
                    });
                }
            }
            Command::RoutingIntent { session_id, action } => {
                let sent = workers.get(&session_id).is_some_and(|worker| {
                    worker.send(SessionCommand::RoutingIntent(action)).is_ok()
                });
                if !sent {
                    let _ = updates.send(Update::RoutingIntentReply {
                        session_id,
                        result: Err("The session is not connected.".into()),
                    });
                }
            }
            Command::SetModel { session_id, model } => {
                let command = SessionCommand::SetModel(model);
                send_to_session_worker(&mut workers, session_id, command, |session_id| {
                    spawn_session_worker(session_id, &updates, &transports)
                });
            }
            Command::CancelPendingModel { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::CancelPendingModel,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::RefreshRuntime { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::RefreshRuntime,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::AuthChanged {
                session_id,
                provider,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::AuthChanged(provider),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::RuntimeCommand {
                session_id,
                command,
                args,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::RuntimeCommand(command, args),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::SessionOperation {
                session_id,
                operation,
            } => {
                let command = SessionCommand::Operation(operation);
                send_to_session_worker(&mut workers, session_id, command, |session_id| {
                    spawn_session_worker(session_id, &updates, &transports)
                });
            }
            Command::SetSessionSaved { session_id, saved } => {
                let operation = SessionCommand::Operation(SessionOperation::SetSaved(saved, None));
                let delivered = workers
                    .get(&session_id)
                    .is_some_and(|worker| worker.send(operation).is_ok());
                if !delivered {
                    workers.remove(&session_id);
                    let updates = updates.clone();
                    let transports = transports.clone();
                    std::thread::Builder::new()
                        .name("jcode-bridge-pin".into())
                        .spawn(move || set_saved_detached(session_id, saved, updates, transports))
                        .expect("spawn pin worker");
                }
            }
            Command::TodoState { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::TodoState,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::ReadTodoSnapshot { session_id, reply } => {
                let transports = transports.clone();
                let _ = std::thread::Builder::new()
                    .name("jcode-bridge-read-todo".into())
                    .spawn(move || {
                        let snapshot = persisted_todo_snapshot(&session_id).or_else(|| {
                            let address = remote::SessionAddress::parse(&session_id).ok()?;
                            let client = match &address.host {
                                Some(host) => transports.connect(host),
                                None => connect("todo-view"),
                            }
                            .ok()?;
                            client.todo_state(&address.session_id).ok()
                        });
                        let _ = reply.try_send(snapshot);
                    });
            }
            Command::ReadContextBreakdown { target, reply } => {
                let transports = transports.clone();
                let _ = std::thread::Builder::new()
                    .name("jcode-bridge-context".into())
                    .spawn(move || {
                        let breakdown = read_context_breakdown(target, &transports);
                        let _ = reply.try_send(breakdown);
                    });
            }
            Command::ShakeTools {
                session_id,
                action,
                reply,
            } => {
                let transports = transports.clone();
                let _ = std::thread::Builder::new()
                    .name("jcode-bridge-shake-tools".into())
                    .spawn(move || {
                        let _ = reply.try_send(shake_tools(&session_id, action, &transports));
                    });
            }
            Command::AckTodoFollowUp {
                session_id,
                effects,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::AckTodoFollowUp(effects),
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::ForkWithPrompt { session_id, prompt } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::ForkWithPrompt {
                        prompt,
                        internal: internal.clone(),
                    },
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::SideQuestion {
                session_id,
                question,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::SideQuestion {
                        question,
                        internal: internal.clone(),
                    },
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::ListSideQuestions {
                parent_session_id,
                request_id,
            } => {
                let (updates, transports) = (updates.clone(), transports.clone());
                std::thread::spawn(move || {
                    let result =
                        side_questions::read(&transports, &parent_session_id, |client, parent| {
                            side_questions::list(client, parent)
                        });
                    let _ = updates.send(Update::SideQuestionHistory {
                        parent_session_id,
                        request_id,
                        result,
                    });
                });
            }
            Command::OpenSideQuestion {
                parent_session_id,
                session_id,
                request_id,
            } => {
                let (updates, transports) = (updates.clone(), transports.clone());
                std::thread::spawn(move || {
                    let result =
                        side_questions::read(&transports, &parent_session_id, |client, parent| {
                            side_questions::open(client, parent, &session_id)
                        });
                    let _ = updates.send(Update::SideQuestionOpened {
                        parent_session_id,
                        request_id,
                        result,
                    });
                });
            }
            Command::ContinueSideQuestion {
                session_id,
                question,
            } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::ContinueSideQuestion { question },
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::DismissSideQuestion { session_id } => {
                if !watched_sessions.contains(&session_id)
                    && let Some(worker) = workers.remove(&session_id)
                {
                    let _ = worker.send(SessionCommand::Stop);
                }
            }
            Command::PromoteSideQuestion { session_id } => {
                if !watched_sessions.contains(&session_id)
                    && let Some(worker) = workers.get(&session_id)
                {
                    let _ = worker.send(SessionCommand::ReplaySideQuestionHistory);
                }
                let (updates, transports, internal) =
                    (updates.clone(), transports.clone(), internal.clone());
                std::thread::spawn(move || {
                    let result =
                        side_questions::read(&transports, &session_id, |client, session| {
                            client
                                .promote_side_question(&session.session_id)
                                .map_err(|error| error.to_string())
                        });
                    match result {
                        Ok(()) => {
                            let _ = internal.send(Command::RefreshSessions);
                        }
                        Err(reason) => {
                            let _ = updates.send(Update::CommandFailed {
                                session_id,
                                reason: format!("Could not open sidechat: {reason}"),
                            });
                        }
                    }
                });
            }
            Command::RestoreSideQuestion { session_id } => {
                if !watched_sessions.contains(&session_id)
                    && let Some(worker) = workers.get(&session_id)
                {
                    let _ = worker.send(SessionCommand::ReplaySideQuestionHistory);
                }
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::RestoreSession,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::Transfer { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::Transfer,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
            Command::Fork { session_id } => {
                send_to_session_worker(
                    &mut workers,
                    session_id,
                    SessionCommand::Fork,
                    |session_id| spawn_session_worker(session_id, &updates, &transports),
                );
            }
        }
    }
    stop_session_workers(workers);
}

pub(super) fn stop_session_workers(workers: HashMap<String, Sender<SessionCommand>>) {
    for worker in workers.into_values() {
        let _ = worker.send(SessionCommand::Stop);
    }
}
