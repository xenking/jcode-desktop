//! Real SDK socket-pair tests for the native SSH address routing boundary.
use super::*;
include!("harness_recovery_reconnect_tests.rs");
include!("harness_recovery_acceptance_tests.rs");
include!("harness_history_recovery_tests.rs");
include!("harness_protocol_recovery_tests.rs");
include!("harness_turn_retry_tests.rs");
use jcode_sdk::api::{ApiRequest, ClientFrame, ServerFrame, read_frame, write_frame};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Transport(UnixStream);
impl jcode_sdk::Transport for Transport {
    fn split(
        self: Box<Self>,
    ) -> jcode_sdk::Result<(Box<dyn BufRead + Send>, Box<dyn Write + Send>)> {
        let writer = self.0.try_clone().unwrap();
        Ok((Box::new(BufReader::new(self.0)), Box::new(writer)))
    }
}

fn info(id: &str) -> SessionInfo {
    SessionInfo {
        user_connected: false,
        session_id: id.into(),
        working_dir: Some("/remote/project".into()),
        title: Some("Remote test".into()),
        status: "idle".into(),
        transcript_bytes: None,
        saved: false,
        updated_at_ms: None,
        last_active_at_ms: None,
        archived: false,
        archived_at_ms: None,
        save_label: None,
        parent_session_id: None,
        agent_label: None,
        swarm_status: None,
        edit_stats: None,
        context: None,
    }
}

struct Server {
    requests: Receiver<ApiRequest>,
    writer: Arc<Mutex<UnixStream>>,
}
impl Server {
    fn new() -> (JcodeClient, Self) {
        Self::with_history_failures(0)
    }

    fn with_history_failures(history_failures: usize) -> (JcodeClient, Self) {
        Self::with_failures(history_failures, 0)
    }

    fn with_failures(
        history_failures: usize,
        attach_failures: usize,
    ) -> (JcodeClient, Self) {
        Self::with_pending_asks(history_failures, attach_failures, None)
    }

