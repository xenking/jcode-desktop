use super::*;

#[gpui::test]
fn transcript_scroll_handler_does_not_keep_closed_panel_alive(cx: &mut gpui::TestAppContext) {
    let panel = cx.update(|cx| {
        cx.new(|cx| {
            Panel::new(
                "lifecycle".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        })
    });
    let weak = panel.downgrade();
    drop(panel);
    cx.run_until_parked();
    assert!(
        weak.upgrade().is_none(),
        "a panel must not own itself through its scroll handler"
    );
}

#[gpui::test]
fn replacing_window_root_releases_rendered_workspace(cx: &mut gpui::TestAppContext) {
    use crate::workspace::Workspace;
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), |_, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
                workspace.push_test_panel("retired-panel", cx);
                workspace
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    let old = window
        .update(cx, |_, _, cx| cx.entity().downgrade())
        .unwrap();
    window
        .update(cx, |_, window, cx| {
            window.replace_root(cx, |_, cx| {
                Workspace::for_test(crate::learning::Coach::new(), cx)
            });
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        old.upgrade().is_none(),
        "hot reload must release the old workspace and its background tasks"
    );
}

#[gpui::test]
fn tab_glows_done_until_seen_and_clears_on_a_new_turn(cx: &mut gpui::TestAppContext) {
    use super::tab_outline::TabMode;
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("tab-focused", cx);
        workspace.push_test_panel("tab-done", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(1))
        .expect("panel exists");
    let status = |status: &str| ApiEvent::SessionStatus {
        session_id: "tab-done".into(),
        status: status.into(),
        pending_soft_interrupts: None,
    };
    let mode = |vcx: &mut gpui::VisualTestContext| {
        panel.read_with(vcx, |panel, _| panel.tab_ring().map(|ring| ring.mode))
    };
    // Panel 1 sits beside the selected panel, out of view.
    panel.update(vcx, |panel, cx| panel.apply(&status("running"), cx));
    assert_eq!(mode(vcx), Some(TabMode::Working));
    panel.update(vcx, |panel, cx| panel.apply(&status("idle"), cx));
    assert_eq!(mode(vcx), Some(TabMode::Done), "finished out of view");
    // A new turn replaces the glow with the working fill.
    panel.update(vcx, |panel, cx| panel.apply(&status("running"), cx));
    assert_eq!(mode(vcx), Some(TabMode::Working));
    panel.update(vcx, |panel, cx| panel.apply(&status("idle"), cx));
    assert_eq!(mode(vcx), Some(TabMode::Done));
    // Viewing the session reads it.
    panel.update(vcx, |panel, cx| panel.set_surface_focused(true, cx));
    assert_eq!(mode(vcx), None);
    // A turn that finishes while watched never glows.
    panel.update(vcx, |panel, cx| {
        panel.set_surface_focused(true, cx);
        panel.apply(&status("running"), cx);
        panel.apply(&status("idle"), cx);
    });
    assert_eq!(mode(vcx), None);
}

#[gpui::test]
fn tab_waits_for_parked_work_and_live_questions(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("waiting-tab", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        use super::tab_outline::TabMode;
        let mode = |panel: &Panel| panel.tab_ring().map(|ring| ring.mode);
        assert_eq!(mode(panel), None);
        panel.set_pending_awaits(vec![jcode_sdk::PendingAwait {
            requested_ids: vec!["worker".into()], deadline_unix_ms: u64::MAX,
        }], cx);
        assert_eq!(mode(panel), Some(TabMode::Waiting));
        panel.set_pending_awaits(Vec::new(), cx);
        assert_eq!(mode(panel), None);

        panel.set_tab_waiting(true, cx);
        panel.set_pending_awaits(vec![jcode_sdk::PendingAwait {
            requested_ids: vec!["removed-worker-1".into(), "removed-worker-2".into()],
            deadline_unix_ms: u64::MAX,
        }], cx);
        assert_eq!(mode(panel), Some(TabMode::Waiting));
        panel.set_tab_waiting(false, cx);
        assert_eq!(mode(panel), Some(TabMode::Waiting));
        panel.set_pending_awaits(Vec::new(), cx);
        assert_eq!(mode(panel), None);
        assert!(panel.awaiting_ids().is_empty());
        panel.set_tab_waiting(true, cx);
        panel.set_pending_awaits(Vec::new(), cx);
        assert_eq!(mode(panel), Some(TabMode::Waiting));
        panel.set_tab_waiting(false, cx);

        for (wire, expected) in [
            (serde_json::json!({"ev":"tool_start", "session_id":"waiting-tab", "call_id":"bg-1", "name":"bg"}), TabMode::Working),
            (serde_json::json!({"ev":"tool_input_delta", "session_id":"waiting-tab", "call_id":"bg-1", "delta":"{\"action\":\"wait\"}"}), TabMode::Waiting),
            (serde_json::json!({"ev":"tool_done", "session_id":"waiting-tab", "call_id":"bg-1", "name":"bg", "output":"Completed"}), TabMode::Working),
        ] {
            panel.apply(&serde_json::from_value(wire).unwrap(), cx);
            assert_eq!(mode(panel), Some(expected));
        }

        let event: ApiEvent = serde_json::from_value(serde_json::json!({
            "ev":"ask_request", "session_id":"waiting-tab", "request_id":"ask-1", "tool_call_id":"tool-1",
            "questions":[{"header":"Choice", "question":"Continue?", "options":[{"label":"Yes"},{"label":"No"}]}]
        })).unwrap();
        panel.apply(&event, cx);
        assert_eq!(mode(panel), Some(TabMode::Question));
    });
}

