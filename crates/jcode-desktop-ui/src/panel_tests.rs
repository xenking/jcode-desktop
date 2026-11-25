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
    assert_eq!(human_auth_method("oauth"), "oauth");
    assert_eq!(human_auth_method("api_key"), "api key");
    assert_eq!(human_auth_method("claude-api"), "api key");
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

#[gpui::test]
fn tool_token_badge_paints_for_empty_and_single_line_results(cx: &mut gpui::TestAppContext) {
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
    for (done, output, failed) in [
        (false, "", false),
        (true, "", false),
        (true, "ok", false),
        (true, "failed", true),
        (false, "failed", true),
    ] {
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Tool {
                call_id: "token-visibility".into(),
                name: "bash".into(),
                input: r#"{"command":"true"}"#.into(),
                output: output.into(),
                done,
                error: failed.then(|| "Tool failed".into()),
            }];
            cx.notify();
        });
        vcx.run_until_parked();
        let button = vcx
            .debug_bounds("tool-output-size")
            .expect("running and finished tools expose their output toggle");
        assert!(
            vcx.debug_bounds("tool-name").is_some(),
            "tool names persist after completion"
        );
        assert_eq!(
            vcx.debug_bounds("tool-row-running").is_some(),
            !done && !failed
        );
        for selector in ["tool-identity-pill", "tool-summary"] {
            let text = vcx.debug_bounds(selector).expect("tool label paints");
            assert_eq!(
                text.size.height, button.size.height,
                "{selector} fills the pill height"
            );
            assert_eq!(
                text.origin.y, button.origin.y,
                "{selector} aligns with the pill"
            );
        }
        let pill = vcx.debug_bounds("tool-identity-pill").unwrap();
        let name = vcx.debug_bounds("tool-name").unwrap();
        let glyph = vcx.debug_bounds("tool-type-icon").unwrap();
        assert!(pill.left() < glyph.left() && name.right() < pill.right());
        assert!(glyph.right() < name.left());
        assert!((f32::from(glyph.center().y - name.center().y)).abs() < 1.0);
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("tool-detail").is_some(),
            "identity pill expands the row"
        );
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        settle_tool_details(&panel, vcx);
        let status_selector = if failed {
            "tool-icon-failed"
        } else if done {
            "tool-icon-succeeded"
        } else {
            "tool-icon-running"
        };
        assert!(vcx.debug_bounds(status_selector).is_some());
        let icon = vcx
            .debug_bounds("tool-type-icon")
            .expect("tool icon paints");
        let name = vcx.debug_bounds("tool-summary").unwrap();
        assert_eq!(icon.size, gpui::size(px(14.0), px(14.0)));
        assert!(icon.right() <= name.left(), "icon precedes the intent");
        assert!((f32::from(icon.center().y - name.center().y)).abs() < 1.0);
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-detail").is_some());
        settle_tool_details(&panel, vcx);
        let button = vcx.debug_bounds("tool-output-size").unwrap();
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        settle_tool_details(&panel, vcx);
        assert!(vcx.debug_bounds("tool-detail").is_none());
    }
}

#[gpui::test]
fn whole_tool_row_toggles_the_tool_output_card(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    // Keep the long expanded card and its header inside the viewport.
    // Otherwise bottom anchoring can clip the header behind workspace chrome,
    // and a debug-bounds click hits the chrome rather than the tool row.
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(1200.), px(1400.)));
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    // A finished call with long, ANSI-colored output: the shapes the
    // clipping and stripping paths exist for.
    let output: String = (0..60)
        .map(|n| format!("\u{1b}[32mline {n}\u{1b}[0m\n"))
        .collect();
    panel.update(vcx, |panel, cx| {
        panel.items.push(Item::Tool {
            call_id: "call-1".into(),
            name: "bash".into(),
            input: r#"{"command":"make build","intent":"compile"}"#.into(),
            output,
            done: true,
            error: None,
        });
        cx.notify();
    });
    vcx.run_until_parked();

    // Collapsed: the size hint paints, the detail does not.
    let hint = vcx
        .debug_bounds("tool-output-size")
        .expect("a collapsed finished call shows its output size");
    assert!(hint.size.width > px(0.));
    assert!(
        vcx.debug_bounds("tool-detail").is_none(),
        "detail stays hidden until expanded"
    );

    // The intent, like the rest of the header, expands the call.
    let name = vcx.debug_bounds("tool-summary").unwrap();
    vcx.simulate_click(name.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tool-detail").is_some());
    settle_tool_details(&panel, vcx);
    let detail = vcx
        .debug_bounds("tool-detail")
        .expect("clicking the token button expands the output card");
    assert!(detail.size.height > px(0.));
    let header = vcx.debug_bounds("tool-header").unwrap();
    assert_eq!(detail.left(), header.left() + px(24.));
    assert!(detail.top() >= header.bottom() + px(4.));
    assert!(
        vcx.debug_bounds("tool-output-size").is_some(),
        "token cost remains visible alongside the expanded detail"
    );
    // The rendered detail is the formatted string: ANSI-free, head and
    // tail kept around the fold marker.
    let rendered = panel.read_with(vcx, |panel, _| match &panel.items[0] {
        Item::Tool {
            name,
            input,
            output,
            ..
        } => tool_detail(name, input, output),
        other => panic!("expected the tool row, got {other:?}"),
    });
    assert!(!rendered.contains('\u{1b}'), "detail must be ANSI-free");
    assert!(rendered.contains("command:"));
    assert!(rendered.contains("command: make build"));
    assert!(rendered.contains("line 0") && rendered.contains("line 59"));
    // The complete output is reachable now, not a 40-line window around a
    // `lines hidden` marker.
    assert!(!rendered.contains("lines hidden"));

    // Clicking the body does not collapse the call or prevent selection.
    vcx.simulate_click(detail.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tool-detail").is_some());

    // Only a second click on the token button collapses it again.
    let button = vcx.debug_bounds("tool-output-size").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("tool-detail").is_some(),
        "closing card remains mounted during exit"
    );
    panel.read_with(vcx, |panel, _| {
        assert!(!panel.expanded_regions.contains("call-1"));
        assert!(panel.tool_detail_motion.contains_key("call-1"));
    });
    // Reverse a closing card without unmounting it or jumping back to zero.
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tool-detail").is_some());
    settle_tool_details(&panel, vcx);
    assert!(panel.read_with(vcx, |panel, _| panel.expanded_regions.contains("call-1")));
    let button = vcx.debug_bounds("tool-output-size").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    settle_tool_details(&panel, vcx);
    assert!(
        vcx.debug_bounds("tool-detail").is_none(),
        "the exit unmounts after its final frame"
    );

    vcx.update(|_, cx| cx.set_reduce_motion(true));
    let button = vcx.debug_bounds("tool-output-size").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tool-detail").is_some());
    assert!(panel.read_with(vcx, |panel, _| panel.tool_detail_motion.is_empty()));
    let button = vcx.debug_bounds("tool-output-size").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tool-detail").is_none());
}

