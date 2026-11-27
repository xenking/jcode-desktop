use super::*;

fn stream_ask(panel: &mut Panel, cx: &mut Context<Panel>) -> ApiEvent {
    let request: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let call_id = request["tool_call_id"].as_str().unwrap();
    let input = serde_json::json!({"questions": request["questions"]}).to_string();
    for event in [
        serde_json::json!({"ev":"tool_start", "session_id":"session-a", "call_id":call_id, "name":"ask"}),
        serde_json::json!({"ev":"tool_input_delta", "session_id":"session-a", "call_id":call_id, "delta":input}),
        serde_json::json!({"ev":"tool_exec", "session_id":"session-a", "call_id":call_id, "name":"ask"}),
    ] {
        panel.apply(&serde_json::from_value(event).unwrap(), cx);
    }
    serde_json::from_value(request).unwrap()
}

#[gpui::test]
fn missing_ask_request_recovers_from_live_tool_and_merges_late_event(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let event = panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        stream_ask(panel, cx)
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-card").is_none(),
        "wait for live AskRequest first"
    );
    vcx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-card").is_some(),
        "the running tool must be answerable without an AskRequest"
    );
    panel.update(vcx, |panel, cx| {
        let card = panel.ask_card.as_mut().unwrap();
        let ApiEvent::AskRequest {
            request_id,
            tool_call_id,
            ..
        } = &event
        else {
            unreachable!()
        };
        assert_eq!(&card.request_id, request_id);
        assert_eq!(&card.tool_call_id, tool_call_id);
        card.toggle(0, 1);
        let editor = card.comment_inputs[0][1].clone();
        panel.apply(&event, cx);
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![1]);
        assert_eq!(card.comment_inputs[0][1], editor);
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel, request_id, Ok(()), cx);
        panel.apply(&event, cx);
    });
    vcx.executor().advance_clock(Duration::from_secs(2));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-card").is_none(),
        "neither replay nor the unfinished transcript row may reopen an acknowledged ask"
    );
}

#[gpui::test]
fn every_replayed_ask_stays_answerable_in_order(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    let ask = |request_id: &str, call_id: &str, header: &str| -> ApiEvent {
        serde_json::from_value(serde_json::json!({
            "ev": "ask_request", "session_id": "session-a",
            "request_id": request_id, "tool_call_id": call_id,
            "questions": [{"header": header, "question": format!("{header}?"),
                "options": [{"label": "A"}, {"label": "B"}]}]
        }))
        .unwrap()
    };
    // A reconnect replays every pending question: an earlier `ask` and a
    // computer-use consent prompt that were both waiting on the owner.
    let team = ask("ask-team", "team", "Teams");
    let consent = ask("ask-computer-consent", "consent", "Computer use");
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&team, cx);
        panel.apply(&consent, cx);
        panel.apply(&team, cx);
        assert_eq!(
            panel.ask_card.as_ref().unwrap().request_id,
            "ask-team",
            "the first question stays on screen"
        );
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel, "ask-team", Ok(()), cx);
        assert_eq!(
            panel.ask_card.as_ref().map(|card| card.request_id.as_str()),
            Some("ask-computer-consent"),
            "a second pending question must not be lost behind the first"
        );
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel, "ask-computer-consent", Ok(()), cx);
        assert!(panel.ask_card.is_none());
    });
    let answered: Vec<_> = commands
        .try_iter()
        .filter_map(|command| match command {
            crate::harness::Command::AskResponse { request_id, .. } => Some(request_id),
            _ => None,
        })
        .collect();
    assert_eq!(answered, ["ask-team", "ask-computer-consent"]);

    // A finished tool retires its queued question without showing it.
    let first = ask("ask-one", "one", "One");
    let second = ask("ask-two", "two", "Two");
    panel.update(vcx, |panel, cx| {
        panel.apply(&first, cx);
        panel.apply(&second, cx);
        panel.apply(
            &serde_json::from_value(serde_json::json!({
                "ev": "tool_done", "session_id": "session-a", "call_id": "two",
                "name": "ask", "output": "User declined"
            }))
            .unwrap(),
            cx,
        );
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel, "ask-one", Ok(()), cx);
        assert!(
            panel.ask_card.is_none(),
            "a finished ask is not shown again"
        );
    });
}