    fn with_pending_asks(
        mut history_failures: usize,
        mut attach_failures: usize,
        pending_ask_ids: Option<Vec<String>>,
    ) -> (JcodeClient, Self) {
        let (socket, server) = UnixStream::pair().unwrap();
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let writer = Arc::new(Mutex::new(server));
        let reply_writer = writer.clone();
        let (tx, requests) = channel();
        std::thread::spawn(move || {
            while let Ok(frame) = read_frame::<_, ClientFrame>(&mut reader) {
                let event = match &frame.request {
                    ApiRequest::Hello { .. } => ApiEvent::HelloOk {
                        runtime_identity: None,
                        version: jcode_sdk::api::API_VERSION_MAJOR,
                        server: "remote-test".into(),
                        capabilities: vec!["runtime_commands".into()],
                    },
                    ApiRequest::Ping => ApiEvent::Pong,
                    ApiRequest::ListSessions { .. } => ApiEvent::Sessions {
                        sessions: vec![info("same-id")],
                        workers: Default::default(),
                    },
                    ApiRequest::AttachSession { .. } if attach_failures > 0 => {
                        attach_failures -= 1;
                        ApiEvent::Error {
                            code: jcode_sdk::api::ErrorCode::Internal,
                            message: "Failed to restore session: Session same-id still has a runtime owner".into(),
                        }
                    }
                    ApiRequest::CreateSession { .. } | ApiRequest::AttachSession { .. } => {
                        ApiEvent::Attached {
                            pending_ask_ids: pending_ask_ids.clone(),
                            session: info("same-id"),
                        }
                    }
                    ApiRequest::GetHistory { .. } if history_failures > 0 => {
                        history_failures -= 1;
                        ApiEvent::Error {
                            code: jcode_sdk::api::ErrorCode::Internal,
                            message: "History is temporarily unavailable".into(),
                        }
                    }
                    ApiRequest::GetHistory { .. } => ApiEvent::History {
                        session_id: "same-id".into(),
                        messages: vec![],
                        images: vec![],
                    },
                    ApiRequest::GetRuntimeInfo { .. } => ApiEvent::RuntimeInfo {
                        speed_mode: Default::default(),
                        speed_options: Vec::new(),
                        session_id: "same-id".into(),
                        provider: None,
                        model: None,
                        routes: vec![],
                        reasoning_effort: None,
                        auth_method: None,
                        account_label: None,
                    },
                    ApiRequest::SetModel { model, .. } if model == "retired-model" => {
                        ApiEvent::Error {
                            code: jcode_sdk::api::ErrorCode::Internal,
                            message: "The requested model was retired".into(),
                        }
                    }
                    ApiRequest::SetSpeedMode { speed_mode } => ApiEvent::SpeedModeChanged {
                        session_id: "same-id".into(),
                        speed_mode: *speed_mode,
                        speed_options: Vec::new(),
                    },
                    ApiRequest::RuntimeCommand {
                        session_id,
                        command,
                        args,
                    } => ApiEvent::RuntimeCommandResult {
                        session_id: session_id.clone(),
                        command: *command,
                        data: serde_json::json!({ "args": args, "runtime": "remote-test" }),
                        message: "Remote runtime result".into(),
                    },
                    ApiRequest::TransferSession { .. } => ApiEvent::SessionForked {
                        session: info("transfer-id"),
                    },
                    ApiRequest::ForkSession { .. } => ApiEvent::SessionForked {
                        session: info("fork-id"),
                    },
                    ApiRequest::Compact { .. } => ApiEvent::Compacted {
                        session_id: "same-id".into(),
                        message: "scheduled".into(),
                    },
                    _ => ApiEvent::Ok,
                };
                if !matches!(frame.request, ApiRequest::Hello { .. } | ApiRequest::Ping) {
                    let _ = tx.send(frame.request.clone());
                }
                if write_frame(
                    &mut *reply_writer.lock().unwrap(),
                    &ServerFrame::reply(frame.id, event),
                )
                .is_err()
                {
                    break;
                }
                if matches!(frame.request, ApiRequest::SendMessage { .. }) {
                    let _ = write_frame(
                        &mut *reply_writer.lock().unwrap(),
                        &ServerFrame::event(ApiEvent::MessageAccepted {
                            session_id: "same-id".into(),
                        }),
                    );
                }
            }
        });
        let client = JcodeClient::connect_with(
            Box::new(Transport(socket)),
            ConnectOptions {
                ensure_runtime: false,
                request_timeout: Some(Duration::from_secs(2)),
                ..Default::default()
            },
        )
        .unwrap();
        (client, Self { requests, writer })
    }
    fn next(&self) -> ApiRequest {
        self.requests
            .recv_timeout(Duration::from_secs(3))
            .expect("SDK request")
    }
    fn event(&self, event: ApiEvent) {
        write_frame(
            &mut *self.writer.lock().unwrap(),
            &ServerFrame::event(event),
        )
        .unwrap();
    }
    fn disconnect(&self) {
        // A worker that stopped on a fatal protocol error closes its side first,
        // so the server's own shutdown then reports NotConnected. That is the
        // state these tests assert, not a fixture failure; anything else is.
        if let Err(error) = self
            .writer
            .lock()
            .unwrap()
            .shutdown(std::net::Shutdown::Both)
            && error.kind() != std::io::ErrorKind::NotConnected
        {
            panic!("server disconnect failed: {error}");
        }
    }
}