#[gpui::test]
fn inline_tool_rows_keep_status_errors_and_edit_previews(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "inline-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    for (name, input, error, preview) in [
        ("bash", r#"{"command":"cargo test"}"#, None, false),
        (
            "bash",
            r#"{"command":"cargo test"}"#,
            Some("build failed"),
            false,
        ),
        (
            "edit",
            r#"{"intent":"Clarify navigation","file_path":"src/main.rs","old_string":"old","new_string":"new"}"#,
            None,
            true,
        ),
    ] {
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Tool {
                call_id: "inline-call".into(),
                name: name.into(),
                input: input.into(),
                output: String::new(),
                done: true,
                error: error.map(str::to_owned),
            }];
            panel.expanded_regions.clear();
            panel.tool_detail_motion.clear();
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-header").is_some());
        assert!(vcx.debug_bounds("tool-detail").is_none());
        assert!(vcx.debug_bounds("code-edit-preview").is_none());
        let row = vcx.debug_bounds("tool-header").unwrap();
        vcx.simulate_click(row.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-detail").is_some());
        assert_eq!(vcx.debug_bounds("code-edit-preview").is_some(), preview);
    }
}

/// Legacy harness input deltas have no call id. Exercise the real event and
/// paint path so an intent cannot silently disappear between transport and
/// the visible tool header again.
#[gpui::test]
fn legacy_tool_input_renders_its_intent_in_the_tool_header(cx: &mut gpui::TestAppContext) {
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
        panel.apply(
            &ApiEvent::ToolStart {
                session_id: "session-a".into(),
                call_id: "call-1".into(),
                name: "bash".into(),
            },
            cx,
        );
        panel.apply(
            &ApiEvent::ToolInputDelta {
                session_id: "session-a".into(),
                call_id: String::new(),
                delta: r#"{"intent":"check the build","command":"cargo test"}"#.into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();

    let summary = panel.read_with(vcx, |panel, _| match &panel.items[0] {
        Item::Tool { input, .. } => tool_summary(input),
        other => panic!("expected the tool row, got {other:?}"),
    });
    assert_eq!(summary, "check the build");
    let rendered = vcx
        .debug_bounds("tool-summary")
        .expect("the intent summary paints in the tool header");
    assert!(rendered.size.width > px(0.) && rendered.size.height > px(0.));
}

#[gpui::test]
fn consecutive_tool_rows_use_compact_spacing(cx: &mut gpui::TestAppContext) {
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
        *panel.items = (0..2)
            .map(|index| Item::Tool {
                call_id: format!("call-{index}"),
                name: "bash".into(),
                input: r#"{"command":"echo hello"}"#.into(),
                output: "hello".into(),
                done: true,
                error: None,
            })
            .collect();
        cx.notify();
    });
    vcx.run_until_parked();
    let first = vcx
        .debug_bounds("transcript-row-0")
        .expect("first tool row");
    let second = vcx
        .debug_bounds("transcript-row-1")
        .expect("second tool row");
    assert_eq!(
        first.size.height - second.size.height,
        px(8.0),
        "adjacent tools use 2px padding instead of the 10px message boundary"
    );
    let header = vcx.debug_bounds("tool-header").expect("tool header");
    assert!(header.size.height < px(24.0), "tool header stays compact");
}

/// Inline tools must retain their intrinsic row height when enough of them
/// are appended to make the transcript scroll. Without `flex_none`, the
/// transcript's flex layout distributes the height deficit across every
/// row and visibly squashes their headers.
#[gpui::test]
fn tool_rows_do_not_shrink_when_the_transcript_overflows(cx: &mut gpui::TestAppContext) {
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
        *panel.items = vec![Item::Tool {
            call_id: "call-0".into(),
            name: "bash".into(),
            input: r#"{"command":"echo 0"}"#.into(),
            output: "done".into(),
            done: true,
            error: None,
        }];
        cx.notify();
    });
    vcx.run_until_parked();
    let baseline_height = vcx
        .debug_bounds("tool-inline")
        .expect("an inline tool paints before the transcript overflows")
        .size
        .height;

    panel.update(vcx, |panel, cx| {
        panel.items.extend((1..30).map(|index| Item::Tool {
            call_id: format!("call-{index}"),
            name: "bash".into(),
            input: format!(r#"{{"command":"echo {index}"}}"#),
            output: "done".into(),
            done: true,
            error: None,
        }));
        cx.notify();
    });
    vcx.run_until_parked();

    let overflowing_height = vcx
        .debug_bounds("tool-inline")
        .expect("an overflowing transcript still paints an inline tool")
        .size
        .height;
    assert_eq!(
        overflowing_height, baseline_height,
        "overflow changed tool-row height from {baseline_height:?} to {overflowing_height:?}"
    );
}

