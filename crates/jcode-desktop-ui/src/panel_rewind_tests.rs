use super::*;
use jcode_sdk::api::RewindMessageKey;

#[gpui::test]
fn clean_chat_rewind_cancel_restores_tail_without_extra_whitespace(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("rewind".into(), None, None, bridge, cx));
    panel.update(vcx, |panel, cx| {
        panel.set_clean_chat(Some(true), cx);
        for i in 0..20 {
            panel.items.push(Item::User(format!("Prompt {i}")));
            panel
                .items
                .push(Item::Assistant(jcode_render_core::reasoning_line_markup(
                    "hidden",
                )));
            panel.items.push(Item::Assistant(format!("Answer {i}")));
        }
        cx.notify();
    });
    let settle = |vcx: &mut gpui::VisualTestContext| {
        for _ in 0..6 {
            vcx.update(|window, cx| window.simulate_next_frame(cx));
        }
    };
    settle(vcx);
    let baseline = vcx.debug_bounds("transcript-row-39").expect("tail visible");
    let viewport = panel.read_with(vcx, |panel, _| panel.transcript_list.viewport_bounds());
    assert!(
        viewport.contains(&baseline.center()),
        "{baseline:?} outside {viewport:?}"
    );
    panel.update(vcx, |panel, cx| panel.start_rewind(cx));
    settle(vcx);
    vcx.simulate_keystrokes("left left");
    settle(vcx);
    vcx.simulate_keystrokes("escape");
    settle(vcx);
    let restored = vcx
        .debug_bounds("transcript-row-39")
        .expect("tail restored");
    assert!(
        (f32::from(restored.bottom() - baseline.bottom())).abs() < 2.,
        "before {baseline:?}, after {restored:?}"
    );
}

#[gpui::test]
fn clean_chat_rewind_skips_hidden_messages_in_both_directions(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("rewind".into(), None, None, bridge, cx));
    panel.update(vcx, |panel, cx| {
        panel.set_clean_chat(Some(true), cx);
        panel.items.push(Item::User("First".into()));
        panel
            .items
            .push(Item::Assistant(jcode_render_core::reasoning_line_markup(
                "hidden",
            )));
        panel.items.push(Item::Assistant("Answer".into()));
        panel
            .items
            .push(Item::Assistant(jcode_render_core::reasoning_line_markup(
                "hidden again",
            )));
        panel.items.push(Item::User("Second".into()));
        panel
            .items
            .push(Item::Assistant(jcode_render_core::reasoning_line_markup(
                "hidden last",
            )));
        panel.start_rewind(cx);
    });
    let check = |vcx: &mut gpui::VisualTestContext, selector: &'static str| {
        for _ in 0..4 {
            vcx.update(|window, cx| window.simulate_next_frame(cx));
        }
        let row = vcx.debug_bounds(selector).expect("visible selection");
        let viewport = panel.read_with(vcx, |panel, _| panel.transcript_list.viewport_bounds());
        assert!(
            viewport.contains(&row.center()),
            "{row:?} outside {viewport:?}"
        );
    };
    check(vcx, "rewind-selected-4");
    vcx.simulate_keystrokes("up");
    check(vcx, "rewind-selected-2");
    vcx.simulate_keystrokes("up");
    check(vcx, "rewind-selected-0");
    let outline = vcx.debug_bounds("rewind-selected-0").unwrap();
    let first = panel.read_with(vcx, |panel, _| {
        panel.transcript_list.bounds_for_item(0).unwrap()
    });
    assert!(
        outline.top() - first.top() < px(32.),
        "oversized rewind gutter: {outline:?} in {first:?}"
    );
    vcx.simulate_keystrokes("down down");
    check(vcx, "rewind-selected-4");
    vcx.simulate_keystrokes("left");
    check(vcx, "rewind-selected-0");
    vcx.simulate_keystrokes("right");
    check(vcx, "rewind-selected-4");
    panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(false), cx));
    vcx.simulate_keystrokes("down");
    check(vcx, "rewind-selected-5");
    panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(true), cx));
    check(vcx, "rewind-selected-4");
    vcx.simulate_keystrokes("escape");
    assert!(vcx.debug_bounds("rewind-keys").is_none());
}

