/// A machine that fails every attach as `unknown_session`, listing `listed`.
fn unknown_session_machine(listed: &'static [&'static str]) -> JcodeClient {
    let (socket, server) = UnixStream::pair().unwrap();
    let mut reader = BufReader::new(server.try_clone().unwrap());
    let writer = Arc::new(Mutex::new(server));
    std::thread::spawn(move || {
        while let Ok(frame) = read_frame::<_, ClientFrame>(&mut reader) {
            let event = match &frame.request {
                ApiRequest::Hello { .. } => ApiEvent::HelloOk {
                    runtime_identity: None,
                    version: jcode_sdk::api::API_VERSION_MAJOR,
                    server: "remote-test".into(),
                    capabilities: vec![],
                },
                ApiRequest::Ping => ApiEvent::Pong,
                ApiRequest::ListSessions { .. } => ApiEvent::Sessions {
                    sessions: listed.iter().map(|id| info(id)).collect(),
                    workers: Default::default(),
                },
                ApiRequest::AttachSession { session_id } => ApiEvent::Error {
                    code: jcode_sdk::api::ErrorCode::UnknownSession,
                    message: format!(
                        "Unknown session '{session_id}' or session could not be loaded for attachment: No such file or directory (os error 2)"
                    ),
                },
                _ => ApiEvent::Ok,
            };
            if write_frame(
                &mut *writer.lock().unwrap(),
                &ServerFrame::reply(frame.id, event),
            )
            .is_err()
            {
                break;
            }
        }
    });
    JcodeClient::connect_with(
        Box::new(Transport(socket)),
        ConnectOptions {
            ensure_runtime: false,
            request_timeout: Some(Duration::from_secs(2)),
            ..Default::default()
        },
    )
    .unwrap()
}

/// A saved remote session that its machine has lost (a session that never ran a
/// turn leaves no file, so a restarted daemon cannot load it) must stop the
/// reconnect loop and be reported once, instead of looping behind an empty panel.
#[test]
fn remote_session_the_machine_does_not_have_is_reported_gone_once() {
    let ui_id = remote::namespace("paired:aabbccddeeff00112233445566778899", "lost-id");
    let (_commands, rx) = channel();
    let (tx, updates) = update_channel();
    let (done_tx, done) = channel();
    let worker_id = ui_id.clone();
    let mut attempts = 0;
    let worker = std::thread::spawn(move || {
        session_worker_with_connector(worker_id, rx, tx, None, |_| {
            attempts += 1;
            // The attach, then one probe connection for the list. No third.
            assert!(attempts <= 2, "a gone session must not reconnect");
            Ok(unknown_session_machine(&[]))
        });
        done_tx.send(()).unwrap();
    });
    done.recv_timeout(Duration::from_secs(5))
        .expect("a session its machine does not have must end the worker");
    worker.join().unwrap();
    let mut gone = 0;
    while let Ok(update) = updates.try_recv() {
        match update {
            Update::SessionGone { session_id } => {
                assert_eq!(session_id, ui_id);
                gone += 1;
            }
            Update::SessionLost { .. } => panic!("a gone session is not a lost connection"),
            _ => {}
        }
    }
    assert_eq!(gone, 1);
}

/// `unknown_session` is how the bridge words any failed attach, so a session
/// the machine still lists keeps retrying instead of being thrown away.
#[test]
fn remote_session_the_machine_still_lists_keeps_retrying() {
    let ui_id = remote::namespace("paired:aabbccddeeff00112233445566778899", "listed-id");
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let worker = std::thread::spawn(move || {
        session_worker_with_connector(ui_id, rx, tx, None, |_| {
            Ok(unknown_session_machine(&["listed-id"]))
        });
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut lost = false;
    while !lost && Instant::now() < deadline {
        match updates.try_recv() {
            Ok(Update::SessionLost { .. }) => lost = true,
            Ok(Update::SessionGone { .. }) => panic!("a listed session is not gone"),
            Ok(_) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    assert!(lost, "the failed attach is still reported as a lost connection");
    drop(commands);
    worker.join().unwrap();
    assert!(
        !std::iter::from_fn(|| updates.try_recv().ok())
            .any(|update| matches!(update, Update::SessionGone { .. }))
    );
}