/// The review fixture itself must paint: every transcript shape the demo
/// seeds (user, reasoning, running and finished tools with ANSI output,
/// full markdown, error) renders together in one painted window without
/// panicking, and the signature regions all occupy space.
#[gpui::test]
fn the_demo_transcript_paints_every_item_shape(cx: &mut gpui::TestAppContext) {
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

    // The same items JCODE_DESKTOP_DEMO_TRANSCRIPT=1 seeds, minus the
    // env-var gate so the test is hermetic.
    panel.update(vcx, |panel, cx| {
        *panel.items = super::fixtures::demo_item_fixtures();
        assert!(panel.items.len() >= 6, "demo covers every item shape");
        cx.notify();
    });
    vcx.run_until_parked();

    let selectors = ["tool-header", "tool-output-size", "md-quote", "code-copy"];
    let mut painted = std::collections::HashSet::new();
    let row_count = panel.read_with(vcx, |panel, _| panel.transcript_list.item_count());
    // A virtualized transcript intentionally omits off-screen rows. Check
    // the initial tail, then reveal each item rather than requiring every
    // shape to fit simultaneously inside the panel's usable viewport.
    for row in 0..=row_count {
        if row > 0 {
            panel.update(vcx, |panel, cx| {
                panel.stick_to_bottom = false;
                panel.transcript_list.scroll_to(gpui::ListOffset {
                    item_ix: row - 1,
                    offset_in_item: px(0.),
                });
                cx.notify();
            });
            vcx.run_until_parked();
        }
        for selector in selectors {
            if let Some(bounds) = vcx.debug_bounds(selector) {
                assert!(
                    bounds.size.width > px(0.) && bounds.size.height > px(0.),
                    "{selector} must occupy space"
                );
                painted.insert(selector);
            }
        }
    }
    for selector in selectors {
        assert!(
            painted.contains(selector),
            "{selector} should paint in the demo"
        );
    }
}

/// Blockquotes paint as a distinct region in a real assistant message.
#[gpui::test]
fn blockquotes_paint_in_a_real_transcript(cx: &mut gpui::TestAppContext) {
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
        panel
            .items
            .push(Item::Assistant("> a quoted line\n> and another".into()));
        cx.notify();
    });
    vcx.run_until_parked();
    let quote = vcx
        .debug_bounds("md-quote")
        .expect("the quote region paints");
    assert!(quote.size.width > px(0.) && quote.size.height > px(0.));
}

#[gpui::test]
fn working_directory_does_not_add_a_chat_header(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "directory-label".into(),
            None,
            Some("/srv/projects/jcode-desktop".into()),
            crate::harness::spawn_inert(),
            cx,
        )
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [320., 800.] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
        for dir in [
            Some("/srv/projects/jcode-desktop"),
            Some("/"),
            None,
            Some(""),
        ] {
            panel.update(vcx, |panel, cx| {
                panel.working_dir = dir.map(str::to_owned);
                cx.notify();
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("panel-working-directory").is_none());
            assert!(vcx.debug_bounds("panel-directory-name").is_none());
            assert!(vcx.debug_bounds("panel-directory-path").is_none());
            assert!(vcx.debug_bounds("fresh-session").is_some());
            assert!(vcx.debug_bounds("panel-meta").is_some());
        }
    }
}