#[gpui::test]
fn minimap_state_covers_session_lifecycle_and_latest_todo_progress(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("minimap-state", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, _| {
        panel.status = "idle".into();
        panel.items.clear();
        assert_eq!(panel.minimap_state(), MinimapSessionState::Idle);
        assert_eq!(panel.latest_todo_progress(), None);

        panel.status = "running_tools".into();
        assert_eq!(panel.minimap_state(), MinimapSessionState::Working);

        panel.streaming_text = "live token".into();
        assert_eq!(panel.minimap_state(), MinimapSessionState::Streaming);
        panel.streaming_text.clear();
        panel.status = "idle".into();

        panel.items.push(Item::Todos(TodoCardPayload {
            todos: vec![
                TodoCardItem {
                    content: "done".into(),
                    status: "completed".into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
                TodoCardItem {
                    content: "next".into(),
                    status: "pending".into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
            ],
            plan: TodoCardPlan::default(),
            goals: Vec::new(),
        }));
        assert_eq!(panel.latest_todo_progress(), Some((1, 2)));
        assert_eq!(panel.minimap_state(), MinimapSessionState::Idle);

        panel.items.push(Item::Todos(TodoCardPayload {
            todos: vec![TodoCardItem {
                content: "finished".into(),
                status: "completed".into(),
                group: None,
                blocked_by: vec![],
                confidence: None,
                completion_confidence: None,
                stage: None,
                assigned_to: None,
            }],
            plan: TodoCardPlan::default(),
            goals: Vec::new(),
        }));
        assert_eq!(panel.latest_todo_progress(), Some((1, 1)));
        assert_eq!(panel.minimap_state(), MinimapSessionState::Complete);

        panel.items.push(Item::Error("provider failed".into()));
        assert_eq!(panel.minimap_state(), MinimapSessionState::Error);

        // A coarse crash status must win even when no error transcript row
        // was emitted, which covers disconnected or abruptly ended turns.
        panel.items.clear();
        panel.status = "crashed".into();
        assert_eq!(panel.minimap_state(), MinimapSessionState::Error);
    });
}

#[gpui::test]
fn minimap_paints_live_state_and_todo_progress_through_the_workspace_surface(
    cx: &mut gpui::TestAppContext,
) {
    let assert_no_tab_status_dot = |vcx: &mut gpui::VisualTestContext| {
        assert!(vcx.debug_bounds("live-session-tab-0").is_some());
        for selector in [
            "live-session-tab-0-idle",
            "live-session-tab-0-working",
            "live-session-tab-0-streaming",
            "live-session-tab-0-complete",
            "live-session-tab-0-error",
        ] {
            assert!(
                vcx.debug_bounds(selector).is_none(),
                "tabs must not paint the redundant status dot: {selector}"
            );
        }
    };
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.enable_test_minimap();
        workspace.push_test_panel("minimap-render-state", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-idle").is_some(),
        "the public workspace surface must paint the idle state"
    );
    assert_no_tab_status_dot(vcx);

    panel.update(vcx, |panel, cx| {
        panel.status = "running_tools".into();
        cx.notify();
    });
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-working").is_some(),
        "the public workspace surface must paint the working state"
    );
    assert_no_tab_status_dot(vcx);

    panel.update(vcx, |panel, cx| {
        panel.status = "idle".into();
        panel.items.push(Item::Error("visible failure".into()));
        cx.notify();
    });
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-error").is_some(),
        "the public workspace surface must paint the error state"
    );
    assert_no_tab_status_dot(vcx);

    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.status = "streaming".into();
        panel.streaming_text = "visible live response".into();
        cx.notify();
    });
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-streaming").is_some(),
        "the public workspace surface must paint the streaming state"
    );
    assert_no_tab_status_dot(vcx);

    panel.update(vcx, |panel, cx| {
        panel.streaming_text.clear();
        panel.status = "idle".into();
        panel.items.push(Item::Todos(TodoCardPayload {
            todos: vec![
                TodoCardItem {
                    content: "finished".into(),
                    status: "completed".into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
                TodoCardItem {
                    content: "remaining".into(),
                    status: "pending".into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
            ],
            plan: TodoCardPlan::default(),
            goals: Vec::new(),
        }));
        cx.notify();
    });
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-idle").is_some(),
        "a partially complete idle session keeps its idle state color"
    );
    assert_no_tab_status_dot(vcx);
    let partial = vcx
        .debug_bounds("minimap-panel-0-todo-progress")
        .expect("partial todo progress indicator is painted");
    let idle_panel = vcx
        .debug_bounds("minimap-panel-0-idle")
        .expect("idle minimap panel is painted");
    assert!(
        (f32::from(partial.size.width) - f32::from(idle_panel.size.width) / 2.0).abs() < 0.6,
        "one of two completed todos fills half the minimap progress footline"
    );

    panel.update(vcx, |panel, cx| {
        panel.items.push(Item::Todos(TodoCardPayload {
            todos: vec![TodoCardItem {
                content: "finished".into(),
                status: "completed".into(),
                group: None,
                blocked_by: vec![],
                confidence: None,
                completion_confidence: None,
                stage: None,
                assigned_to: None,
            }],
            plan: TodoCardPlan::default(),
            goals: Vec::new(),
        }));
        cx.notify();
    });
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("minimap-panel-0-complete").is_some(),
        "the public workspace surface must paint completed sessions"
    );
    assert_no_tab_status_dot(vcx);
    let progress = vcx
        .debug_bounds("minimap-panel-0-todo-progress")
        .expect("completed todo progress indicator is painted");
    let panel_bounds = vcx
        .debug_bounds("minimap-panel-0-complete")
        .expect("completed minimap panel is painted");
    assert_eq!(
        progress.size.width, panel_bounds.size.width,
        "a fully complete todo set fills the minimap progress footline"
    );
}

#[gpui::test]
fn restored_scroll_is_not_replaced_when_history_reattaches(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    let mut saved = panel.read_with(vcx, |panel, cx| panel.snapshot(cx));
    saved.scroll_y = -137.0;
    saved.stick_to_bottom = false;

    panel.update(vcx, |panel, cx| {
        panel.restore_snapshot(saved, cx);
        panel.load_history(Vec::new(), Vec::new(), cx);
        assert_eq!(f32::from(panel.test_scroll_offset_y()), -137.0);
        assert!(!panel.stick_to_bottom);
    });
}

#[gpui::test]
fn a_few_tall_restored_messages_keep_their_measured_scroll(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, cx| {
        let mut snapshot = panel.snapshot(cx);
        snapshot.scroll_y = -137.0;
        snapshot.stick_to_bottom = false;
        panel.restore_snapshot(snapshot, cx);
        panel.load_history(
            vec![
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "user".into(),
                    content: (0..80)
                        .map(|line| format!("Detailed request line {line}"))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                },
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "assistant".into(),
                    content: (0..80)
                        .map(|line| format!("Detailed response line {line}"))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                },
            ],
            Vec::new(),
            cx,
        );
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    vcx.run_until_parked();

    panel.read_with(vcx, |panel, _| {
        assert!(panel.pending_history_scroll.is_none());
        assert_eq!(f32::from(panel.test_scroll_offset_y()), -137.0);
        assert!(!panel.stick_to_bottom);
    });
}

