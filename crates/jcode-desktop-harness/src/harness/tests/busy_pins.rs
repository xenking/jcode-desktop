/// A worker attached to `Server`, idle, with its attach traffic consumed.
fn attached_worker(
    ui_id: &str,
) -> (
    Server,
    std::sync::mpsc::Sender<SessionCommand>,
    UpdateReceiver,
    std::thread::JoinHandle<()>,
) {
    let (client, server) = Server::new();
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let id = ui_id.to_owned();
    let worker = std::thread::spawn(move || session_worker(id, rx, tx, Some(client)));
    assert!(matches!(server.next(), ApiRequest::GetHistory { .. }));
    assert!(matches!(server.next(), ApiRequest::GetRuntimeInfo { .. }));
    (server, commands, updates, worker)
}

fn pin(name: &str) -> SessionCommand {
    SessionCommand::RuntimeCommand(
        jcode_sdk::api::RuntimeCommand::Mode,
        vec!["toggle".into(), name.into()],
    )
}

fn pinned(request: ApiRequest) -> String {
    match request {
        ApiRequest::RuntimeCommand {
            command: jcode_sdk::api::RuntimeCommand::Mode,
            args,
            ..
        } => args[1].clone(),
        other => panic!("expected a mode pin, got {other:?}"),
    }
}

/// Picking a mode while a turn runs (a local session, and a paired machine's,
/// which differ only by the id) neither reaches the busy daemon nor fails: it
/// applies when the turn ends.
#[test]
fn a_mode_pinned_during_a_turn_applies_after_it_without_an_error() {
    for ui_id in [
        "same-id".to_owned(),
        remote::namespace("user@desktop", "same-id"),
    ] {
        let (server, commands, updates, worker) = attached_worker(&ui_id);
        commands
            .send(SessionCommand::Send {
                content: "long turn".into(),
                images: vec![],
                attachments: vec![],
            })
            .unwrap();
        assert!(matches!(server.next(), ApiRequest::SendMessage { .. }));
        commands.send(pin("quiet")).unwrap();
        commands.send(pin("plan")).unwrap();
        assert!(
            server
                .requests
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "the daemon refuses a pin mid-turn; the worker must hold it"
        );
        server.event(ApiEvent::TurnDone {
            session_id: "same-id".into(),
            pending_soft_interrupts: None,
        });
        assert_eq!(pinned(server.next()), "quiet");
        assert_eq!(pinned(server.next()), "plan");
        wait_update(&updates, |u| {
            matches!(
                u,
                Update::Event {
                    event: ApiEvent::RuntimeCommandResult {
                        command: jcode_sdk::api::RuntimeCommand::Mode,
                        ..
                    },
                    ..
                }
            )
        });
        drop(commands);
        worker.join().unwrap();
    }
}

/// The worker cannot always know a turn is running (another client started
/// it): the daemon's refusal is retried in order, never shown.
#[test]
fn a_busy_refusal_of_a_mode_pin_is_retried_in_order_and_never_shown() {
    let (server, commands, updates, worker) = attached_worker("same-id");
    server
        .busy_pins
        .store(1, std::sync::atomic::Ordering::SeqCst);
    commands.send(pin("quiet")).unwrap();
    commands.send(pin("plan")).unwrap();
    let seen: Vec<_> = (0..3).map(|_| pinned(server.next())).collect();
    assert_eq!(seen, ["quiet", "quiet", "plan"]);
    wait_update(&updates, |u| {
        matches!(
            u,
            Update::Event {
                event: ApiEvent::RuntimeCommandResult {
                    command: jcode_sdk::api::RuntimeCommand::Mode,
                    ..
                },
                ..
            }
        )
    });
    drop(commands);
    worker.join().unwrap();
}