/// The acceptance path: real events land in a real panel inside a painted
/// window, and the footer element occupies space on screen. Without this,
/// the tests above only prove the string is right, not that anyone sees it.
#[gpui::test]
fn the_identity_footer_paints_after_runtime_info_and_token_events(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        let _ = window;
        workspace
    });
    vcx.run_until_parked();

    // The footer owns stable layout space even before runtime identity
    // arrives, so the composer does not jump when attach completes.
    assert!(
        vcx.debug_bounds("panel-meta").is_some(),
        "identity footer reserves layout space before metadata arrives"
    );

    // The events the harness worker forwards after attach and during a turn.
    workspace.update(vcx, |workspace, cx| {
        let panel = workspace.test_panel(0).expect("panel exists");
        panel.update(cx, |panel, cx| {
            panel.status = "connected".into();
            assert_eq!(panel.status_line(), "Ready");
            panel.status = "running_tools".into();
            assert_eq!(panel.status_line(), "Running tools");
            panel.connection_phase = "connected".into();
            assert_eq!(panel.status_line(), "Running tools");
            panel.connection_phase = "Retrying request".into();
            assert_eq!(panel.status_line(), "Retrying request");
            panel.connection_phase.clear();
            panel.status = "idle".into();
            assert_eq!(panel.status_line(), "Ready");
            panel.apply(
                &ApiEvent::RuntimeInfo {
                    speed_mode: Default::default(),
                    speed_options: Vec::new(),
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6-sol".into()),
                    auth_method: None,
                    account_label: None,
                    routes: vec![jcode_sdk::ModelRouteInfo {
                        speed_options: Vec::new(),
                        reasoning_efforts: None,
                        usage: None,
                        model: "gpt-5.6-sol".into(),
                        provider: "openai".into(),
                        api_method: "openai-oauth".into(),
                        available: true,
                        detail: String::new(),
                    }],
                    reasoning_effort: None,
                },
                cx,
            );
            panel.apply(
                &ApiEvent::TokenUsage {
                    session_id: "session-a".into(),
                    input: 100_000,
                    output: 8_000,
                    cache_read_input: Some(28_000),
                    cache_creation_input: None,
                    effective_budget: None,
                },
                cx,
            );
            assert_eq!(panel.model.as_deref(), Some("gpt-5.6-sol"));
            assert_eq!(panel.provider.as_deref(), Some("openai"));
            assert_eq!(panel.auth_method.as_deref(), Some("oauth"));

            // Repeating the current model must not wipe the still-correct
            // auth label. A real switch to another model invalidates it.
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6-sol".into()),
                    reasoning_effort: None,
                    auth_method: None,
                    account_label: None,
                },
                cx,
            );
            assert_eq!(
                panel.auth_method.as_deref(),
                Some("oauth"),
                "same-model broadcast must not clear the auth label"
            );
            panel.apply(
                &ApiEvent::ModelInfo {
                    session_id: "session-a".into(),
                    provider: Some("anthropic".into()),
                    model: Some("claude-fable-5".into()),
                    reasoning_effort: None,
                    auth_method: None,
                    account_label: None,
                },
                cx,
            );
            assert_eq!(
                panel.auth_method, None,
                "a real model switch invalidates the auth label"
            );
        });
    });
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("panel-meta")
        .expect("the identity footer should have painted");
    assert!(
        bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.),
        "the footer must occupy real space, got {bounds:?}"
    );
    assert!(
        vcx.debug_bounds("panel-build").is_none(),
        "build metadata belongs in the workspace header, not each panel"
    );
    let identity = vcx
        .debug_bounds("panel-identity")
        .expect("identity tabs paint");
    let status = vcx.debug_bounds("panel-status").expect("status paints");
    let input = vcx.debug_bounds("prompt-input").expect("input paints");
    // Identity pills and status share one row directly below the input.
    assert!(identity.top() >= input.bottom());
    assert!((f32::from(identity.center().y - bounds.center().y)).abs() < 1.);
    assert!(identity.right() <= bounds.left() + gpui::px(1.));
    assert!(status.right() <= bounds.right());
    assert!(vcx.debug_bounds("panel-status-pulse").is_none());
}

#[gpui::test]
fn reconnect_history_recovers_a_response_missed_by_the_event_stream(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");

    panel.update(vcx, |panel, cx| {
        panel.history_loaded = true;
        *panel.items = vec![Item::User("hello".into())];
        panel.load_history(
            vec![
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None, timestamp_ms: None, tool: None,
                    role: "user".into(),
                    content: "hello".into(),
                },
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None, timestamp_ms: None, tool: None,
                    role: "assistant".into(),
                    content: "recovered response".into(),
                },
            ],
            Vec::new(),
            cx,
        );
        panel.apply(&ApiEvent::SessionStatus { session_id: "session-a".into(), status: "idle".into(), pending_soft_interrupts: None }, cx);
        assert!(matches!(
            panel.items.last(),
            Some(Item::Assistant(text)) if text == "recovered response"
        ));

        *panel.items = vec![Item::User("next".into())];
        panel.streaming_text = "partial".into();
        panel.load_history(
            vec![jcode_sdk::HistoryMessage { attachments: Vec::new(), role: "user".into(), content: "next".into(), response_stats: None, timestamp_ms: None, tool: None }, jcode_sdk::HistoryMessage { attachments: Vec::new(),
                response_stats: None, timestamp_ms: None, tool: None,
                role: "assistant".into(),
                content: "partial response completed".into(),
            }],
            Vec::new(),
            cx,
        );
        assert_eq!(panel.streaming_text, "partial", "history must not duplicate queued deltas");
        panel.apply(&ApiEvent::SessionStatus { session_id: "session-a".into(), status: "idle".into(), pending_soft_interrupts: None }, cx);
        assert!(matches!(panel.items.last(), Some(Item::Assistant(text)) if text == "partial response completed"));
    });
}

#[gpui::test]
fn model_read_images_are_anchored_and_deduplicated_in_the_transcript(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");

    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::Tool {
            call_id: "read-1".into(),
            name: "read".into(),
            input: r#"{"file_path":"chart.png"}"#.into(),
            output: "image loaded".into(),
            done: true,
            error: None,
        }];
        let event = ApiEvent::SidePaneImages {
            session_id: "session-a".into(),
            images: vec![jcode_sdk::RenderedImage {
                session_image: None,
                history_message_index: None,
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
                label: Some("chart.png".into()),
                source: jcode_sdk::RenderedImageSource::ToolResult {
                    tool_name: "read".into(),
                },
                anchor: Some(jcode_sdk::RenderedImageAnchor::ToolCall {
                    id: "read-1".into(),
                }),
            }],
        };
        panel.apply(&event, cx);
        panel.apply(&event, cx);

        assert_eq!(panel.items.len(), 2, "replayed image events are deduplicated");
        assert!(matches!(&panel.items[1], Item::Image(image) if image.label.as_deref() == Some("chart.png")));
    });
}