#[gpui::test]
fn rejected_answer_stays_on_the_card_with_the_reason(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
        let request = panel.ask_card.as_ref().unwrap().request_id.clone();
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(
            panel,
            &request,
            Err(AskResponseError::Failed(
                "Could not send your answer: your answer was not accepted".into(),
            )),
            cx,
        );
        let card = panel
            .ask_card
            .as_ref()
            .expect("a rejected answer keeps the card");
        assert!(!card.submitting());
        assert!(
            card.submit_error
                .as_deref()
                .unwrap()
                .contains("not accepted")
        );
    });
}

#[gpui::test]
fn stale_cold_wake_releases_composer_without_another_response(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    let request = "ask-cold-wake-expired";
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.input.update(cx, |input, cx| {
            input.set_content("draft to keep".into(), cx)
        });
        let mut event: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        event["request_id"] = request.into();
        panel.apply(&serde_json::from_value(event).unwrap(), cx);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let choice = panel.read(cx).ask_card.as_ref().unwrap().option_focus[0][0].clone();
        window.focus(&choice, cx);
    });
    panel.update(vcx, |panel, cx| {
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel,
            request,
            Err(AskResponseError::Stale),
            cx,
        );
        assert!(panel.ask_card.is_none(), "a stale answer must not leave a blocking card");
        assert!(panel.items.iter().any(|item| matches!(item, super::super::Item::Error(message) if message.contains("Your answer was not accepted"))));
        assert_eq!(panel.input.read(cx).snapshot().content, "draft to keep");
    });
    vcx.run_until_parked();
    assert!(vcx.update(|window, cx| { panel.read(cx).input.read(cx).is_focused(window) }));
    let responses: Vec<_> = commands
        .try_iter()
        .filter(|command| matches!(command, Command::AskResponse { .. }))
        .collect();
    assert_eq!(
        responses.len(),
        1,
        "closing a stale card must not send another answer"
    );
}

#[gpui::test]
fn ask_recovery_after_snapshot_ignores_only_expired_tools(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_recording().0,
            cx,
        )
    });
    let fallback_tool = |id: &str| super::super::Item::Tool {
        call_id: id.into(),
        name: "ask".into(),
        input: serde_json::json!({"questions": [question(false)]}).to_string(),
        output: String::new(),
        done: false,
        error: None,
        ask: None,
    };
    panel.update(vcx, |panel, cx| {
        panel.items.push(fallback_tool("expired"));
        panel.reconcile_asks(1, Some(&[]), cx);
        assert!(panel.recoverable_ask().is_none());
        panel.items.push(fallback_tool("fresh"));
        panel.schedule_ask_recovery(cx);
    });
    vcx.run_until_parked();
    vcx.executor().advance_clock(Duration::from_secs(2));
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.ask_card.as_ref().unwrap().request_id, "ask-fresh");
    });
}

#[gpui::test]
fn reconnect_retires_missing_asks_and_late_ack_cannot_close_the_current_attempt(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    panel.update(vcx, |panel, cx| {
        let expired = "ask-cold-wake-old";
        let live = "ask-cold-wake-live";
        let live_pending = [live.into()];
        panel.reconcile_asks(1, None, cx);
        panel.show_live_ask(expired, expired, &[question(false)], AskKind::Question, cx);
        panel.show_live_ask(live, live, &[question(true)], AskKind::Question, cx);
        panel.answer_ask(AskResponse::declined(), cx);
        let expired_attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
        panel.reconcile_asks(2, Some(&live_pending), cx);
        assert_eq!(panel.ask_card.as_ref().unwrap().request_id, live);
        panel.ask_card.as_mut().unwrap().toggle(0, 1);
        panel.answer_ask(panel.ask_card.as_ref().unwrap().response(), cx);
        let previous_attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
        panel.reconcile_asks(3, Some(&live_pending), cx);
        assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], [1]);
        assert!(!panel.ask_card.as_ref().unwrap().submitting());
        panel.answer_ask(panel.ask_card.as_ref().unwrap().response(), cx);
        panel.ask_response_settled(expired, expired_attempt, Ok(()), cx);
        panel.ask_response_settled(live, previous_attempt, Ok(()), cx);
        assert!(panel.ask_card.as_ref().unwrap().submitting());
        settle_test_ask(panel, live, Ok(()), cx);
        assert!(panel.ask_card.is_none());
    });
    let answer_attachments: Vec<_> = commands
        .try_iter()
        .filter_map(|command| match command {
            Command::AskResponse { attachment_id, .. } => Some(attachment_id),
            _ => None,
        })
        .collect();
    assert_eq!(answer_attachments, [1, 2, 3]);
}