#[gpui::test]
fn short_restored_history_discards_stale_offscreen_scroll(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, cx| {
        let mut snapshot = panel.snapshot(cx);
        snapshot.scroll_y = -10_000.0;
        snapshot.stick_to_bottom = false;
        panel.restore_snapshot(snapshot, cx);
        panel.load_history(
            vec![
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "user".into(),
                    content: "Fix the rendering".into(),
                },
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "assistant".into(),
                    content: "Fixed.".into(),
                },
            ],
            Vec::new(),
            cx,
        );
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert!(panel.pending_history_scroll.is_none());
        assert_eq!(f32::from(panel.test_scroll_offset_y()), 0.0);
    });
    assert!(
        vcx.debug_bounds("transcript").is_some(),
        "short restored history remains visible after measured clamping"
    );
}

/// Measures frame construction, not GPU presentation, with realistic history
/// sizes. Run explicitly with `transcript_repaint_profile -- --ignored --nocapture`.
#[gpui::test]
#[ignore = "manual transcript repaint profiler"]
fn transcript_repaint_profile(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("profile-transcript", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    for count in [100, 1_000, 10_000] {
        panel.update(vcx, |panel, cx| {
            *panel.items = (0..count)
                .map(|index| {
                    let text = format!("Message {index}: **formatted** text and `inline code`.\n\nA second paragraph for layout.");
                    if index % 2 == 0 {
                        Item::User(text)
                    } else {
                        Item::Assistant(text)
                    }
                })
                .collect();
            cx.notify();
        });
        vcx.run_until_parked();
        let row_started = std::time::Instant::now();
        for _ in 0..1_000 {
            panel.read_with(vcx, |panel, _| {
                std::hint::black_box(panel.transcript_render_rows());
            });
        }
        println!(
            "TRANSCRIPT_ROWS rows={count} descriptor_bytes={} mean_us={:.3}",
            std::mem::size_of::<TranscriptRenderRow>(),
            row_started.elapsed().as_secs_f64() * 1_000.0,
        );
        let mut samples = Vec::new();
        for iteration in 0..60 {
            let started = std::time::Instant::now();
            panel.update(vcx, |_, cx| cx.notify());
            vcx.run_until_parked();
            if iteration >= 10 {
                samples.push(started.elapsed().as_secs_f64() * 1_000.0);
            }
        }
        samples.sort_by(f64::total_cmp);
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.transcript_row_count),
            count
        );
        assert!(
            vcx.debug_bounds("transcript").is_some(),
            "transcript must paint"
        );
        println!(
            "TRANSCRIPT_REPAINT rows={count} p50={:.3}ms p95={:.3}ms",
            samples[25], samples[47]
        );
    }
}

#[gpui::test]
fn restored_alternating_history_still_overflows_and_scrolls(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    // Reload restores the scroll handle before Watch returns history. Paint
    // that empty intermediate state, matching the real asynchronous handoff.
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        let mut snapshot = panel.snapshot(cx);
        snapshot.scroll_y = -137.0;
        snapshot.stick_to_bottom = false;
        panel.restore_snapshot(snapshot, cx);
        cx.notify();
    });
    vcx.run_until_parked();

    panel.update(vcx, |panel, cx| {
        let history = (0..80)
            .map(|index| jcode_sdk::HistoryMessage {
                attachments: Vec::new(),
                response_stats: None,
                timestamp_ms: None,
                tool: None,
                role: if index % 2 == 0 { "user" } else { "assistant" }.into(),
                content: format!("restored message {index}"),
            })
            .collect();
        panel.load_history(history, Vec::new(), cx);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("transcript-scrollbar").is_some(),
        "restored alternating history must produce scrollable overflow"
    );
    let before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    let transcript = vcx.debug_bounds("transcript").expect("transcript painted");
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: transcript.center(),
        // Physical mouse wheels arrive as non-precise line deltas, unlike
        // the precise pixel delta used by a touchpad.
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, 3.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let (during_dispatch, glide_started) = panel.read_with(vcx, |panel, _| {
        (
            panel.test_scroll_offset_y(),
            panel.transcript_wheel_frame.is_some(),
        )
    });
    assert_eq!(during_dispatch, before, "restored history must not jump");
    assert!(glide_started, "restored history starts a momentum glide");
    panel.update(vcx, |panel, cx| {
        panel.transcript_wheel_frame = None;
        assert!(panel.advance_transcript_wheel(cx));
    });
    let after = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    assert_ne!(after, before, "wheel input must move restored history");
}

#[gpui::test]
fn large_transcripts_only_paint_the_visible_tail(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, cx| {
        *panel.items = (0..1_000)
            .map(|index| Item::Assistant(format!("message {index}")))
            .collect();
        panel.stick_to_bottom = true;
        cx.notify();
    });
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("transcript-row-999").is_some(),
        "the newest row should be painted while following the tail"
    );
    assert!(
        vcx.debug_bounds("transcript-row-0").is_none(),
        "offscreen rows must not be painted"
    );
}

#[test]
fn tool_elapsed_labels_are_compact() {
    assert_eq!(format_tool_elapsed(Duration::from_millis(900)), "0s");
    assert_eq!(format_tool_elapsed(Duration::from_secs(42)), "42s");
    assert_eq!(format_tool_elapsed(Duration::from_secs(72)), "1m 12s");
    assert_eq!(format_tool_elapsed(Duration::from_secs(3_780)), "1h 03m");
}

#[test]
fn message_acceptance_promotes_the_oldest_pending_prompt() {
    let now = Instant::now();
    let mut pending = VecDeque::from([4, 7]);
    let mut accepted = HashMap::new();
    acknowledge_next(&mut pending, &mut accepted, now);
    assert_eq!(pending, VecDeque::from([7]));
    assert_eq!(accepted.get(&4), Some(&now));
}