#[gpui::test]
fn history_restores_pasted_images_after_their_user_prompt(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");

    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.load_history(
            vec![jcode_sdk::HistoryMessage {
                attachments: Vec::new(),
                response_stats: None,
                timestamp_ms: None,
                tool: None,
                role: "user".into(),
                content: "what is in this?".into(),
            }],
            vec![jcode_sdk::RenderedImage {
                session_image: None,
                history_message_index: None,
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
                label: None,
                source: jcode_sdk::RenderedImageSource::UserInput,
                anchor: Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal: 0 }),
            }],
            cx,
        );
        assert!(matches!(
            panel.items.as_slice(),
            [Item::User(_), Item::Image(_)]
        ));
    });
}

#[gpui::test]
fn signed_agent_message_paints_without_replacing_a_draft(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("recipient", cx);
        workspace.push_test_panel("sender", cx);
        workspace
    });
    let (recipient, sender) = workspace.update(vcx, |workspace, _| {
        (
            workspace.test_panel(0).unwrap(),
            workspace.test_panel(1).unwrap(),
        )
    });
    recipient.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel
            .input
            .update(cx, |input, cx| input.set_content("unsent draft".into(), cx));
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            crate::harness::Update::Event {
                session_id: "recipient".into(),
                event: ApiEvent::AgentMessage {
                    session_id: "recipient".into(),
                    from_session: "sender".into(),
                    from_name: Some("fox".into()),
                    message: "DM from fox: check the output".into(),
                },
            },
            cx,
        );
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message").is_some());
    recipient.update(vcx, |panel, cx| {
        assert!(
            matches!(panel.items.last(), Some(Item::AgentMessage { from_session, message, .. })
            if from_session == "sender" && message == "DM from fox: check the output")
        );
        assert_eq!(panel.input.read(cx).snapshot().content, "unsent draft");
    });
    sender.update(vcx, |panel, _| {
        assert!(
            !panel
                .items
                .iter()
                .any(|item| matches!(item, Item::AgentMessage { .. }))
        );
    });
}