#[gpui::test]
fn rewind_uses_transcript_rows_and_returns_focus_to_composer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("rewind", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.load_history(
            [
                ("user", "First prompt"),
                ("assistant", "First answer"),
                ("user", "Second prompt"),
                ("assistant", "Second answer"),
            ]
            .into_iter()
            .map(|(role, text)| {
                serde_json::from_value(serde_json::json!({"role": role, "content": text})).unwrap()
            })
            .collect(),
            vec![],
            cx,
        );
    });
    let enter_mode = |vcx: &mut gpui::VisualTestContext| {
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input_focus_handle(cx);
            window.focus(&focus, cx);
        });
        vcx.simulate_input("/rewind");
        vcx.simulate_keystrokes("enter");
    };
    enter_mode(vcx);
    assert!(vcx.debug_bounds("rewind-keys").is_some());
    assert!(vcx.debug_bounds("rewind-selected-3").is_some());
    vcx.simulate_keystrokes("up");
    assert!(vcx.debug_bounds("rewind-selected-2").is_some());
    vcx.simulate_keystrokes("down");
    assert!(vcx.debug_bounds("rewind-selected-3").is_some());
    vcx.simulate_keystrokes("up left");
    assert!(vcx.debug_bounds("rewind-selected-0").is_some());
    vcx.simulate_keystrokes("right");
    assert!(vcx.debug_bounds("rewind-selected-2").is_some());
    // A click or a focus restoration may return focus to the composer while
    // rewind is still active. Its Escape binding must not win.
    vcx.update(|window, cx| {
        let focus = panel.read(cx).input_focus_handle(cx);
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("escape");
    assert!(vcx.debug_bounds("rewind-keys").is_none());
    assert!(commands.try_recv().is_err());
    vcx.simulate_keystrokes("escape");
    assert!(commands.try_recv().is_err());
    assert!(vcx.debug_bounds("response-stop-notice").is_none());
    vcx.update(|window, cx| {
        assert!(panel.read(cx).input_focus_handle(cx).is_focused(window));
    });

    enter_mode(vcx);
    vcx.simulate_keystrokes("left left");
    let assistant = vcx.debug_bounds("transcript-row-1").unwrap();
    let viewport = panel.read_with(vcx, |panel, _| panel.transcript_list.viewport_bounds());
    assert!(
        viewport.contains(&assistant.center()),
        "{assistant:?} outside {viewport:?}"
    );
    vcx.simulate_click(assistant.center(), gpui::Modifiers::default());
    assert!(vcx.debug_bounds("rewind-selected-1").is_some());
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::SessionOperation {
        ref session_id, operation: SessionOperation::RewindTranscript(target),
    }) if session_id == "rewind" && Some(target.selected) == RewindMessageKey::new("assistant", "First answer")));
    assert!(vcx.debug_bounds("rewind-keys").is_none());
    vcx.update(|window, cx| {
        assert!(panel.read(cx).input_focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn rewind_centers_messages_and_crosses_history_pages(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("rewind".into(), None, None, bridge, cx));
    let message = |role: &str, content: String| {
        serde_json::from_value(serde_json::json!({"role": role, "content": content})).unwrap()
    };
    panel.update(vcx, |panel, cx| {
        panel.load_bounded_history(
            crate::harness::BoundedHistoryPage {
                messages: (0..100)
                    .map(|i| match i {
                        0 => message("user", "First loaded prompt".into()),
                        98 => message("assistant", "Long paragraph.\n\n".repeat(100)),
                        _ => message("assistant", format!("Message {i}")),
                    })
                    .collect(),
                images: vec![],
                before: Some("older".into()),
                prompt_offset: None,
            },
            cx,
        );
        panel.start_rewind(cx);
    });
    let assert_centered = |vcx: &mut gpui::VisualTestContext, selector| {
        for _ in 0..3 {
            vcx.update(|window, cx| window.simulate_next_frame(cx));
        }
        let row = vcx.debug_bounds(selector).expect(selector);
        let (viewport, dock) = panel.read_with(vcx, |panel, _| {
            (
                panel.transcript_list.viewport_bounds(),
                panel.transcript_end_pad,
            )
        });
        let center = viewport.center().y - px(dock / 2.);
        assert!(
            (f32::from(row.center().y - center)).abs() <= 2.,
            "{selector}: {row:?}, viewport: {viewport:?}, dock: {dock}"
        );
    };
    assert_centered(vcx, "rewind-selected-99");
    vcx.simulate_keystrokes("up");
    for _ in 0..3 {
        vcx.update(|window, cx| window.simulate_next_frame(cx));
    }
    let long = vcx.debug_bounds("rewind-selected-98").unwrap();
    let viewport = panel.read_with(vcx, |panel, _| panel.transcript_list.viewport_bounds());
    assert!(long.size.height > viewport.size.height);
    assert!((f32::from(long.top() - viewport.top())).abs() <= 2.);
    assert!(vcx.debug_bounds("pinned-latest-prompt").is_none());
    vcx.simulate_keystrokes("down");
    assert_centered(vcx, "rewind-selected-99");
    vcx.simulate_keystrokes("left");
    assert_centered(vcx, "rewind-selected-0");
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, Command::GetHistoryPage { .. }))
    );
    vcx.simulate_keystrokes("left");
    assert!(commands.try_iter().any(|command| matches!(
        command,
        Command::GetHistoryPage { session_id, before: Some(before) }
            if session_id == "rewind" && before == "older"
    )));
    panel.update(vcx, |panel, cx| {
        panel.load_bounded_history_page(
            Ok(crate::harness::BoundedHistoryPage {
                messages: vec![message("user", "Older prompt".into())],
                images: vec![],
                before: None,
                prompt_offset: Some(0),
            }),
            cx,
        );
    });
    assert_centered(vcx, "rewind-selected-0");
}

#[gpui::test]
fn escape_does_not_request_a_stop_when_the_panel_is_not_running(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("idle".into(), None, None, bridge, cx));
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        window.focus(&panel.read(cx).input_focus_handle(cx), cx);
    });
    panel.update(vcx, |panel, cx| {
        // A transport disconnect is not an indication that a turn is active.
        panel.connection_lost("disconnected", cx);
    });
    vcx.simulate_keystrokes("escape");
    assert!(commands.try_recv().is_err());
    assert!(vcx.debug_bounds("response-stop-notice").is_none());
}