#[test]
fn tool_summary_prefers_intent_over_implementation_details() {
    assert_eq!(
        tool_summary(r#"{"intent":"look","command":"cargo test"}"#),
        "look"
    );
    assert_eq!(tool_summary(r#"{"command":"cargo test"}"#), "cargo test");
    assert_eq!(tool_summary(r#"{"other":1}"#), r#"{"other":1}"#);
}

#[test]
fn condense_flattens_and_clips() {
    assert_eq!(condense("a\n  b\tc", 90), "a b c");
    assert_eq!(condense("abcdef", 3), "abc…");
}

#[test]
fn folder_header_defaults_to_new_session_until_named() {
    let session_id = "01JABCDEF0123456789";
    let fallback = short_id(session_id);

    assert_eq!(
        custom_session_title(session_id, "Plan the release"),
        Some("Plan the release")
    );
    assert_eq!(custom_session_title(session_id, &fallback), None);
    assert_eq!(custom_session_title(session_id, "  "), None);
    assert_eq!(
        folder_session_title(session_id, "Plan the release").as_ref(),
        "Plan the release"
    );
    assert_eq!(
        folder_session_title(session_id, &fallback).as_ref(),
        "New session"
    );
    assert_eq!(
        folder_session_title(session_id, "  ").as_ref(),
        "New session"
    );
    assert_eq!(folder_session_title(session_id, "").as_ref(), "New session");
    assert_eq!(
        folder_session_title("startup://draft/123-0", "New session").as_ref(),
        "New session"
    );
}

#[test]
fn first_prompt_becomes_a_compact_provisional_title() {
    assert_eq!(
        first_prompt_title("  help me   improve the session sidebar\nplease ").as_deref(),
        Some("help me improve the session sidebar please")
    );
    assert_eq!(first_prompt_title(" /model gpt-5.6 "), None);
    assert!(first_prompt_title(&"x".repeat(80)).unwrap().ends_with('…'));
}

#[test]
fn tool_detail_pretty_prints_and_clips() {
    let detail = tool_detail("bash", r#"{"a":1}"#, "line1\nline2");
    assert!(detail.starts_with("a: 1"));
    assert!(detail.contains("\n\nline1\nline2"));
    assert!(detail.contains("line2"));
    // Nothing is dropped from the middle: the painted region scrolls and
    // Copy keeps every line, so the detail is the complete output.
    let long: String = (0..500).map(|n| format!("l{n}\n")).collect();
    let detail = tool_detail("bash", "{}", &long);
    assert!(!detail.contains("Tool call"));
    for n in [0, 250, 499] {
        assert!(detail.contains(&format!("l{n}")), "line {n} is kept");
    }
    assert!(!detail.contains("lines hidden"));
}

#[test]
fn reasoning_tail_keeps_the_newest_words() {
    assert_eq!(condense_tail("short", 200), "short");
    let tail = condense_tail(&"word ".repeat(100), 20);
    assert!(tail.starts_with('…'));
    assert!(tail.ends_with("word"));
}

#[gpui::test]
fn full_live_reasoning_is_preserved_after_it_settles(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, _| {
        panel.items.clear();
        let complete_thought = "a complete train of thought ".repeat(12);
        panel.streaming_reasoning = complete_thought.clone();
        panel.flush_reasoning();

        assert!(panel.streaming_reasoning.is_empty());
        assert!(matches!(panel.items.as_slice(), [Item::Reasoning(text)] if text == &complete_thought));
    });
}

#[test]
fn adjacent_reasoning_segments_share_one_visual_block() {
    let mut items = vec![Item::Reasoning("first thought".into())];
    append_reasoning(&mut items, "second thought".into());

    assert_eq!(items.len(), 1);
    assert!(matches!(
        &items[0],
        Item::Reasoning(text) if text == "first thought\n\nsecond thought"
    ));
}

#[gpui::test]
fn transcript_rows_stay_compact_and_preserve_sources(cx: &mut gpui::TestAppContext) {
    assert!(
        std::mem::size_of::<TranscriptRenderRow>() <= 6 * std::mem::size_of::<usize>(),
        "settled rows must not reserve storage for the largest transcript Item"
    );
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("compact-rows", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |panel, _| {
        *panel.items = vec![
            Item::User("Question".into()),
            Item::Reasoning("First α".into()),
            Item::Todos(TodoCardPayload::default()),
            Item::Reasoning("Second β".into()),
            Item::Reasoning("Third γ".into()),
            Item::Assistant("Answer".into()),
            Item::Reasoning("Separate thought".into()),
        ];
        panel.streaming_reasoning = "Live δ".into();
        panel.streaming_text = "Live answer".into();
        let rows = panel.transcript_render_rows();
        assert_eq!(rows.len(), 6);
        assert!(matches!(rows[0].source, TranscriptRowSource::Settled(0)));
        assert_eq!(rows[0].role, Some("you"));
        assert!(rows[0].show_label);
        assert_eq!(rows[1].index, 1);
        assert!(matches!(&rows[1].source, TranscriptRowSource::Owned(item)
            if matches!(item.as_ref(), Item::Reasoning(text) if text == "First α\n\nSecond β\n\nThird γ")));
        assert!(matches!(rows[2].source, TranscriptRowSource::Settled(5)));
        assert!(rows[2].show_label);
        assert!(matches!(rows[3].source, TranscriptRowSource::Settled(6)));
        assert_eq!(rows[4].index, usize::MAX - 1);
        assert!(matches!(&rows[4].source, TranscriptRowSource::Owned(item)
            if matches!(item.as_ref(), Item::Reasoning(text) if text == "Live δ")));
        assert_eq!(rows[5].index, usize::MAX);
        assert!(matches!(&rows[5].source, TranscriptRowSource::Owned(item)
            if matches!(item.as_ref(), Item::Assistant(text) if text == "Live answer")));
        for index in [1, 3, 4] {
            assert_eq!(rows[index].role, None);
            assert!(!rows[index].show_label);
        }
        assert!(!rows[5].show_label, "same speaker after reasoning stays unlabelled");
        assert!(matches!(&panel.items[1], Item::Reasoning(text) if text == "First α"));
    });
}

#[gpui::test]
fn reasoning_rows_never_get_role_captions(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, _| {
        *panel.items = vec![
            Item::Reasoning("First row".into()),
            Item::User("Question".into()),
            Item::Reasoning("After a user".into()),
            Item::Reasoning("Another restored segment".into()),
            Item::Assistant("Answer".into()),
            Item::Reasoning("After an answer".into()),
        ];
        panel.streaming_reasoning = "Live segment".into();
        let rows = panel.transcript_render_rows();
        let mut thoughts = 0;
        for row in rows {
            let item = match &row.source {
                TranscriptRowSource::Settled(index) => &panel.items[*index],
                TranscriptRowSource::Owned(item) => item,
            };
            if matches!(item, Item::Reasoning(_)) {
                thoughts += 1;
                assert_eq!(row.role, None);
                assert!(!row.show_label, "thinking must never gain a role caption");
            } else {
                assert!(row.show_label, "normal speaker labels are preserved");
            }
        }
        assert_eq!(thoughts, 4);
    });
}

#[gpui::test]
fn inline_reasoning_paints_full_markdown_live_and_settled(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let styles = markdown::capture_painted_font_styles();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let thought = format!(
        "## A quiet heading\n\n{}\n\nThe **final sentence** is still here.",
        "A complete thought, not a truncated preview. ".repeat(8)
    );
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.apply(
            &ApiEvent::ReasoningDelta {
                session_id: "session-a".into(),
                text: thought.clone(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    assert_eq!(
        styles.borrow()["The final sentence is still here."],
        gpui::FontStyle::Italic
    );
    let live_bounds = vcx.debug_bounds("reasoning-inline").unwrap();
    assert!(live_bounds.size.height > px(0.0));
    let live_tail_selector =
        Box::leak(format!("selectable-text-{}-2", usize::MAX - 1).into_boxed_str());
    assert!(vcx.debug_bounds(live_tail_selector).is_some());

    styles.borrow_mut().clear();
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::ReasoningDone {
                session_id: "session-a".into(),
                duration_secs: None,
            },
            cx,
        );
    });
    vcx.run_until_parked();
    assert_eq!(
        styles.borrow()["The final sentence is still here."],
        gpui::FontStyle::Italic
    );
    let settled_bounds = vcx.debug_bounds("reasoning-inline").unwrap();
    assert_eq!(
        live_bounds.size, settled_bounds.size,
        "settling must not collapse thinking"
    );
    let heading = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let last_line = vcx.debug_bounds("selectable-text-0-2").unwrap();
    assert!(
        heading.size.height <= last_line.size.height,
        "thinking headings stay as small as the surrounding reasoning"
    );

    // A quadruple click selects the entire Markdown leaf, not card chrome.
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: last_line.center(),
        modifiers: gpui::Modifiers::default(),
        click_count: 4,
        first_mouse: false,
    });
    panel.update(vcx, |panel, cx| {
        panel
            .transcript_selection
            .update(cx, |selection, cx| selection.copy(cx));
    });
    let copied = vcx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("The final sentence is still here."));
    assert_eq!(
        vcx.debug_bounds("reasoning-inline").unwrap().size,
        settled_bounds.size
    );
}

#[test]
fn adjacent_reasoning_rows_are_coalesced_before_rendering() {
    let rows = vec![
        (4, Item::Reasoning("first thought".into())),
        (5, Item::Reasoning("second thought".into())),
        (usize::MAX - 1, Item::Reasoning("live thought".into())),
    ];

    let grouped = coalesce_reasoning_rows(rows);
    assert_eq!(grouped.len(), 1);
    assert!(matches!(
        &grouped[0],
        (4, Item::Reasoning(text))
            if text == "first thought\n\nsecond thought\n\nlive thought"
    ));
}

#[gpui::test]
fn reasoning_after_streamed_text_starts_a_new_block_below_it(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        let session_id = "session-a".to_string();
        for event in [
            ApiEvent::ReasoningDelta {
                session_id: session_id.clone(),
                text: "first thought".into(),
            },
            ApiEvent::TextDelta {
                session_id: session_id.clone(),
                text: "partial answer".into(),
                message_id: None,
            },
            ApiEvent::ReasoningDelta {
                session_id: session_id.clone(),
                text: "second thought".into(),
            },
        ] {
            panel.apply(&event, cx);
        }
        // While live, the new thought paints after the text, not above it.
        let rows = panel.transcript_render_rows();
        let kinds: Vec<_> = rows
            .iter()
            .map(|row| match &row.source {
                TranscriptRowSource::Settled(index) => &panel.items[*index],
                TranscriptRowSource::Owned(item) => item.as_ref(),
            })
            .map(|item| match item {
                Item::Reasoning(text) => format!("thinking:{text}"),
                Item::Assistant(text) => format!("text:{text}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "thinking:first thought",
                "text:partial answer",
                "thinking:second thought"
            ]
        );

        panel.apply(
            &ApiEvent::ReasoningDone {
                session_id: session_id.clone(),
                duration_secs: None,
            },
            cx,
        );
        assert!(matches!(
            panel.items.as_slice(),
            [Item::Reasoning(a), Item::Assistant(b), Item::Reasoning(c)]
                if a == "first thought" && b == "partial answer" && c == "second thought"
        ));
    });
}

