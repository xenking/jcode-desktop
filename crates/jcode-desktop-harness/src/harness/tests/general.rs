//! Worker reconnect, activity and steering tests.

use super::*;
use std::time::Instant;

#[test]
fn unknown_session_reconnects_back_off_to_the_cap() {
    let unknown = jcode_sdk::Error::new(
        jcode_sdk::ErrorKind::Harness(jcode_sdk::api::ErrorCode::UnknownSession),
        "gone",
    );
    let transport = jcode_sdk::Error::new(jcode_sdk::ErrorKind::ConnectFailed, "down");
    let base = Duration::from_millis(300);
    assert_eq!(
        next_reconnect_delay(base, &unknown),
        Duration::from_millis(1200)
    );
    assert_eq!(
        next_reconnect_delay(base, &transport),
        Duration::from_millis(600)
    );
    let mut delay = base;
    for _ in 0..20 {
        delay = next_reconnect_delay(delay, &unknown);
    }
    assert_eq!(delay, MAX_RECONNECT_DELAY);
}

#[test]
fn runtime_revive_is_throttled() {
    let now = Instant::now();
    assert!(revive_due(None, now));
    assert!(!revive_due(Some(now), now + Duration::from_millis(500)));
    assert!(revive_due(Some(now), now + RUNTIME_REVIVE_INTERVAL));
}

#[test]
fn reconnect_logging_thins_out() {
    let logged = (0..1000)
        .filter(|&attempt| should_log_reconnect(attempt))
        .count();
    assert!(logged <= 13, "{logged}");
    assert!(should_log_reconnect(0));
}

