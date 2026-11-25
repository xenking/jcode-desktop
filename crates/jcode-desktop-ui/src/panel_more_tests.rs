use super::*;

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