#[test]
fn reasoning_after_another_item_starts_a_new_block() {
    let mut items = vec![Item::Assistant("answer".into())];
    append_reasoning(&mut items, "new thought".into());

    assert_eq!(items.len(), 2);
    assert!(matches!(&items[1], Item::Reasoning(text) if text == "new thought"));
}

#[test]
fn ansi_escapes_are_stripped_from_tool_output() {
    assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
    assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
    assert_eq!(strip_ansi("plain"), "plain");
    assert!(tool_detail("bash", "{}", "\u{1b}[31mred\u{1b}[0m").contains("red"));
    assert!(!tool_detail("bash", "{}", "\u{1b}[31mred\u{1b}[0m").contains('\u{1b}'));
}

fn route(model: &str, api_method: &str) -> jcode_sdk::ModelRouteInfo {
    jcode_sdk::ModelRouteInfo {
        speed_options: Vec::new(),
        reasoning_efforts: None,
        usage: None,
        model: model.into(),
        provider: "openai".into(),
        api_method: api_method.into(),
        available: true,
        detail: String::new(),
    }
}

#[test]
fn auth_method_is_read_from_the_current_models_route() {
    let routes = vec![
        route("gpt-5.6-sol", "openai-oauth"),
        route("claude-fable-5", "anthropic-api-key"),
    ];
    assert_eq!(
        auth_method_for_model(Some("gpt-5.6-sol"), &routes).as_deref(),
        Some("oauth")
    );
    assert_eq!(
        auth_method_for_model(Some("claude-fable-5"), &routes).as_deref(),
        Some("api key")
    );
    // Unknown model or no model: nothing to claim.
    assert_eq!(auth_method_for_model(Some("other"), &routes), None);
    assert_eq!(auth_method_for_model(None, &routes), None);
}