fn wait_update(updates: &UpdateReceiver, predicate: impl Fn(&Update) -> bool) -> Update {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        assert!(Instant::now() < deadline, "timed out waiting for UI update");
        if let Ok(update) = updates.try_recv() {
            assert!(
                !matches!(
                    update,
                    Update::CommandFailed { .. } | Update::SendFailed { .. }
                ),
                "{update:?}"
            );
            if predicate(&update) {
                return update;
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn live_runtime_owner_is_attaching_not_lost() {
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let mut attempts = 0;
    let worker = std::thread::spawn(move || {
        session_worker_with_connector("same-id".into(), rx, tx, None, move |_| {
            attempts += 1;
            let (client, _server) = Server::with_failures(0, usize::from(attempts == 1));
            Ok(client)
        });
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut lost = false;
    let mut connected = false;
    while Instant::now() < deadline && !connected {
        if let Ok(update) = updates.try_recv() {
            lost |= matches!(update, Update::SessionLost { .. });
            connected |= matches!(update, Update::SessionConnected { .. });
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    assert!(connected, "the worker should reconnect");
    assert!(!lost, "a live runtime owner is not a lost session");
}

#[test]
fn restoring_session_reads_last_model_and_effort_before_attach() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("sessions")).unwrap();
    std::fs::write(
        home.path().join("sessions/same-id.json"),
        r#"{"model":"old-model","reasoning_effort":"low","messages":[]}"#,
    )
    .unwrap();
    std::fs::write(
        home.path().join("sessions/same-id.journal.jsonl"),
        "{\"meta\":{\"model\":\"gpt-5.6\",\"reasoning_effort\":\"high\"}}\n",
    )
    .unwrap();
    let update = saved_session_identity(home.path(), "same-id", "same-id").unwrap();
    assert!(
        matches!(update, Update::Event { session_id, event: ApiEvent::ModelInfo { model: Some(model), reasoning_effort: Some(effort), .. } }
        if session_id == "same-id" && model == "gpt-5.6" && effort == "high")
    );
    assert!(saved_session_identity(home.path(), "../escape", "ignored").is_none());
}

#[test]
fn remote_worker_routes_every_native_operation_and_namespaces_outputs() {
    let ui_id = remote::namespace("user@desktop", "same-id");
    let (client, server) = Server::new();
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let worker_id = ui_id.clone();
    let worker = std::thread::spawn(move || session_worker(worker_id, rx, tx, Some(client)));
    wait_update(
        &updates,
        |u| matches!(u, Update::History { session_id, .. } if session_id == &ui_id),
    );
    assert!(
        matches!(server.next(), ApiRequest::GetHistory { session_id } if session_id == "same-id")
    );
    assert!(
        matches!(server.next(), ApiRequest::GetRuntimeInfo { session_id } if session_id == "same-id")
    );

    commands
        .send(SessionCommand::Send {
            content: "remote prompt".into(),
            images: vec![("image/png".into(), "bytes".into())],
            attachments: vec![],
        })
        .unwrap();
    assert!(
        matches!(server.next(), ApiRequest::SendMessage { session_id, content, images, .. }
        if session_id == "same-id" && content == "remote prompt" && images.len() == 1)
    );
    wait_update(&updates, |u| {
        matches!(u, Update::Event { session_id, event: ApiEvent::MessageAccepted { session_id: inner } }
        if session_id == &ui_id && inner == &ui_id)
    });
    commands
        .send(SessionCommand::Send {
            content: "steering".into(),
            images: vec![],
            attachments: vec![],
        })
        .unwrap();
    assert!(
        matches!(server.next(), ApiRequest::SoftInterrupt { session_id, urgent: true, .. } if session_id == "same-id")
    );

    commands
        .send(SessionCommand::RuntimeCommand(
            jcode_sdk::api::RuntimeCommand::Memory,
            vec!["search".into(), "remote/path".into()],
        ))
        .unwrap();
    assert!(
        matches!(server.next(), ApiRequest::RuntimeCommand { session_id, command: jcode_sdk::api::RuntimeCommand::Memory, args }
        if session_id == "same-id" && args == vec!["search", "remote/path"])
    );
    wait_update(&updates, |u| {
        matches!(u, Update::Event { session_id, event: ApiEvent::RuntimeCommandResult { session_id: inner, data, message, .. } }
        if session_id == &ui_id && inner == &ui_id && data["runtime"] == "remote-test" && message == "Remote runtime result")
    });

    commands.send(SessionCommand::Cancel).unwrap();
    assert!(matches!(server.next(), ApiRequest::Cancel { session_id } if session_id == "same-id"));
    commands
        .send(SessionCommand::SetModel("model".into()))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::Clear))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::Compact))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::SetEffort(
            "high".into(),
        )))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::Rename(Some(
            "name".into(),
        ))))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::Rewind(2)))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::RewindTranscript(
            jcode_sdk::api::RewindAnchor {
                selected: jcode_sdk::api::RewindMessageKey::new("user", "remote prompt").unwrap(),
                before: None,
                after: None,
            },
        )))
        .unwrap();
    commands
        .send(SessionCommand::Operation(SessionOperation::RewindUndo))
        .unwrap();
    commands.send(SessionCommand::Fork).unwrap();
    commands.send(SessionCommand::Transfer).unwrap();
    for expected in [
        "set_model",
        "clear",
        "compact",
        "set_reasoning_effort",
        "rename_session",
        "rewind",
        "get_history",
        "rewind_transcript",
        "get_history",
        "rewind_undo",
        "get_history",
        "fork_session",
        "transfer_session",
    ] {
        let request = serde_json::to_value(server.next()).unwrap();
        assert_eq!(request["session_id"], "same-id", "{expected}: {request}");
        assert_eq!(request["req"], expected, "{request}");
    }
    wait_update(&updates, |u| {
        matches!(u, Update::Event { session_id, event: ApiEvent::Compacted { message, .. } }
            if session_id == &ui_id && message == "scheduled")
    });
    wait_update(&updates, |u| matches!(u, Update::RewoundHistory { session_id, bounded: false, .. } if session_id == &ui_id));
    wait_update(
        &updates,
        |u| matches!(u, Update::SessionForked { session, .. } if session.session_id == remote::namespace("user@desktop", "fork-id")),
    );
    wait_update(&updates, |u| {
        matches!(u, Update::SessionForked { session, .. }
        if session.session_id == remote::namespace("user@desktop", "transfer-id"))
    });
    // A different remote session must not leak into this panel, even for images.
    server.event(ApiEvent::SidePaneImages {
        session_id: "other-id".into(),
        images: vec![],
    });
    server.event(ApiEvent::TextDelta {
        message_id: Some("response-id".into()),
        session_id: "same-id".into(),
        text: "answer".into(),
    });
    wait_update(&updates, |u| {
        assert!(!matches!(
            u,
            Update::Event {
                event: ApiEvent::SidePaneImages { .. },
                ..
            }
        ));
        matches!(u, Update::Event { session_id, event: ApiEvent::TextDelta { session_id: inner, .. } } if session_id == &ui_id && inner == &ui_id)
    });
    server.event(ApiEvent::TextReplace {
        session_id: "same-id".into(),
        text: "corrected".into(),
        message_id: Some("response-id".into()),
    });
    wait_update(&updates, |u| matches!(u, Update::Event {
        session_id, event: ApiEvent::TextReplace { session_id: inner, text, message_id: Some(id) }
    } if session_id == &ui_id && inner == &ui_id && id == "response-id" && text == "corrected"));
    server.event(ApiEvent::TextDone {
        session_id: "same-id".into(),
        message_id: Some("response-id".into()),
    });
    wait_update(&updates, |u| matches!(u, Update::Event {
        session_id, event: ApiEvent::TextDone { session_id: inner, message_id: Some(id) }
    } if session_id == &ui_id && inner == &ui_id && id == "response-id"));
    server.event(ApiEvent::TurnDone {
        session_id: "same-id".into(),
        pending_soft_interrupts: None,
    });
    wait_update(&updates, |u| {
        matches!(
            u,
            Update::Event {
                event: ApiEvent::TurnDone { .. },
                ..
            }
        )
    });
    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    assert!(
        matches!(server.next(), ApiRequest::DetachSession { session_id } if session_id == "same-id")
    );
    server.disconnect();
}

