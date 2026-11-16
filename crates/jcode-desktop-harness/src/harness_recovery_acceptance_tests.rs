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

#[test]
#[ignore = "requires scripts/accept-stale-bridge.py isolated bundle fixture"]
fn real_bundle_bridge_replaces_stale_bridge_and_delivers_peer_prompt_live() {
    use jcode_build_support::runtime_generation as generation;
    use std::io::Write;
    let session = std::env::var("JCODE_STALE_BRIDGE_SESSION").expect("isolated fixture session");
    let expected = std::env::var_os("JCODE_STALE_BRIDGE_EXPECTED_BINARY").expect("new bundle binary");
    let old_socket = jcode_sdk::api::api_socket_path();
    let own_socket = crate::runtime_generation::local_bridge_socket().unwrap();
    assert_ne!(own_socket, old_socket, "old bridge lock must not block this bundle");
    let wrong_binary = std::env::var_os("JCODE_STALE_BRIDGE_WRONG_BINARY").expect("copied wrong bundle");
    struct OwnedBridge(std::process::Child);
    impl Drop for OwnedBridge {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut wrong = OwnedBridge(std::process::Command::new(wrong_binary)
        .args(["--no-update", "--no-selfdev", "api-bridge"])
        .env("JCODE_API_SOCKET", &own_socket)
        .env_remove(generation::TARGET_ENV)
        .env_remove(generation::RELOAD_ID_ENV)
        .env_remove(generation::GENERATION_ENV)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !crate::platform::socket_live(&own_socket) {
        assert!(Instant::now() < deadline, "wrong bridge failed to bind");
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("BUNDLE_BRIDGE_WRONG_PID {}", wrong.0.id());
    let (startup, _startup_updates) = update_channel();
    start_local_runtime(startup);
    let deadline = Instant::now() + Duration::from_secs(5);
    while wrong.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "mismatched bridge did not retire after restart");
        std::thread::sleep(Duration::from_millis(5));
    }
    let client = connect("stale-bridge-acceptance").unwrap();
    let identity = client.runtime_identity.as_ref().expect("verified bridge identity");
    assert_eq!(identity.binary_path, std::path::PathBuf::from(expected));
    println!("BUNDLE_BRIDGE_IDENTITY {}", serde_json::to_string(identity).unwrap());
    println!("BUNDLE_BRIDGE_SOCKET {}", own_socket.display());
    drop(client);

    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let worker = std::thread::spawn(move || session_worker(session, rx, tx, None));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut live_prompt = false;
    while Instant::now() < deadline {
        match updates.try_recv() {
            Ok(Update::SessionConnected { .. }) => {
                println!("BUNDLE_BRIDGE_DESKTOP_ATTACHED");
                std::io::stdout().flush().unwrap();
            }
            Ok(Update::Event { event: ApiEvent::PeerUserMessage { content, .. }, .. }) => {
                assert_eq!(content, "stale-bridge-peer-prompt");
                println!("BUNDLE_BRIDGE_PEER_PROMPT_LIVE");
                live_prompt = true;
                break;
            }
            Ok(Update::CommandFailed { reason, .. } | Update::SendFailed { reason, .. }) => panic!("{reason}"),
            Ok(_) | Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    commands.send(SessionCommand::Stop).unwrap();
    worker.join().unwrap();
    assert!(live_prompt, "peer prompt must reach the real Desktop worker without history reload");
}