#[test]
fn model_picker_only_offers_available_routes() {
    let mut unavailable = route("luna", "openai-api-key");
    unavailable.available = false;
    let routes = vec![
        unavailable,
        route("gpt-5.6-sol", "openai-api-key"),
        route("gpt-5.6-sol", "openai-oauth"),
        route("claude-fable-5", "anthropic-api-key"),
    ];

    assert_eq!(
        available_model_names(&routes),
        vec!["claude-fable-5", "gpt-5.6-sol"]
    );
}

#[gpui::test]
fn runtime_catalog_only_populates_picker_with_available_routes(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });

    workspace.update(vcx, |workspace, cx| {
        let panel = workspace.test_panel(0).expect("panel exists");
        panel.update(cx, |panel, cx| {
            let mut unavailable = route("luna", "openai-api-key");
            unavailable.available = false;
            panel.apply(
                &ApiEvent::RuntimeInfo {
                    speed_mode: Default::default(),
                    speed_options: Vec::new(),
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6-sol".into()),
                    reasoning_effort: None,
                    auth_method: None,
                    account_label: None,
                    routes: vec![unavailable, route("gpt-5.6-sol", "openai-api-key")],
                },
                cx,
            );
            assert_eq!(
                panel.input.read(cx).command_models(),
                &["openai-api:gpt-5.6-sol"],
                "an unavailable Luna route must not reach the visible picker"
            );
        });
    });
}

#[test]
fn account_label_follows_the_serving_credential_not_the_first_route() {
    // A Claude model lists its API key route before its OAuth route.
    let routes = vec![
        route("claude-opus-5-5", "claude-api"),
        route("claude-opus-5-5", "claude-oauth"),
    ];
    // Without a daemon report, OAuth wins like the daemon's auto rule.
    assert_eq!(
        auth_method_for_model(Some("claude-opus-5-5"), &routes).as_deref(),
        Some("oauth")
    );
    // An unavailable OAuth route does not serve the session.
    let mut no_login = routes.clone();
    no_login[1].available = false;
    assert_eq!(
        auth_method_for_model(Some("claude-opus-5-5"), &no_login).as_deref(),
        Some("api key")
    );
}

#[gpui::test]
fn runtime_info_auth_method_overrides_route_guessing(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });

    workspace.update(vcx, |workspace, cx| {
        let panel = workspace.test_panel(0).expect("panel exists");
        panel.update(cx, |panel, cx| {
            panel.apply(
                &ApiEvent::RuntimeInfo {
                    speed_mode: Default::default(),
                    speed_options: Vec::new(),
                    session_id: "session-a".into(),
                    provider: Some("Claude".into()),
                    model: Some("claude-opus-5-5".into()),
                    reasoning_effort: None,
                    auth_method: Some("api_key".into()),
                    account_label: Some("initial".into()),
                    routes: vec![
                        route("claude-opus-5-5", "claude-api"),
                        route("claude-opus-5-5", "claude-oauth"),
                    ],
                },
                cx,
            );
            assert_eq!(panel.auth_method.as_deref(), Some("api key"));
            assert_eq!(panel.account_label.as_deref(), Some("initial"));
            // A same-model credential switch reported by the daemon.
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("Claude".into()),
                    model: Some("claude-opus-5-5".into()),
                    reasoning_effort: None,
                    auth_method: Some("oauth".into()),
                    account_label: Some("work".into()),
                },
                cx,
            );
            assert_eq!(panel.auth_method.as_deref(), Some("oauth"));
            assert_eq!(panel.account_label.as_deref(), Some("work"));
            // Automatic failover can change the account without changing model or auth.
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("Claude".into()),
                    model: Some("claude-opus-5-5".into()),
                    reasoning_effort: None,
                    auth_method: Some("oauth".into()),
                    account_label: Some("personal".into()),
                },
                cx,
            );
            assert_eq!(panel.account_label.as_deref(), Some("personal"));
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("Claude".into()),
                    model: Some("claude-opus-5-5".into()),
                    reasoning_effort: None,
                    auth_method: Some("api_key".into()),
                    account_label: None,
                },
                cx,
            );
            assert_eq!(panel.account_label, None);
        });
    });
}

#[test]
fn compact_directory_marks_home_but_not_its_children() {
    let home = std::env::var("HOME").expect("test home");
    assert_eq!(compact_dir(&home), "~ (home)");
    assert_eq!(compact_dir(&format!("{home}/project")), "~/project");
    assert_eq!(
        compact_dir(&format!("{home}-other")),
        format!("{home}-other")
    );
}

#[test]
fn serving_account_labels_identify_the_actual_login() {
    assert_eq!(
        serving_account_label(Some("openai"), Some("oauth"), Some("work")),
        "OpenAI · OAuth · work"
    );
    assert_eq!(
        serving_account_label(Some("claude"), Some("oauth"), Some("personal@example.com")),
        "Anthropic · OAuth · personal@example.com"
    );
    assert_eq!(
        serving_account_label(Some("openai"), Some("oauth"), Some("  ")),
        "OpenAI · OAuth"
    );
    assert_eq!(
        serving_account_label(Some("openai"), Some("api_key"), None),
        "OpenAI · API key"
    );
}

#[test]
fn footer_labels_keep_model_account_and_context_separate() {
    assert_eq!(
        account_method_label(Some("openai"), Some("oauth")),
        "OpenAI · OAuth"
    );
    assert_eq!(
        account_method_label(Some("anthropic"), Some("api key")),
        "Anthropic · API key"
    );
    assert_eq!(
        account_method_label(Some("Anthropic API"), Some("api key")),
        "Anthropic · API key"
    );
    assert_eq!(
        account_method_label(Some("claude-api"), Some("api key")),
        "Anthropic · API key"
    );
    assert_eq!(account_method_label(Some("OpenAI API"), None), "OpenAI");
    assert_eq!(
        account_method_label(Some("My Local Router"), Some("custom-acp")),
        "My Local Router · custom-acp"
    );
    assert_eq!(account_method_label(None, Some("oauth")), "OAuth");
    assert_eq!(account_method_label(Some("openai"), None), "OpenAI");
    assert_eq!(account_method_label(None, None), "Accounts");
    assert_eq!(account_method_label(Some(""), Some("")), "Accounts");
}