#[gpui::test]
fn agent_dm_discloses_its_body_on_click(cx: &mut gpui::TestAppContext) {
    let opened_sender = std::rc::Rc::new(std::cell::RefCell::new(None));
    let reported_sender = opened_sender.clone();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("recipient", cx);
        cx.subscribe(
            &workspace.test_panel(0).unwrap(),
            move |_, _, event: &OpenSwarmWorker, _| {
                reported_sender.replace(Some(event.0.clone()));
            },
        )
        .detach();
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            crate::harness::Update::Event {
                session_id: "recipient".into(),
                event: ApiEvent::AgentMessage {
                    session_id: "recipient".into(),
                    from_session: "sender".into(),
                    from_name: Some("fox".into()),
                    message: "DM from fox: First line\n\n**Hidden second line**".into(),
                },
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let summary = vcx
        .debug_bounds("agent-message-summary")
        .expect("one-line DM disclosure");
    let pill = vcx
        .debug_bounds("tool-identity-pill")
        .expect("incoming DM shows the swarm pill");
    let time = vcx
        .debug_bounds("tool-elapsed")
        .expect("live DM shows its arrival time");
    let icon = vcx.debug_bounds("swarm-action").unwrap();
    let sender = vcx.debug_bounds("agent-message-sender").unwrap();
    assert!(pill.left() < icon.left() && icon.right() < pill.right());
    assert!(pill.right() < sender.left());
    assert!(vcx.debug_bounds("swarm-direction").is_none());
    assert!(pill.left() >= summary.left() && time.right() <= summary.right());
    assert!(pill.right() < time.left());
    assert!(vcx.debug_bounds("tool-card-tokens").is_none());
    let window = vcx.window_handle();
    vcx.simulate_window_resize(window, gpui::size(px(480.), px(680.)));
    vcx.run_until_parked();
    let summary = vcx.debug_bounds("agent-message-summary").unwrap();
    let preview = vcx.debug_bounds("tool-swarm-preview").unwrap();
    let time = vcx.debug_bounds("tool-elapsed").unwrap();
    assert!(preview.right() <= time.left() && time.right() <= summary.right());
    assert!(vcx.debug_bounds("agent-message-body").is_none());
    vcx.simulate_click(summary.center(), gpui::Modifiers::none());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-body").is_some());
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-body").is_none());
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-body").is_some());
    vcx.simulate_click(summary.center(), gpui::Modifiers::none());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-body").is_none());

    let sender = vcx.debug_bounds("agent-message-sender").unwrap();
    vcx.simulate_click(sender.center(), gpui::Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(opened_sender.borrow().as_deref(), Some("sender"));
    assert!(vcx.debug_bounds("agent-message-body").is_none());
}

#[gpui::test]
fn agent_dm_search_discloses_a_restored_message(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("recipient", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        let history = serde_json::from_value(serde_json::json!([{
            "role": "user",
            "content": "DM from fox: First line\n\n**Obscured search term**"
        }]))
        .unwrap();
        panel.load_history(history, vec![], cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-summary").is_some());
    assert!(vcx.debug_bounds("tool-identity-pill").is_some());
    assert!(vcx.debug_bounds("tool-elapsed").is_none());
    assert!(vcx.debug_bounds("tool-card-tokens").is_none());
    assert!(vcx.debug_bounds("agent-message-body").is_none());
    panel.update(vcx, |panel, cx| {
        panel.search_terms = vec!["obscured".into()];
        assert!(panel.reveal_transcript_match(Some("Obscured search term"), true, cx));
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("agent-message-body").is_some());
    panel.update(vcx, |panel, _| {
        let item = panel.items.last().unwrap();
        let open = expandable::Open::for_row(&panel.expanded_regions, item, 0);
        let text: Vec<_> =
            text_document::segments_with_display(item, 0, open, &crate::config::display())
                .into_iter()
                .map(|(_, text)| text.to_string())
                .collect();
        assert_eq!(text, ["First line", "Obscured search term"]);
    });
}

#[gpui::test]
fn peer_prompt_paints_at_end_of_long_history(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.unwrap();
    panel.update(vcx, |panel, cx| {
        let messages = (0..40)
            .map(|i| {
                serde_json::json!({
                    "role": if i % 2 == 0 { "user" } else { "assistant" },
                    "content": format!("previous message {i}")
                })
            })
            .collect::<Vec<_>>();
        panel.load_history(serde_json::from_value(messages.into()).unwrap(), vec![], cx);
    });
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.transcript_list.scroll_to_end();
        panel.stick_to_bottom = true;
        panel.apply(
            &ApiEvent::PeerUserMessage {
                session_id: "session-a".into(),
                content: "давай делать".into(),
            },
            cx,
        );
        panel.apply(
            &ApiEvent::ReasoningDelta {
                session_id: "session-a".into(),
                text: "thinking".into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let bounds = vcx
        .debug_bounds("user-prompt-40")
        .expect("peer prompt paints at the bottom");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
}

#[gpui::test]
fn peer_prompt_paints_in_workspace_after_previous_answer(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.unwrap();
    panel.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.items.push(Item::Assistant("previous answer".into()));
    });
    workspace.update(vcx, |workspace, cx| {
        for event in [
            ApiEvent::PeerUserMessage {
                session_id: "session-a".into(),
                content: "давай делать".into(),
            },
            ApiEvent::ReasoningDelta {
                session_id: "session-a".into(),
                text: "thinking".into(),
            },
        ] {
            workspace.apply(
                crate::harness::Update::Event {
                    session_id: "session-a".into(),
                    event,
                },
                cx,
            );
        }
    });
    vcx.run_until_parked();
    let bounds = vcx
        .debug_bounds("user-prompt-1")
        .expect("peer prompt card paints");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
}

#[gpui::test]
fn peer_prompt_is_visible_live_after_compaction(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    panel.unwrap().update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel
            .items
            .push(Item::Compaction(compaction::CompactionRow::complete(
                "compacted · snapcompact",
            )));
        panel.apply(
            &ApiEvent::PeerUserMessage {
                session_id: "session-a".into(),
                content: "external prompt".into(),
            },
            cx,
        );
        panel.apply(
            &ApiEvent::TextDelta {
                session_id: "session-a".into(),
                message_id: None,
                text: "answer".into(),
            },
            cx,
        );
        assert!(
            matches!(panel.items.last(), Some(Item::User(text)) if text == "external prompt")
        );
        assert_eq!(panel.streaming_text, "answer");
    });
}

#[gpui::test]
fn keyboard_submission_reaches_the_bridge_and_streamed_text_paints(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();

    vcx.simulate_input("hello jcode");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::Send { session_id, content, images, .. })
            if session_id == "session-a" && content == "hello jcode" && images.is_empty()
    ));

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::TextDelta {
                message_id: None,
                session_id: "session-a".into(),
                text: "hello back".into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("assistant-response")
        .expect("streamed assistant response should paint");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
    assert!(vcx.debug_bounds("assistant-avatar").is_none());
    assert!(vcx.debug_bounds("role-caption-jcode").is_none());
}

#[gpui::test]
fn escape_cancels_an_active_transcript_from_the_focused_prompt(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    panel.update(vcx, |panel, cx| {
        panel.status = "busy".into();
        cx.notify();
    });

    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-activity").is_some());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-activity").is_none());
    panel.read_with(vcx, |panel, _| assert!(!panel.activity_active()));

    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::Cancel { session_id }) if session_id == "session-a"
    ));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::ForceStopTurn { session_id }) if session_id == "session-a"
    ));
}

/// Nested inline markdown used to produce overlapping highlight ranges,
/// and GPUI aborts the whole process (`invalid text run`) when those reach
/// `StyledText`. Streaming crash-shaped text through the public event path
/// and painting it through the real window is the acceptance check that a
/// live session can no longer take the desktop down.
#[gpui::test]
fn streamed_nested_markdown_paints_without_aborting(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");

    // Every shape that nests one inline span in another, including the
    // exact input from the recorded crash ("n the" inside a 12-byte run).
    let crash_shaped = "prefix **`n the`** suffix, *italic with [a link](https://example.com) \
                        and `code`*, ~~strike **bold `code`** tail~~, and \
                        [**bold link**](https://example.com/x).";
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::TextDelta {
                message_id: None,
                session_id: "session-a".into(),
                text: crash_shaped.into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("assistant-response")
        .expect("nested markdown response should paint");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
}

/// Mermaid support must travel through the same streamed assistant event and
/// virtualized transcript paint path used by a real session. Testing the SVG
/// helper alone would not catch a panel that still displayed the fenced source.
#[gpui::test]
fn streamed_mermaid_renders_as_a_visible_panel_diagram(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::TextDelta {
                message_id: None,
                session_id: "session-a".into(),
                text: "```mermaid\nflowchart LR\nA[Start] --> B[Done]\n```".into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("md-mermaid")
        .expect("Mermaid fence should paint as a diagram in the panel");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
}

#[gpui::test]
fn slash_commands_dispatch_native_session_operations(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    vcx.simulate_input("/effort x");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::SessionOperation {
            session_id,
            operation: SessionOperation::SetEffort(effort),
        }) if session_id == "session-a" && effort == "xhigh"
    ));

    vcx.simulate_input("/clear");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::SessionOperation {
            session_id,
            operation: SessionOperation::Clear,
        }) if session_id == "session-a"
    ));
}