#[test]
fn changing_speed_on_another_model_requires_a_successful_model_switch() {
    use jcode_sdk::api::SpeedMode;

    let ui_id = remote::namespace("user@desktop", "same-id");
    let (client, server) = Server::new();
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let worker_id = ui_id.clone();
    let worker = std::thread::spawn(move || session_worker(worker_id, rx, tx, Some(client)));
    wait_update(&updates, |u| matches!(u, Update::History { .. }));
    assert!(matches!(server.next(), ApiRequest::GetHistory { .. }));
    assert!(matches!(server.next(), ApiRequest::GetRuntimeInfo { .. }));

    for (serial, model) in [(1, "retired-model"), (2, "target-model")] {
        commands
            .send(SessionCommand::Operation(
                SessionOperation::SetModelWithSpeed {
                    model: model.into(),
                    effort: Some("high".into()),
                    serial,
                    mode: SpeedMode::Ultrafast,
                },
            ))
            .unwrap();
        assert!(
            matches!(server.next(), ApiRequest::SetModel { session_id, model: sent }
            if session_id == "same-id" && sent == model)
        );
        if serial == 2 {
            assert!(
                matches!(server.next(), ApiRequest::SetReasoningEffort { session_id, effort }
                if session_id == "same-id" && effort == "high")
            );
            assert!(matches!(
                server.next(),
                ApiRequest::SetSpeedMode {
                    speed_mode: SpeedMode::Ultrafast
                }
            ));
        }
        wait_update(&updates, |update| match update {
            Update::SpeedSettled {
                session_id,
                serial: settled,
                result,
            } if *settled == serial => {
                assert_eq!(session_id, &ui_id);
                if serial == 1 {
                    assert!(result.as_ref().unwrap_err().contains("retired"));
                } else {
                    assert_eq!(result.as_ref().unwrap(), &SpeedMode::Ultrafast);
                }
                true
            }
            _ => false,
        });
    }

    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    assert!(matches!(server.next(), ApiRequest::DetachSession { .. }));
    server.disconnect();
}

