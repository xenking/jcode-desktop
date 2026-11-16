// Acceptance entrypoint driven by tests/e2e/reload_recovery_owner.py.
#[test]
#[ignore = "requires the isolated real daemon and bridge fixture"]
fn real_daemon_bridge_desktop_recovery_without_user_input() {
    let socket = std::env::var_os("JCODE_RECOVERY_ACCEPTANCE_API_SOCKET")
        .expect("isolated bridge socket");
    let session = std::env::var("JCODE_RECOVERY_ACCEPTANCE_SESSION")
        .expect("interrupted fixture session");
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let worker = std::thread::spawn(move || {
        session_worker_with_connector(session, rx, tx, None, |_| {
            JcodeClient::connect(ConnectOptions {
                socket_path: Some(socket.clone().into()),
                ensure_runtime: false,
                request_timeout: Some(Duration::from_secs(10)),
                ..Default::default()
            })
        });
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut running = false;
    while Instant::now() < deadline {
        match updates.try_recv() {
            Ok(Update::Event { event: ApiEvent::SessionStatus { status, .. }, .. }) if status == "running" => {
                println!("DESKTOP_RECOVERY_RUNNING");
                running = true;
                break;
            }
            Ok(Update::CommandFailed { reason, .. } | Update::SendFailed { reason, .. }) => panic!("{reason}"),
            Ok(_) | Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    assert!(running, "Desktop must observe daemon-owned recovery without sending input");
}