#[gpui::test]
fn update_command_submits_through_the_prompt_and_reports_platform_availability(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let _update_guard = crate::updates::test_lock();
    crate::updates::set(crate::updates::UpdateState::Idle);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, _| panel = workspace.test_panel(0));
    let panel = panel.expect("test panel exists");
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    vcx.simulate_input("/update");
    vcx.simulate_keystrokes("enter");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();

    assert!(commands.try_recv().is_err(), "slash command stays local");
    panel.read_with(vcx, |panel, _| {
        assert!(matches!(
            panel.items.last(),
            Some(Item::Assistant(message))
                if message == "Automatic updates are unavailable in this build of Jcode Desktop."
        ));
    });
    let bounds = vcx
        .debug_bounds("assistant-response")
        .expect("the update result should paint in the transcript");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));

    #[cfg(target_os = "linux")]
    {
        // Exercise the production slash-command and asynchronous completion
        // path, with a deterministic platform callback rather than network IO.
        extern "C" fn current() {
            crate::updates::set(crate::updates::UpdateState::Finished {
                message: "Jcode Desktop is already current.".into(),
            });
        }
        extern "C" fn failed() {
            crate::updates::set(crate::updates::UpdateState::Failed {
                message: "Update failed: checksum mismatch.".into(),
            });
        }
        unsafe { crate::updates::jcode_update_register_actions(current, current) };
        vcx.simulate_input("/update");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(matches!(panel.items.last(), Some(Item::Assistant(message))
                if message == "Jcode Desktop is already current."));
        });
        unsafe { crate::updates::jcode_update_register_actions(failed, failed) };
        vcx.simulate_input("/update");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(matches!(panel.items.last(), Some(Item::Error(message))
                if message == "Update failed: checksum mismatch."));
        });
        assert!(
            commands.try_recv().is_err(),
            "Linux updater never invokes the model or CLI updater"
        );
        crate::updates::clear_test_actions();
    }
}

#[gpui::test]
fn model_command_replaces_suggestions_above_input_and_enter_switches_selection(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let mut panel = None;
    workspace.update(vcx, |workspace, cx| {
        panel = workspace.test_panel(0);
        panel.as_ref().unwrap().update(cx, |panel, cx| {
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
                    routes: vec![
                        route("claude-fable-5", "anthropic-api-key"),
                        route("gpt-5.6-sol", "openai-oauth"),
                    ],
                },
                cx,
            );
        });
    });
    let panel = panel.expect("test panel exists");
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    vcx.simulate_input("/mod");
    vcx.run_until_parked();
    let command = vcx
        .debug_bounds("slash-command-row-0")
        .expect("model command suggestion exists");
    vcx.simulate_click(command.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "clicking /model opens choices without selecting a model"
    );
    vcx.update(|_, cx| assert_eq!(panel.read(cx).input.read(cx).content.as_ref(), "/model "));
    assert!(vcx.debug_bounds("model-picker-logo-0").is_some());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());
    vcx.update(|_, cx| assert_eq!(panel.read(cx).input.read(cx).content.as_ref(), "/model "));
    vcx.simulate_keystrokes("cmd-a backspace");

    vcx.simulate_input("/model");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("model-picker-overlay").is_none());
    assert!(vcx.debug_bounds("model-picker-dialog").is_none());
    let suggestions = vcx
        .debug_bounds("slash-command-overlay")
        .expect("model suggestions replace command suggestions");
    let input = vcx
        .debug_bounds("prompt-input")
        .expect("composer remains rendered");
    assert!(
        suggestions.bottom() <= input.top() || suggestions.top() >= input.bottom(),
        "model suggestions must not cover the composer"
    );
    assert!((suggestions.left() - input.left()).abs() <= px(1.0));
    assert!((suggestions.size.width - input.size.width).abs() <= px(2.0));
    assert!(vcx.debug_bounds("model-picker-logo-0").is_some());
    assert!(vcx.debug_bounds("model-picker-logo-1").is_some());
    vcx.update(|_, cx| {
        assert_eq!(panel.read(cx).input.read(cx).content.as_ref(), "/model");
        assert_eq!(panel.read(cx).input.read(cx).model_picker_rows().len(), 2);
    });

    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            panel.read(cx).input.read(cx).model_picker_rows(),
            vec![
                ("gpt-5.6-sol".into(), false),
                ("claude-fable-5".into(), true)
            ]
        );
    });
    vcx.simulate_keystrokes("enter");
    let received = commands.recv_timeout(std::time::Duration::from_millis(100));
    match received {
        Ok(Command::SetModel { session_id, model }) => {
            assert_eq!(session_id, "session-a");
            assert_eq!(model, "claude-api:claude-fable-5");
        }
        _ => panic!("picker did not emit SetModel"),
    }
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("model-picker-overlay").is_none());
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());

    vcx.simulate_input("/model claude");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            panel.read(cx).input.read(cx).model_picker_rows(),
            vec![("claude-fable-5".into(), true)]
        );
    });
    vcx.simulate_keystrokes("ctrl-a");
    vcx.simulate_input("/model missing-route");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "no matches must not send an invalid model"
    );
    assert!(vcx.debug_bounds("slash-command-overlay").is_some());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());
    vcx.simulate_keystrokes("ctrl-a");
    vcx.simulate_input("/models");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").is_some());
    let row = vcx
        .debug_bounds("slash-command-row-0")
        .expect("model suggestion is clickable");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::SetModel { model, .. }) if model == "openai-oauth:gpt-5.6-sol")
    );
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());
    vcx.simulate_input("composer focus retained");
    vcx.update(|_, cx| {
        assert_eq!(
            panel.read(cx).input.read(cx).content.as_ref(),
            "composer focus retained"
        )
    });
}