#[test]
fn ask_reconnect_reports_new_pending_owner_and_never_replays_an_old_answer() {
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let (connections_tx, connections) = channel();
    let worker = std::thread::spawn(move || {
        let mut attachment = 0;
        session_worker_with_connector("same-id".into(), rx, tx, None, |_| {
            let request = if attachment == 0 { "ask-old" } else { "ask-new" };
            attachment += 1;
            let (client, server) = Server::with_pending_asks(0, 0, Some(vec![request.into()]));
            connections_tx.send(server).unwrap();
            Ok(client)
        });
    });
    let await_attachment = |request_id: &str| {
        let Update::AskAttachment { attachment_id, pending_ask_ids, .. } =
            wait_update(&updates, |u| matches!(u, Update::AskAttachment { .. }))
        else { unreachable!() };
        assert_eq!(pending_ask_ids.unwrap(), [request_id]);
        attachment_id
    };
    let first = connections.recv_timeout(Duration::from_secs(3)).unwrap();
    let old_attachment = await_attachment("ask-old");
    first.disconnect();
    wait_update(&updates, |u| matches!(u, Update::SessionLost { .. }));

    let answer = |request_id: &str, attachment_id, attempt_id| {
        commands.send(SessionCommand::AskResponse {
            request_id: request_id.into(),
            attachment_id,
            attempt_id,
            response: jcode_sdk::AskResponse::default(),
        }).unwrap();
    };
    answer("ask-old", old_attachment, 1);
    let second = connections.recv_timeout(Duration::from_secs(3)).unwrap();
    let new_attachment = await_attachment("ask-new");
    assert_ne!(old_attachment, new_attachment);
    wait_update(&updates, |u| matches!(u, Update::AskResponseSettled {
        attempt_id: 1, result: Err(AskResponseError::ConnectionChanged), ..
    }));
    assert!(!second.requests.try_iter().any(|r| matches!(r, ApiRequest::AskResponse { .. })));

    answer("ask-new", new_attachment, 2);
    assert!(matches!(second.next(), ApiRequest::AskResponse { request_id, response, .. }
        if request_id == "ask-new" && response.answers.is_none()));
    wait_update(&updates, |u| matches!(u, Update::AskResponseSettled {
        attempt_id: 2, result: Ok(()), ..
    }));
    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    second.disconnect();
}

#[test]
fn restored_remote_address_reconnects_to_same_host_without_blocking_local_worker() {
    let ui_id = remote::namespace("desktop", "same-id");
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let (connections_tx, connections) = channel();
    let worker_id = ui_id.clone();
    let worker = std::thread::spawn(move || {
        session_worker_with_connector(worker_id, rx, tx, None, |address| {
            assert_eq!(address.host.as_deref(), Some("desktop"));
            assert_eq!(address.session_id, "same-id");
            let (client, server) = Server::new();
            connections_tx.send(server).unwrap();
            Ok(client)
        })
    });
    let server = connections.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        matches!(server.next(), ApiRequest::AttachSession { session_id } if session_id == "same-id")
    );
    wait_update(
        &updates,
        |u| matches!(u, Update::History { session_id, .. } if session_id == &ui_id),
    );
    server.disconnect();
    wait_update(
        &updates,
        |u| matches!(u, Update::SessionLost { session_id, .. } if session_id == &ui_id),
    );
    wait_update(&updates, |u| matches!(u,
        Update::OpenrigHostIdentity { session_id, self_host_id: None } if session_id == &ui_id
    ));

    // The same raw ID on the local host remains independent and responsive.
    let (local_client, local_server) = Server::new();
    let (local_commands, local_rx) = channel();
    let (local_tx, local_updates) = update_channel();
    let local = std::thread::spawn(move || {
        session_worker("same-id".into(), local_rx, local_tx, Some(local_client))
    });
    wait_update(
        &local_updates,
        |u| matches!(u, Update::History { session_id, .. } if session_id == "same-id"),
    );
    let replacement = connections.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        matches!(replacement.next(), ApiRequest::AttachSession { session_id } if session_id == "same-id")
    );
    wait_update(
        &updates,
        |u| matches!(u, Update::SessionConnected { session_id } if session_id == &ui_id),
    );
    commands.send(SessionCommand::Stop).unwrap();
    local_commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    local.join().unwrap();
    replacement.disconnect();
    local_server.disconnect();
}