#[gpui::test]
fn acknowledged_ask_returns_keyboard_focus_to_composer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, _) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
    });
    vcx.run_until_parked();
    let choice = vcx.update(|window, cx| {
        let handle = panel.read(cx).ask_card.as_ref().unwrap().option_focus[0][0].clone();
        window.focus(&handle, cx);
        handle
    });
    vcx.run_until_parked();
    assert!(vcx.update(|window, _| choice.is_focused(window)));
    panel.update(vcx, |panel, cx| {
        let request = panel.ask_card.as_ref().unwrap().request_id.clone();
        panel.answer_ask(AskResponse::declined(), cx);
        settle_test_ask(panel, &request, Ok(()), cx);
    });
    vcx.run_until_parked();
    assert!(vcx.update(|window, cx| { panel.read(cx).input.read(cx).is_focused(window) }));
}

#[gpui::test]
fn restored_in_flight_ask_recovers_without_an_event(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace.push_test_panel("session-restored", cx);
        workspace
    });
    let (live, restored) = workspace.read_with(vcx, |workspace, _| {
        (
            workspace.test_panel(0).unwrap(),
            workspace.test_panel(1).unwrap(),
        )
    });
    let snapshot = live.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        stream_ask(panel, cx);
        panel.snapshot_for_reload(cx)
    });
    restored.update(vcx, |panel, cx| panel.restore_snapshot(snapshot, cx));
    vcx.run_until_parked();
    vcx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
    restored.read_with(vcx, |panel, _| {
        assert_eq!(
            panel.ask_card.as_ref().unwrap().questions[0]
                .header
                .as_deref(),
            Some("Database")
        );
    });
}

#[gpui::test]
fn typed_comment_selects_its_option_without_submitting(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
    });
    vcx.run_until_parked();

    let option = vcx.debug_bounds("ask-option-0-0").unwrap().center();
    vcx.simulate_click(option, gpui::Modifiers::default());
    let comment = vcx.debug_bounds("ask-comment-button-0-1").unwrap().center();
    vcx.simulate_click(comment, gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
        assert_eq!(
            answer.selected,
            ["Postgres"],
            "empty comment focus changed the answer"
        );
    });
    vcx.simulate_input("reason");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
        assert_eq!(
            answer.selected,
            ["SQLite"],
            "nonempty comment must select its radio option"
        );
    });
    vcx.simulate_keystrokes("shift-enter");
    vcx.simulate_input("second line");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !commands
            .try_iter()
            .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
    );
    let submit = vcx.debug_bounds("ask-submit").unwrap().center();
    vcx.simulate_click(submit, gpui::Modifiers::default());
    vcx.run_until_parked();
    let response = commands
        .try_iter()
        .find_map(|cmd| match cmd {
            crate::harness::Command::AskResponse { response, .. } => Some(response),
            _ => None,
        })
        .expect("explicit Submit must send the answer");
    let answers = response.answers.unwrap();
    assert_eq!(answers[0].selected, ["SQLite"]);
    assert_eq!(answers[0].comments["SQLite"], "reason\nsecond line");
}