#[test]
fn todo_tool_output_parses_items_and_plan() {
    let payload = parse_todo_tool_output(
        r#"[{"id":"build","content":"Build the card","status":"in_progress","priority":"high","group":"Desktop","confidence":"validated"}]
Plan: {"user_intention":"See progress at a glance","understands_user_intent":"clear"}
Goals: []"#,
    )
    .expect("todo payload parses");

    assert_eq!(payload.todos.len(), 1);
    assert_eq!(payload.todos[0].content, "Build the card");
    assert_eq!(payload.todos[0].group.as_deref(), Some("Desktop"));
    assert_eq!(
        payload.plan.user_intention.as_deref(),
        Some("See progress at a glance")
    );
}

#[test]
fn todo_tool_output_parses_goals_intent_and_confidence_like_the_tui() {
    let output = r#"[{"content":"Ship","status":"completed","priority":"high","id":"1","group":"Desktop","confidence":"plausible","completion_confidence":"validated"},{"content":"Next","status":"pending","priority":"high","id":"2","group":"Desktop","confidence":"speculative"}]

Plan:
{"user_intention":"Pinned todos stay open","understands_user_intent":"partial"}

Goals:
[{"group":"Desktop","closed_feedback_loop":"usable","feedback_loop_relevance":"representative","feedback_loop_coverage":"main_paths","feedback_loop_traceability":"partial","delivery_state":"integrated"}]"#;
    let payload = parse_todo_tool_output(output).unwrap();
    assert_eq!(
        payload.plan.understands_user_intent,
        Some(jcode_base::todo::IntentUnderstanding::Partial)
    );
    assert_eq!(
        todo_confidence_label(&payload.todos[0]).as_deref(),
        Some("plausible→validated")
    );
    assert_eq!(
        todo_confidence_label(&payload.todos[1]).as_deref(),
        Some("speculative")
    );
    let goal = todo_goal_for_group(&payload.goals, Some("Desktop")).unwrap();
    let labels: Vec<_> = todo_goal_gate_segments(goal)
        .into_iter()
        .map(|segment| (segment.label, segment.state, segment.tone))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("Closed feedback loop", "usable".into(), TodoTone::Warn),
            ("Delivery", "integrated".into(), TodoTone::Warn),
        ]
    );

    let passing = jcode_base::todo::TodoGoal {
        closed_feedback_loop: Some(jcode_base::todo::FeedbackLoopState::Closed),
        feedback_loop_relevance: Some(
            jcode_base::todo::FeedbackLoopRelevance::AcceptanceAligned,
        ),
        feedback_loop_coverage: Some(
            jcode_base::todo::FeedbackLoopCoverage::EdgeAndIntegrationPaths,
        ),
        feedback_loop_traceability: Some(jcode_base::todo::FeedbackLoopTraceability::Complete),
        ..Default::default()
    };
    let segments = todo_goal_gate_segments(&passing);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].tone, TodoTone::Ok);
    assert!(
        todo_goal_gate_segments(&Default::default())
            .iter()
            .all(|segment| segment.state == "missing" && segment.tone == TodoTone::Fail)
    );
}

#[gpui::test]
fn completed_todo_tool_paints_as_a_native_card(cx: &mut gpui::TestAppContext) {
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
        *panel.items = vec![
            Item::User("Keep the task list compact".into()),
            Item::Tool {
                call_id: "todo-1".into(),
                name: "todo".into(),
                input: r#"{"intent":"Track implementation"}"#.into(),
                output: r#"[{"id":"one","content":"Render rich rows","status":"completed","priority":"high","completion_confidence":"verified","blocked_by":[]},{"id":"two","content":"Test the card","status":"in_progress","priority":"medium","group":"Validation","confidence":"validated","blocked_by":[]}] Plan: {"user_intention":"See progress visually","understands_user_intent":"clear"} Goals: []"#.into(),
                done: true,
                error: None,
            },
        ];
        cx.notify();
    });
    vcx.run_until_parked();

    let pinned = vcx
        .debug_bounds("pinned-todo-card")
        .expect("todo card should be pinned outside the transcript");
    assert!(
        vcx.debug_bounds("pinned-latest-prompt").is_none(),
        "a visible transcript prompt must not be duplicated above the todo card"
    );
    let transcript = vcx.debug_bounds("transcript").expect("transcript paints");
    assert!(pinned.bottom() <= transcript.top());
    assert!(vcx.debug_bounds("tool-inline").is_none());
    assert!(
        vcx.debug_bounds("todo-card").is_none(),
        "no tool-style card when pinned"
    );
    let paper = vcx
        .debug_bounds("pinned-todo-details-paper")
        .expect("pinned todos are always expanded");
    let intent = vcx
        .debug_bounds("pinned-todo-intent-row")
        .expect("intent row heads the card");
    assert!(intent.top() >= paper.top());
    let count = vcx
        .debug_bounds("pinned-todo-count")
        .expect("progress count");
    assert!(count.right() <= paper.right());
    assert!(vcx.debug_bounds("pinned-todo-intent-state").is_some());
    assert!(vcx.debug_bounds("todo-row-confidence").is_some());
    let rows = vcx.debug_bounds("todo-row").expect("task rows");
    assert!(rows.top() > intent.bottom());
}