#[test]
fn context_windows_match_the_families_jcode_uses() {
    assert_eq!(context_window_for_model("gpt-5.6-sol"), Some(272_000));
    assert_eq!(context_window_for_model("gpt-5.4-alto"), Some(1_000_000));
    assert_eq!(
        context_window_for_model("gpt-5.2-chat-latest"),
        Some(128_000)
    );
    assert_eq!(
        context_window_for_model("claude-sonnet-4-20250514"),
        Some(200_000)
    );
    assert_eq!(context_window_for_model("gemini-3-pro"), Some(1_000_000));
    assert_eq!(context_window_for_model("mystery"), None);
}

#[test]
fn token_counts_format_compactly() {
    assert_eq!(format_tokens(950), "950");
    assert_eq!(format_tokens(12_500), "12.5k");
    assert_eq!(format_tokens(1_048_576), "1.0m");
}

#[test]
fn discrete_wheel_glide_eases_and_preserves_the_full_distance() {
    let mut glide = WheelGlide::default();
    glide.push(120.0);
    let first = glide
        .take_step(Duration::from_millis(16))
        .expect("wheel input starts a glide");
    assert!(first > 0.0 && first < 120.0);

    let mut traveled = first;
    while let Some(step) = glide.take_step(Duration::from_millis(16)) {
        traveled += step;
    }
    assert!((traveled - 120.0).abs() <= WheelGlide::SETTLE);
}

#[test]
fn reversing_the_wheel_cancels_old_direction_momentum() {
    let mut glide = WheelGlide::default();
    glide.push(120.0);
    let _ = glide.take_step(Duration::from_millis(16));
    glide.push(-40.0);
    assert!(
        glide
            .take_step(Duration::from_millis(16))
            .is_some_and(|step| step < 0.0)
    );
}

/// The acceptance path for the scroll-lock fix: a real wheel event over a
/// real painted transcript releases stick-to-bottom, the "↓ latest" chip
/// paints, and clicking the chip re-engages following. Before the fix the
/// panel yanked the view back down on every streamed event.
#[gpui::test]
fn scrolling_up_releases_follow_mode_and_the_chip_restores_it(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    // A long transcript, so the scroll region actually overflows.
    panel.update(vcx, |panel, cx| {
        for n in 0..80 {
            panel.items.push(Item::Assistant(format!("message {n}")));
        }
        cx.notify();
    });
    vcx.run_until_parked();
    // Scroll metrics are produced during layout. Repaint once with those
    // measured metrics, as the running app does on its next frame.
    panel.update(vcx, |_panel, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        panel.read_with(vcx, |panel, _| panel.stick_to_bottom),
        "a fresh panel follows the stream"
    );
    assert!(
        vcx.debug_bounds("jump-to-latest").is_none(),
        "no chip while following"
    );
    let scrollbar_before = vcx
        .debug_bounds("transcript-scrollbar")
        .expect("an overflowing transcript paints a scrollbar");
    assert_eq!(scrollbar_before.size.width, px(3.0));
    assert!(scrollbar_before.size.height >= px(28.0));

    // A real discrete upward wheel event over the transcript. Unlike a
    // touchpad pixel delta, it must be captured and animated rather than
    // moving the list by the full notch distance in this dispatch.
    let transcript = vcx.debug_bounds("transcript").expect("transcript painted");
    let offset_before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: transcript.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, 3.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    let (offset_during_dispatch, glide_started) = panel.read_with(vcx, |panel, _| {
        (
            panel.test_scroll_offset_y(),
            panel.transcript_wheel_frame.is_some(),
        )
    });
    assert_eq!(
        offset_during_dispatch, offset_before,
        "a wheel notch is eased instead of jumping immediately"
    );
    assert!(
        glide_started,
        "the painted transcript starts a momentum glide"
    );
    panel.update(vcx, |panel, cx| {
        panel.transcript_wheel_frame = None;
        assert!(panel.advance_transcript_wheel(cx));
    });
    let offset_after_frame = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    assert_ne!(
        offset_after_frame, offset_before,
        "one eased frame moves the real transcript list state"
    );
    vcx.run_until_parked();
    panel.update(vcx, |_panel, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        !panel.read_with(vcx, |panel, _| panel.stick_to_bottom),
        "scrolling up must release follow mode"
    );
    let scrollbar_after = vcx
        .debug_bounds("transcript-scrollbar")
        .expect("the scrollbar remains visible after scrolling");
    assert_eq!(scrollbar_after.size.width, px(3.0));
    let chip = vcx
        .debug_bounds("jump-to-latest")
        .expect("the catch-up chip paints once detached");
    assert!(chip.size.width > px(0.) && chip.size.height > px(0.));

    // While detached, streamed events must not yank the view back down.
    let offset_before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::TextDelta {
                message_id: None,
                session_id: "session-a".into(),
                text: "more streamed text".into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let offset_after = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    assert_eq!(
        offset_before, offset_after,
        "streaming must not move a detached viewport"
    );

    // Clicking the chip re-engages following and removes the chip.
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        panel.read_with(vcx, |panel, _| panel.stick_to_bottom),
        "the chip must restore follow mode"
    );
    assert!(
        vcx.debug_bounds("jump-to-latest").is_none(),
        "the chip disappears once following again"
    );
}

/// The acceptance path for the code-copy affordance: the button paints in
/// a real assistant message and a real click puts the code body (not the
/// fence syntax) on the clipboard.
#[gpui::test]
fn clicking_copy_on_a_code_block_fills_the_clipboard(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    panel.update(vcx, |panel, cx| {
        panel.items.push(Item::Assistant(
            "```rust\nfn main() {\n    println!(\"hi\");\n}\n```".into(),
        ));
        cx.notify();
    });
    vcx.run_until_parked();

    let button = vcx
        .debug_bounds("code-copy")
        .expect("the copy button paints on a fenced block");
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    let copied = vcx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .expect("clicking copy fills the clipboard");
    assert_eq!(copied, "fn main() {\n    println!(\"hi\");\n}");
}