#[test]
fn remote_metadata_never_reads_matching_local_files() {
    let home = tempfile::tempdir().unwrap();
    let id = remote::namespace("desktop", "same-id");
    // Deliberately populate the exact path a naive home.join(id) would read.
    let path = home.path().join("sessions").join(format!("{id}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, r#"{"status":"crashed"}"#).unwrap();
    let todos = home.path().join("todos").join(format!("{id}.json"));
    std::fs::create_dir_all(todos.parent().unwrap()).unwrap();
    std::fs::write(
        todos,
        r#"[{"content":"LOCAL SECRET","status":"in_progress"}]"#,
    )
    .unwrap();
    assert_eq!(persisted_todo_title(home.path(), &id), None);
    let merged = merge_persisted_sessions(vec![info(&id)], Some(home.path()));
    let remote = merged
        .iter()
        .find(|session| session.session_id == id)
        .unwrap();
    assert_eq!(remote.status, "idle");
    assert_ne!(remote.title.as_deref(), Some("LOCAL SECRET"));
}

#[test]
fn remote_create_preserves_request_id_and_sends_working_dir_only_as_json() {
    let (client, server) = Server::new();
    let (internal, created) = channel();
    let (tx, updates) = update_channel();
    let cwd = "/remote/space dir/$(touch never);'quoted'";
    create_remote_session(
        " user@desktop ".into(),
        Some(cwd.into()),
        Some("draft-id".into()),
        tx,
        internal,
        |host| {
            assert_eq!(host, "user@desktop");
            Ok((client, false))
        },
    );
    assert!(
        matches!(server.next(), ApiRequest::CreateSession { working_dir, .. }
        if working_dir.as_deref() == Some(cwd))
    );
    match created.recv_timeout(Duration::from_secs(2)).unwrap() {
        Command::CreatedInternal {
            session,
            request_id,
            ..
        } => {
            assert_eq!(
                session.session_id,
                remote::namespace("user@desktop", "same-id")
            );
            assert_eq!(request_id.as_deref(), Some("draft-id"));
        }
        _ => panic!("wrong create handoff"),
    }
    wait_update(&updates, |update| {
        matches!(update, Update::RemoteStatus { message, request_id, failed, .. }
        if message == "SSH connected. Creating Jcode session..."
            && request_id.as_deref() == Some("draft-id") && !failed)
    });
    wait_update(&updates, |update| {
        matches!(update, Update::RemoteStatus { host, message, .. }
        if host == "user@desktop" && message.starts_with("Connected"))
    });
    server.disconnect();
}

#[test]
fn remote_create_failure_is_visible_and_never_retries_a_startup_draft() {
    let calls = Cell::new(0);
    let (internal, created) = channel();
    let (tx, updates) = update_channel();
    create_remote_session(
        "desktop".into(),
        None,
        Some("draft-id".into()),
        tx,
        internal,
        |_| {
            calls.set(calls.get() + 1);
            Err(jcode_sdk::Error::new(
                jcode_sdk::ErrorKind::Timeout,
                "SSH handshake timed out",
            ))
        },
    );
    assert_eq!(calls.get(), 1);
    assert!(created.try_recv().is_err());
    wait_update(&updates, |update| {
        matches!(update, Update::RemoteStatus { host, message, .. }
        if host == "desktop" && message.contains("SSH handshake timed out"))
    });
}

#[test]
fn paired_create_failure_carries_the_typed_cause_and_ssh_failure_does_not() {
    for (host, expect_typed) in [
        ("paired:0123456789abcdef0123456789abcdef", true),
        ("desktop", false),
    ] {
        let (internal, _created) = channel();
        let (tx, updates) = update_channel();
        create_remote_session(host.into(), None, Some("draft-id".into()), tx, internal, |_| {
            Err(jcode_sdk::Error::new(
                jcode_sdk::ErrorKind::Paired(jcode_sdk::PairedFailure::Unresolved),
                "The gateway's name did not resolve",
            ))
        });
        let update = wait_update(&updates, |update| {
            matches!(update, Update::RemoteStatus { failed: true, .. })
        });
        let Update::RemoteStatus { failure, .. } = update else { unreachable!() };
        assert_eq!(failure.is_some(), expect_typed, "{host}");
        if let Some(failure) = failure {
            assert_eq!(
                failure.kind,
                crate::connect_failure::FailureKind::Paired(jcode_sdk::PairedFailure::Unresolved)
            );
        }
    }
}

#[test]
fn invalid_remote_create_host_never_starts_a_transport() {
    let (internal, created) = channel();
    let (tx, updates) = update_channel();
    create_remote_session(
        "-oProxyCommand=evil".into(),
        None,
        None,
        tx,
        internal,
        |_| panic!("invalid host reached transport"),
    );
    assert!(created.try_recv().is_err());
    wait_update(&updates, |update| {
        matches!(update, Update::RemoteStatus { message, .. }
        if message.starts_with("Remote session failed:"))
    });
}

#[test]
fn named_cloud_catalog_lists_and_reattaches_the_original_remote_id() {
    let (client, server) = Server::new();
    let sessions = list_remote_sessions("cloud:ai-core", |host| {
        assert_eq!(host, "cloud:ai-core");
        Ok(client.clone())
    })
    .unwrap();
    assert!(matches!(server.next(), ApiRequest::ListSessions { .. }));
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "cloud://ai-core/same-id");
    let address = remote::SessionAddress::parse(&sessions[0].session_id).unwrap();
    assert_eq!(address.host.as_deref(), Some("cloud:ai-core"));
    assert_eq!(
        client
            .attach_session(&address.session_id)
            .unwrap()
            .session_id,
        "same-id"
    );
    assert!(
        matches!(server.next(), ApiRequest::AttachSession { session_id } if session_id == "same-id")
    );
}

#[test]
fn self_hosted_creation_uses_remote_home_without_the_cloud_bootstrap_path() {
    for (host, self_hosted) in [
        ("cloud:ai-core", true),
        ("jcode-cloud", true),
        ("jcode-cloud", false),
    ] {
        let (client, server) = Server::new();
        let (internal, created) = channel();
        let (tx, _updates) = update_channel();
        create_remote_session(host.into(), None, None, tx, internal, |_| {
            Ok((client, self_hosted))
        });
        assert!(
            matches!(server.next(), ApiRequest::CreateSession { working_dir, .. }
            if working_dir.as_deref() == (!self_hosted).then_some(crate::managed_cloud::DEFAULT_WORKSPACE))
        );
        assert!(
            matches!(created.recv_timeout(Duration::from_secs(2)), Ok(Command::CreatedInternal { session, .. })
            if session.session_id == remote::namespace(host, "same-id"))
        );
    }
}

#[test]
fn last_bridge_handle_stops_its_coordinator_for_ssh_cleanup() {
    let (bridge, commands) = spawn_recording();
    let second = bridge.clone();
    drop(bridge);
    assert!(commands.try_recv().is_err());
    drop(second);
    assert!(matches!(
        commands.recv_timeout(Duration::from_secs(1)),
        Ok(Command::Shutdown)
    ));
}

/// Live end-to-end check of managed Jcode Cloud through Desktop's own create
/// path, without a model request. Reports cold and warm panel startup separately.
#[cfg(unix)]
#[test]
#[ignore = "live: creates two sessions on the account's Jcode Cloud machine"]
fn managed_cloud_live_warm_panels_use_distinct_sessions() {
    let transports = transport::RemoteTransports::default();
    let mut clients = Vec::new();
    let mut ids = Vec::new();
    for index in 0..2 {
        if index == 1 {
            let deadline = Instant::now() + Duration::from_secs(15);
            while !transports.cloud_spare_ready() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            assert!(
                transports.cloud_spare_ready(),
                "cloud API stream did not preconnect"
            );
        }
        let (internal, created) = channel();
        let (tx, _updates) = update_channel();
        let started = Instant::now();
        let mut phases = Vec::new();
        create_remote_session(
            crate::managed_cloud::HOST.into(),
            None,
            Some(format!("live-warm-{index}")),
            tx,
            internal,
            |host| {
                let connecting = Instant::now();
                let result = transports.connect_with_progress(host, &mut |message| {
                    eprintln!("[cloud {index}] {message}");
                    phases.push(message.to_owned());
                });
                eprintln!(
                    "[cloud {index}] transport ready after {:?}",
                    connecting.elapsed()
                );
                result
            },
        );
        let (session, client) = match created.recv_timeout(Duration::from_secs(5)) {
            Ok(Command::CreatedInternal {
                session, client, ..
            }) => (session, client),
            _ => panic!("cloud panel {index} failed before session creation"),
        };
        eprintln!("[cloud {index}] ready after {:?}", started.elapsed());
        assert!(session.session_id.starts_with("ssh://jcode-cloud/"));
        if index == 1 {
            assert!(
                phases.iter().any(|phase| phase.contains("Reusing ready")),
                "warm path not used: {phases:?}"
            );
        }
        ids.push(session.session_id);
        clients.push(client);
    }
    assert_ne!(
        ids[0], ids[1],
        "panels must never share an attached API client"
    );
}

#[test]
#[ignore = "live: lists a registered Cloud machine's sessions via pinned SSH"]
fn managed_cloud_live_named_machine_lists_sessions() {
    let name = std::env::var("JCODE_CLOUD_TEST_HOST").unwrap_or_else(|_| "ai-core".into());
    let destination = crate::managed_cloud::Destination::named(&name).expect("valid machine name");
    let account = crate::managed_cloud::account().expect("configured Jcode account");
    let machines = crate::managed_cloud::list_machines(&account).expect("Cloud registry");
    assert!(
        machines.iter().any(|machine| machine.name == name),
        "machine not in account registry: {name}"
    );
    let transports = transport::RemoteTransports::default();
    let host = destination.preference();
    let sessions = list_remote_sessions(&host, |host| {
        let (client, self_hosted) = transports
            .connect_with_progress(host, &mut |message| eprintln!("[{name}] {message}"))?;
        assert!(self_hosted, "named machine must be self-hosted");
        Ok(client)
    })
    .expect("remote session catalog");
    assert!(
        sessions
            .iter()
            .all(|session| remote_host(&session.session_id).as_deref() == Some(&host))
    );
    eprintln!(
        "[{name}] Registry confirmed, pinned SSH handshake succeeded, listed {} sessions",
        sessions.len()
    );
}

/// Live end-to-end check of managed Jcode Cloud through Desktop's own create
/// path. Requires a signed-in, subscribed Jcode account and provisions or wakes
/// that account's real cloud machine. Uses no AWS credentials or SSH config.
/// Run: cargo test -p jcode-desktop-ui managed_cloud_live -- --ignored --nocapture
#[test]
#[ignore = "live: provisions the account's Jcode Cloud machine"]
fn managed_cloud_live_session_answers_a_prompt() {
    let (internal, created) = channel();
    let (tx, updates) = update_channel();
    let transports = transport::RemoteTransports::default();
    let printer = std::thread::spawn(move || {
        while let Ok(update) = updates.recv_blocking() {
            if let Update::RemoteStatus {
                message, failed, ..
            } = update
            {
                eprintln!("[cloud] {message}{}", if failed { " (failed)" } else { "" });
            }
        }
    });
    let started = Instant::now();
    create_remote_session(
        crate::managed_cloud::HOST.into(),
        None,
        Some("live".into()),
        tx,
        internal,
        |host| transports.connect_with_progress(host, &mut |m| eprintln!("[cloud] {m}")),
    );
    let (session, client) = match created.recv_timeout(Duration::from_secs(5)) {
        Ok(Command::CreatedInternal {
            session, client, ..
        }) => (session, client),
        _ => panic!("managed cloud session was not created"),
    };
    eprintln!(
        "[cloud] session {} after {:?}",
        session.session_id,
        started.elapsed()
    );
    assert!(session.session_id.starts_with("ssh://jcode-cloud/"));
    let address = remote::SessionAddress::parse(&session.session_id).unwrap();
    let events = client.events(Some(&address.session_id));
    client
        .send_message(
            &address.session_id,
            "Reply with exactly: DESKTOP_CLOUD_OK",
            vec![],
            None,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut text = String::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.next_timeout(left) {
            Some(jcode_sdk::api::ApiEvent::TextDelta { text: delta, .. }) => text.push_str(&delta),
            Some(jcode_sdk::api::ApiEvent::TurnDone { .. }) => break,
            Some(jcode_sdk::api::ApiEvent::TurnStopped { message, .. }) => {
                panic!("turn stopped: {message}")
            }
            Some(_) => {}
            None => break,
        }
    }
    eprintln!("[cloud] reply: {text:?} total {:?}", started.elapsed());
    assert!(
        text.contains("DESKTOP_CLOUD_OK"),
        "unexpected reply {text:?}"
    );
    drop(client);
    drop(printer);
}