#[gpui::test]
fn real_ask_request_event_renders_card_and_submits(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    // The composer (and the ask card above it) docks over a real transcript.
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
    vcx.run_until_parked();
    for _ in 0..120 {
        if vcx.debug_bounds("composer-dock").is_some() {
            break;
        }
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    assert!(vcx.debug_bounds("composer-dock").is_some());
    assert!(vcx.debug_bounds("ask-card").is_none());
    let bar_before = vcx.debug_bounds("composer-tabs").unwrap();

    panel.update(vcx, |panel, cx| {
        panel.seed_screenshot_queued(2, 0);
        panel.input.update(cx, |input, cx| {
            input.set_content("valuable draft".into(), cx)
        });
    });
    vcx.update(|window, cx| {
        let focus = panel.read(cx).input_focus_handle(cx);
        window.focus(&focus, cx);
    });
    let event: jcode_sdk::ApiEvent =
        serde_json::from_str(REAL_ASK_REQUEST).expect("real ask_request frame parses");
    let ApiEvent::AskRequest { tool_call_id, .. } = &event else {
        unreachable!();
    };
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &serde_json::from_value(serde_json::json!({
                "ev":"tool_start", "session_id": panel.session_id,
                "call_id":tool_call_id, "name":"ask"
            }))
            .unwrap(),
            cx,
        );
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();

    assert!(vcx.debug_bounds("ask-card").is_some(), "ask card paints");
    assert!(
        vcx.debug_bounds("composer-tabs").is_some()
            && vcx.debug_bounds("session-status-ask").is_some()
            && vcx.debug_bounds("composer-text-slot").is_none(),
        "ask replaces only the text slot and leaves the composer bar visible"
    );
    assert!(vcx.debug_bounds("composer-ask-pending").is_none());
    assert!(vcx.debug_bounds("session-status-tools").is_none());
    assert!(vcx.debug_bounds("transcript-activity").is_none());
    let ask = vcx.debug_bounds("ask-card").unwrap();
    let queue = vcx
        .debug_bounds("queue-card")
        .expect("visible prompt queue");
    let bar = vcx.debug_bounds("composer-tabs").unwrap();
    assert!(queue.bottom() <= ask.top() && ask.bottom() <= bar.top());
    assert_eq!(
        bar.bottom(),
        bar_before.bottom(),
        "the bar stays anchored to the bottom"
    );
    assert!(
        vcx.debug_bounds("ask-scroll").is_some(),
        "questions have their own scroll area"
    );
    assert!(
        vcx.debug_bounds("ask-other-editor-0").is_some(),
        "free answers are always offered"
    );
    for selector in ["ask-option-0-0", "ask-option-0-1", "ask-option-0-2"] {
        assert!(vcx.debug_bounds(selector).is_some(), "{selector} paints");
    }
    assert!(vcx.debug_bounds("ask-decline").is_some());
    vcx.simulate_keystrokes("2");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, cx| {
        assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![1]);
        assert_eq!(panel.input.read(cx).snapshot().content, "valuable draft");
    });

    let sqlite = vcx.debug_bounds("ask-option-0-0").unwrap();
    vcx.simulate_click(sqlite.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().expect("card still open");
        assert_eq!(card.chosen[0], vec![0]);
        let answers = card.response().answers.unwrap();
        assert_eq!(answers[0].selected, vec!["Postgres"]);
    });

    vcx.simulate_keystrokes("2");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![1])
    });
    vcx.simulate_keystrokes("n");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-comment-0-1").is_some());
    vcx.simulate_input("fast local development");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
        assert_eq!(
            answer.comments.get("SQLite").map(String::as_str),
            Some("fast local development")
        );
    });
    let other = vcx.debug_bounds("ask-other-editor-0").unwrap();
    vcx.simulate_click(other.center(), gpui::Modifiers::default());
    vcx.simulate_input("my own answer");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
        assert_eq!(answer.other.as_deref(), Some("my own answer"));
        assert!(answer.selected.is_empty());
    });
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let request_id = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().request_id.clone()
    });
    assert!(commands.try_iter().any(|command| matches!(command,
        crate::harness::Command::AskResponse { session_id, request_id: id, response, .. }
        if session_id == "session-a" && id == request_id && response.answers.as_ref().is_some_and(|answers| answers[0].other.as_deref() == Some("my own answer"))
    )), "Submit must send the typed answer through the bridge");
    let attempt_id = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().attempt_id.unwrap()
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            crate::harness::Update::AskResponseSettled {
                session_id: "session-a".into(),
                request_id,
                attempt_id,
                result: Ok(()),
            },
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-card").is_none(),
        "card closes only after server acknowledgment"
    );
    panel.read_with(vcx, |panel, cx| {
        assert!(panel.ask_card.is_none());
        assert_eq!(panel.input.read(cx).snapshot().content, "valuable draft");
    });
    assert!(
        vcx.debug_bounds("composer").is_some(),
        "composer returns after answering"
    );
    panel.update(vcx, |panel, cx| {
        let next_tool = serde_json::from_value(serde_json::json!({
            "ev":"tool_start", "session_id":"session-a", "call_id":"next-tool", "name":"bash"
        }))
        .unwrap();
        panel.apply(&next_tool, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-status-tools").is_some());
    assert!(vcx.debug_bounds("transcript-activity").is_some());
}

#[gpui::test]
fn cancelled_turn_leaves_no_ask_even_if_the_request_replays(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    let ask = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&ask, cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-status-ask").is_some());

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::TurnStopped {
                session_id: "session-a".into(),
                reason: jcode_sdk::api::TurnStopReason::Interrupted,
                message: "Cancelled".into(),
                provider_stop_reason: None,
            },
            cx,
        );
        panel.apply(&ask, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-status-stopped").is_some());
    assert!(vcx.debug_bounds("ask-card").is_none());
    assert!(vcx.debug_bounds("composer-text-slot").is_some());
}