#[gpui::test]
fn swarm_worker_notices_remain_compact_transcript_rows(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("swarm-notices".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        let initial = panel.items.len();
        let notify = |message: &str| ApiEvent::SwarmWorkerNotice {
            session_id: "swarm-notices".into(),
            message: message.into(),
        };
        panel.apply(&notify("worker: model set to test · medium"), cx);
        panel.apply(&notify("worker: context compaction started"), cx);
        assert!(matches!(&panel.items[initial..], [Item::SwarmWorkerNotice(model), Item::SwarmWorkerNotice(compact)]
            if model == "worker: model set to test · medium" && compact == "worker: context compaction started"));
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("swarm-worker-notice").is_some());
}

#[gpui::test]
fn setting_changes_reload_from_history_as_notice_rows(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("setting-history".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        let history: jcode_sdk::HistoryMessage = serde_json::from_value(serde_json::json!({
            "role": "setting_change", "content": "Model: a → b (you chose it)", "timestamp_ms": 10
        }))
        .unwrap();
        panel.load_history(vec![history], vec![], cx);
        assert!(panel.items.iter().any(|item| matches!(item, Item::SwarmWorkerNotice(text)
            if text == "Model: a → b (you chose it)")));
    });
}

#[gpui::test]
fn queued_compaction_settles_one_row_with_result_or_error(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        let session_id = "session-a".to_owned();
        let queued = ApiEvent::Compacted {
            session_id: session_id.clone(),
            message: "Context compaction queued.".into(),
        };
        let divider = |content: &str, running| ApiEvent::CompactionDivider {
            session_id: session_id.clone(),
            content: content.into(),
            running,
            method: None,
            trigger: Some("manual".into()),
            pre_tokens: None,
            post_tokens: None,
        };
        let initial = panel.items.len();
        panel.apply(&queued, cx);
        assert!(matches!(&panel.items[initial..], [Item::Compaction(row)] if row.state.is_running() && row.label() == "Context compaction queued."));
        panel.apply(&divider("compacting… · manual", true), cx);
        assert!(matches!(&panel.items[initial..], [Item::Compaction(row)] if row.state.is_running() && row.label() == "compacting… · manual"));
        panel.apply(&divider("compacted · 10k→4k", false), cx);
        assert!(matches!(&panel.items[initial..], [Item::Compaction(row)] if !row.state.is_running() && row.label() == "compacted · 10k→4k"));

        panel.apply(&queued, cx);
        panel.apply(&divider("compacting… · manual", true), cx);
        panel.apply(&ApiEvent::BackgroundProgress {
            session_id,
            task_id: "context-compaction".into(),
            label: "Context compaction".into(),
            percent: None,
            summary: "Compaction failed: provider unavailable".into(),
            detail: None,
            done: true,
        }, cx);
        assert!(matches!(&panel.items[initial..], [Item::Compaction(row), Item::Error(error)] if !row.state.is_running() && error == "Compaction failed: provider unavailable"));
    });
}

/// Progress reuses a quiet row, then finishes as a compact result card.
#[gpui::test]
fn background_progress_updates_one_native_card(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    for (percent, summary, done) in [
        (Some(35.0), "35% · Running tests", false),
        (
            None,
            "Compiling dependencies and waiting for the linker to finish a very long build\nAdditional output stays in the tooltip",
            false,
        ),
        (None, "✓ completed · 8.2s · exit 0", true),
    ] {
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::BackgroundProgress {
                    session_id: "session-a".into(),
                    task_id: "task-42".into(),
                    label: "Workspace tests".into(),
                    percent,
                    summary: summary.into(),
                    detail: None,
                    done,
                },
                cx,
            );
        });
        vcx.run_until_parked();
        let row = vcx
            .debug_bounds("background-task-card")
            .expect("task row paints");
        if done {
            assert!(
                row.size.height > px(0.0) && row.size.height <= px(40.0),
                "finished tasks render as one compact row: {row:?}"
            );
        } else {
            assert!(
                row.size.height <= px(28.0),
                "running tasks stay one compact row: {row:?}"
            );
        }
    }

    assert!(vcx.debug_bounds("background-task-card").is_some());
    panel.read_with(vcx, |panel, _| {
        let tasks: Vec<_> = panel
            .items
            .iter()
            .filter_map(|item| match item {
                Item::BackgroundTask {
                    task_id,
                    summary,
                    percent,
                    done,
                    ..
                } => Some((task_id, summary, percent, done)),
                _ => None,
            })
            .collect();
        assert_eq!(tasks.len(), 1, "progress ticks update rather than append");
        assert_eq!(tasks[0].0, "task-42");
        assert_eq!(tasks[0].1, "✓ completed · 8.2s · exit 0");
        assert_eq!(*tasks[0].2, None);
        assert!(*tasks[0].3);
    });
}

#[test]
fn tool_token_badge_estimates_output_and_colors_severity_boundaries() {
    let theme = Theme::global();
    for (tokens, label, color) in [
        (0, "~0 tok", theme.OK),
        (1_900, "~1.9k tok", theme.OK),
        (3_999, "~3.9k tok", theme.OK),
        (4_000, "~4k tok", theme.WARN),
        (11_999, "~11k tok", theme.WARN),
        (12_000, "~12k tok", theme.ERROR),
    ] {
        assert_eq!(
            tool_output_token_badge(&"x".repeat(tokens * 4)),
            (label.into(), color)
        );
    }
    assert_eq!(tool_output_token_badge("x").0, "~1 tok");
    assert_eq!(
        tool_output_token_badge(&"xxx\n".repeat(1_900)).0,
        "~1.9k tok"
    );
}

#[test]
fn tool_token_badge_reports_the_original_size_of_trimmed_mcp_results() {
    let output = format!(
        "{}\n\n[MCP result trimmed from 46210 to 7980 tokens to fit the context budget. \
         Pass accept_large_output: true for the full result, or narrow the request.]",
        "x".repeat(31_900)
    );
    let (label, _) = tool_output_token_badge(&output);
    assert_eq!(label, "~8k tok of 46k");
    assert_eq!(trimmed_mcp_original_tokens("plain output"), None);
}

fn settle_tool_details(panel: &Entity<Panel>, cx: &mut gpui::VisualTestContext) {
    panel.update(cx, |panel, cx| {
        for motion in panel.tool_detail_motion.values_mut() {
            motion.sample(Instant::now() + Duration::from_secs(1));
        }
        cx.notify();
    });
    cx.run_until_parked();
}

#[path = "panel_more_tests.rs"]
mod more;