#[test]
fn reconnect_wait_wakes_on_commands_and_stops() {
    let (tx, rx) = channel();
    let mut pending = VecDeque::new();
    let started = Instant::now();
    assert_eq!(
        wait_for_reconnect(&rx, &mut pending, Duration::from_millis(20)),
        ReconnectWait::Elapsed
    );
    tx.send(SessionCommand::Cancel).unwrap();
    assert_eq!(
        wait_for_reconnect(&rx, &mut pending, Duration::from_secs(30)),
        ReconnectWait::Woken
    );
    assert_eq!(pending.len(), 1);
    tx.send(SessionCommand::Stop).unwrap();
    assert_eq!(
        wait_for_reconnect(&rx, &mut pending, Duration::from_secs(30)),
        ReconnectWait::Stop
    );
    drop(tx);
    assert_eq!(
        wait_for_reconnect(&rx, &mut pending, Duration::from_secs(30)),
        ReconnectWait::Stop
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn bounded_bridge_batches_preserve_order_and_leave_backlog_queued() {
    let (updates, receiver) = update_channel();
    let (commands, _command_rx) = channel();
    let bridge = Bridge {
        _lifetime: std::sync::Arc::new(BridgeLifetime(commands.clone())),
        commands,
        updates: receiver,
    };
    for index in 0..1000 {
        updates.try_send(Update::Status(index.to_string())).unwrap();
    }
    assert!(bridge.drain_up_to(0).is_empty());
    assert_eq!(bridge.updates.len(), 1000);
    let mut received = Vec::new();
    while !bridge.updates.is_empty() {
        let batch = bridge.drain_up_to(128);
        assert!(!batch.is_empty() && batch.len() <= 128);
        received.extend(batch.into_iter().map(|update| match update {
            Update::Status(value) => value,
            _ => panic!("unexpected event"),
        }));
    }
    assert_eq!(
        received,
        (0..1000).map(|i| i.to_string()).collect::<Vec<_>>()
    );
    assert!(bridge.drain_up_to(128).is_empty());
    drop(updates);
    assert!(bridge.drain_up_to(128).is_empty());
}

#[test]
fn runtime_info_backlog_collapses_to_newest_per_session_in_order() {
    let (updates, receiver) = update_channel();
    let (commands, _command_rx) = channel();
    let bridge = Bridge {
        _lifetime: std::sync::Arc::new(BridgeLifetime(commands.clone())),
        commands,
        updates: receiver,
    };
    let runtime = |session: &str, model: &str| Update::Event {
        session_id: session.into(),
        event: ApiEvent::RuntimeInfo {
            speed_mode: Default::default(),
            speed_options: Vec::new(),
            session_id: session.into(),
            provider: None,
            model: Some(model.into()),
            reasoning_effort: None,
            auth_method: None,
            account_label: None,
            routes: Vec::new(),
        },
    };
    // A usage-update storm: thousands of full catalogs for two panels,
    // interleaved with ordinary updates that must all survive in order.
    for index in 0..5000 {
        updates
            .try_send(runtime("a", &format!("a{index}")))
            .unwrap();
        updates
            .try_send(runtime("b", &format!("b{index}")))
            .unwrap();
        if index % 1000 == 0 {
            updates.try_send(Update::Status(index.to_string())).unwrap();
        }
    }
    // Catalog payloads are already bounded before any consumer runs.
    assert_eq!(bridge.updates.len(), 7);
    let first = bridge.updates.try_recv().unwrap();
    let batch = bridge.extend_batch(vec![first], 127);
    let mut rest = batch;
    while !bridge.updates.is_empty() {
        rest.extend(bridge.drain_up_to(127));
    }
    let describe = |update: &Update| match update {
        Update::Status(value) => format!("status {value}"),
        Update::Event {
            event: ApiEvent::RuntimeInfo { model, .. },
            ..
        } => format!("runtime {}", model.clone().unwrap_or_default()),
        _ => panic!("unexpected update"),
    };
    let seen: Vec<_> = rest.iter().map(describe).collect();
    let runtime_count = seen.iter().filter(|s| s.starts_with("runtime")).count();
    // At most one catalog per session per MAX_SCAN window, never thousands.
    assert!(
        runtime_count <= 6,
        "{runtime_count} catalogs applied: {seen:?}"
    );
    assert!(seen.contains(&"runtime a4999".to_string()));
    assert!(seen.contains(&"runtime b4999".to_string()));
    let statuses: Vec<_> = seen.iter().filter(|s| s.starts_with("status")).collect();
    assert_eq!(
        statuses,
        [
            "status 0",
            "status 1000",
            "status 2000",
            "status 3000",
            "status 4000"
        ]
    );
    // The last catalogs follow every status update that preceded them.
    let last_status = seen.iter().rposition(|s| s.starts_with("status")).unwrap();
    let last_a = seen.iter().rposition(|s| s == "runtime a4999").unwrap();
    assert!(last_a > last_status);
}

#[test]
fn daemon_connection_closed_is_a_transport_event() {
    let event = ApiEvent::Error {
        code: jcode_sdk::api::ErrorCode::Internal,
        message: "daemon connection closed".into(),
    };
    assert!(is_daemon_connection_closed(&event));
}

#[test]
fn wrong_session_attachment_is_retried_as_a_transport_failure() {
    let mismatch = "this connection is attached to `session_old`, not `session_new`; attach to it first or use another connection";
    let images = vec![("image/png".into(), "payload".into())];
    let mut pending = VecDeque::new();
    assert!(queue_wrong_session_retry(
        mismatch,
        &mut pending,
        "original prompt".into(),
        images.clone(),
        vec![],
    ));
    let Some(SessionCommand::Send {
        content,
        images: queued_images,
        ..
    }) = pending.pop_front()
    else {
        panic!("original send was not queued for retry");
    };
    assert_eq!(content, "original prompt");
    assert_eq!(queued_images, images);

    assert!(!queue_wrong_session_retry(
        "ordinary provider error",
        &mut pending,
        "must not retry".into(),
        Vec::new(),
        Vec::new(),
    ));
    assert!(pending.is_empty());
}

#[test]
fn asynchronous_attachment_error_replays_the_unaccepted_prompt() {
    let event = ApiEvent::Error {
        code: jcode_sdk::api::ErrorCode::UnknownSession,
        message: "this connection is attached to `old`, not `wanted`; attach to it first or use another connection".into(),
    };
    let images = vec![("image/png".into(), "payload".into())];
    let mut unaccepted = VecDeque::from([SessionCommand::Send {
        content: "hi".into(),
        images: images.clone(),
        attachments: vec![],
    }]);
    let mut pending = VecDeque::new();

    assert!(recover_async_attachment_mismatch(
        &event,
        &mut unaccepted,
        &mut pending
    ));
    assert!(unaccepted.is_empty());
    assert!(matches!(
        pending.pop_front(),
        Some(SessionCommand::Send { content, images: queued, .. })
            if content == "hi" && queued == images
    ));
}

#[test]
fn session_event_identity_prevents_cross_session_activity() {
    let event = ApiEvent::SessionStatus {
        session_id: "other-session".into(),
        status: "generating".into(),
        pending_soft_interrupts: None,
    };
    assert_eq!(event_session_id(&event), Some("other-session"));
    assert_ne!(event_session_id(&event), Some("this-session"));
}

#[test]
fn speed_events_keep_the_remote_session_namespace_and_witness() {
    use jcode_sdk::api::SpeedMode;

    let remote_id = remote::namespace("user@desktop", "shared-id");
    let address = remote::SessionAddress::parse(&remote_id).unwrap();
    let event = ApiEvent::SpeedModeReported {
        session_id: "shared-id".into(),
        requested: Some(SpeedMode::Fast),
        actual: SpeedMode::Standard,
    };
    assert_eq!(event_session_id(&event), Some("shared-id"));
    assert!(
        matches!(namespace_event(event, &address), ApiEvent::SpeedModeReported {
        session_id,
        requested: Some(SpeedMode::Fast),
        actual: SpeedMode::Standard,
    } if session_id == remote_id)
    );
}

#[test]
fn connection_scoped_ask_with_empty_identity_is_not_filtered_out() {
    let event = ApiEvent::AskRequest {
        session_id: String::new(),
        request_id: "ask-call".into(),
        tool_call_id: "call".into(),
        questions: vec![],
    };
    assert_eq!(event_session_id(&event), None);
}

/// The daemon replays occupancy right after an attach. Without a session
/// identity the event would reach whichever panel shares the connection and
/// paint another session's context ring.
#[test]
fn replayed_context_occupancy_is_scoped_to_its_session() {
    let event = ApiEvent::ContextOccupancy {
        session_id: "other-session".into(),
        prompt_tokens: 41_000,
    };
    assert_eq!(event_session_id(&event), Some("other-session"));
}

/// Opt-in acceptance check against the real local runtime and configured
/// model. Run with `cargo test live_prompt_round_trip -- --ignored`.
#[test]
#[ignore = "requires a configured model and makes a real model request"]
fn live_prompt_round_trip() {
    let bridge = spawn();
    bridge.send(Command::CreateSession {
        working_dir: None,
        request_id: None,
    });

    let deadline = Instant::now() + Duration::from_secs(120);
    let (session_id, mut attached) = loop {
        assert!(
            Instant::now() < deadline,
            "runtime did not create a session"
        );
        let updates = bridge.drain();
        if let Some(session_id) = updates.iter().find_map(|update| match update {
            Update::SessionCreated { session, .. } => Some(session.session_id.clone()),
            _ => None,
        }) {
            let attached = updates.iter().any(|update| {
                matches!(update, Update::SessionConnected { session_id: connected } if connected == &session_id)
            });
            break (session_id, attached);
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    bridge.send(Command::Watch {
        session_id: session_id.clone(),
    });

    // Match the real UI: the panel is ready when its dedicated worker has
    // attached. SessionStatus wording belongs to the daemon and has changed
    // over time, while SessionConnected is the bridge's public readiness
    // signal.
    while !attached {
        assert!(
            Instant::now() < deadline,
            "panel never reached attached status"
        );
        attached = bridge.drain().into_iter().any(
            |update| matches!(update, Update::SessionConnected { session_id: ref connected } if connected == &session_id),
        );
        if !attached {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    bridge.send(Command::Send {
        session_id: session_id.clone(),
        content: "Reply with exactly JCODE_DESKTOP_OK and nothing else.".into(),
        images: Vec::new(),
        attachments: Vec::new(),
    });

    let mut accepted = false;
    loop {
        assert!(
            Instant::now() < deadline,
            "runtime never accepted the submitted prompt"
        );
        for update in bridge.drain() {
            match update {
                Update::MessageSubmitted {
                    session_id: event_session,
                } if event_session == session_id => accepted = true,
                Update::SendFailed {
                    session_id: event_session,
                    reason,
                } if event_session == session_id => panic!("send failed: {reason}"),
                _ => {}
            }
        }
        if accepted {
            bridge.send(Command::Cancel {
                session_id: session_id.clone(),
            });
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn sending_without_a_watched_worker_starts_one_and_delivers() {
    let mut workers = HashMap::new();
    let (tx, rx) = channel();
    send_to_session_worker(
        &mut workers,
        "new-session".into(),
        SessionCommand::Send {
            content: "hello".into(),
            images: vec![],
            attachments: vec![],
        },
        |_| tx.clone(),
    );

    assert!(workers.contains_key("new-session"));
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(100)),
        Ok(SessionCommand::Send { content, .. }) if content == "hello"
    ));
}

#[test]
fn sending_to_a_disconnected_worker_restarts_it_without_losing_the_message() {
    let mut workers = HashMap::new();
    let (stale_tx, stale_rx) = channel();
    drop(stale_rx);
    workers.insert("stale-session".into(), stale_tx);
    let (replacement_tx, replacement_rx) = channel();
    let mut starts = 0;

    send_to_session_worker(
        &mut workers,
        "stale-session".into(),
        SessionCommand::Send {
            content: "do not drop me".into(),
            images: vec![],
            attachments: vec![],
        },
        |_| {
            starts += 1;
            replacement_tx.clone()
        },
    );

    assert_eq!(starts, 1);
    assert!(matches!(
        replacement_rx.recv_timeout(Duration::from_millis(100)),
        Ok(SessionCommand::Send { content, .. }) if content == "do not drop me"
    ));
}

#[test]
fn messages_are_retained_while_the_runtime_is_disconnected() {
    let (tx, rx) = channel();
    tx.send(SessionCommand::Send {
        content: "hi".into(),
        images: vec![("image/png".into(), "cG5n".into())],
        attachments: vec![],
    })
    .unwrap();
    tx.send(SessionCommand::Cancel).unwrap();
    let mut pending = VecDeque::new();

    assert!(!collect_disconnected_commands(&rx, &mut pending));
    assert_eq!(pending.len(), 2);
    assert!(matches!(
        pending.pop_front(),
        Some(SessionCommand::Send { content, images, .. })
            if content == "hi" && images == [("image/png".into(), "cG5n".into())]
    ));
    assert!(matches!(pending.pop_front(), Some(SessionCommand::Cancel)));
}

#[test]
fn closing_a_disconnected_panel_stops_its_worker() {
    let (tx, rx) = channel();
    tx.send(SessionCommand::Stop).unwrap();
    let mut pending = VecDeque::new();

    assert!(collect_disconnected_commands(&rx, &mut pending));
    assert!(pending.is_empty());
}

#[test]
fn active_worker_observes_command_channel_disconnect_after_pending_work() {
    let (tx, rx) = channel();
    let mut pending = VecDeque::from([SessionCommand::Cancel]);
    drop(tx);

    assert!(matches!(
        next_worker_command(&rx, &mut pending),
        WorkerCommand::Command(SessionCommand::Cancel)
    ));
    assert!(matches!(
        next_worker_command(&rx, &mut pending),
        WorkerCommand::Disconnected
    ));
}

#[test]
fn turn_activity_tracks_acceptance_completion_and_session_status() {
    let mut active = false;
    update_turn_activity(
        &ApiEvent::SessionStatus {
            session_id: "s1".into(),
            status: "attached".into(),
            pending_soft_interrupts: None,
        },
        &mut active,
    );
    assert!(!active, "attaching an idle session is not an active turn");

    update_turn_activity(
        &ApiEvent::MessageAccepted {
            session_id: "s1".into(),
        },
        &mut active,
    );
    assert!(active);

    update_turn_activity(
        &ApiEvent::SessionStatus {
            session_id: "s1".into(),
            status: "idle".into(),
            pending_soft_interrupts: None,
        },
        &mut active,
    );
    assert!(!active);
}

#[test]
fn completed_worker_can_detach_without_waiting_for_idle_status() {
    let mut active = true;
    update_turn_activity(
        &ApiEvent::TurnDone {
            session_id: "s1".into(),
            pending_soft_interrupts: None,
        },
        &mut active,
    );
    assert!(!active);
    assert!(should_detach_on_drop(active));

    // A later turn must restore interruption detection.
    update_turn_activity(
        &ApiEvent::MessageAccepted {
            session_id: "s1".into(),
        },
        &mut active,
    );
    assert!(!should_detach_on_drop(active));
}

#[test]
fn streaming_worker_disconnect_keeps_crash_detection_armed() {
    assert!(!should_detach_on_drop(true));
    assert!(should_detach_on_drop(false));
}

#[test]
fn active_turn_race_error_is_recognized_for_interrupt_retry() {
    assert!(is_already_processing_error("Already processing a message"));
    assert!(is_already_processing_error(
        "request failed: ALREADY PROCESSING A MESSAGE"
    ));
    assert!(!is_already_processing_error("daemon connection closed"));
}

#[test]
fn observer_activity_and_reconnect_status_control_worker_steering() {
    let mut active = false;
    update_turn_activity(
        &ApiEvent::TextDelta {
            message_id: None,
            session_id: "s1".into(),
            text: "observed".into(),
        },
        &mut active,
    );
    assert!(active);
    for status in ["idle", "cancelled", "canceled"] {
        update_turn_activity(
            &ApiEvent::SessionStatus {
                session_id: "s1".into(),
                status: "processing".into(),
                pending_soft_interrupts: None,
            },
            &mut active,
        );
        assert!(active);
        update_turn_activity(
            &ApiEvent::SessionStatus {
                session_id: "s1".into(),
                status: "attached".into(),
                pending_soft_interrupts: None,
            },
            &mut active,
        );
        assert!(active, "transport notification must not change steering");
        update_turn_activity(
            &ApiEvent::SessionStatus {
                session_id: "s1".into(),
                status: status.into(),
                pending_soft_interrupts: None,
            },
            &mut active,
        );
        assert!(!active);
    }
}
