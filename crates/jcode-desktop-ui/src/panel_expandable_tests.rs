//! Red-first coverage for truncated text: every cut region must toggle open and
//! keep the complete text reachable, per class of truncation.

use super::*;

#[gpui::test]
fn markdown_tool_output_is_rendered_and_selectable(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = panel_with(cx, vec![Item::Tool {
        call_id: "markdown".into(), name: "webfetch".into(), input: r#"{"url":"example"}"#.into(),
        output: "# Report\n\nA **useful** result.".into(), done: true, error: None,
    }]);
    click(vcx, "tool-header");
    assert!(vcx.debug_bounds("tool-detail-markdown").is_some());
    let document = document_text(&panel, vcx);
    assert!(document.contains("Report") && document.contains("useful"));
    assert!(!document.contains("**useful**"));
}

#[gpui::test]
fn bash_result_displays_exit_badge(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = panel_with(cx, vec![Item::Tool {
        call_id: "bash-status".into(), name: "bash".into(), input: r#"{"command":"false"}"#.into(),
        output: "failed\n\n--- Command finished with exit code: 1 ---\n".into(), done: true, error: None,
    }]);
    click(vcx, "tool-header");
    assert!(vcx.debug_bounds("tool-detail-exit-code").is_some());
    assert!(document_text(&panel, vcx).contains("command: false"));
}

#[gpui::test]
fn read_and_diff_use_selectable_syntax_rendering(cx: &mut gpui::TestAppContext) {
    for (name, input, output, selector) in [
        ("read", r#"{"file_path":"lib.rs"}"#, "fn main() {}\n", "tool-detail-syntax-read"),
        ("edit", "{}", "@@ -1 +1 @@\n-old\n+new\n", "tool-detail-syntax-diff"),
    ] {
        let (_, vcx) = panel_with(cx, vec![Item::Tool { call_id: name.into(), name: name.into(), input: input.into(), output: output.into(), done: true, error: None }]);
        click(vcx, "tool-header");
        assert!(vcx.debug_bounds(selector).is_some(), "{name}");
        assert!(vcx.debug_bounds("selectable-text-tool-detail-0").is_some());
    }
}

#[gpui::test]
fn background_completion_opens_output_preview(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = panel_with(cx, vec![]);
    panel.update(vcx, |panel, cx| panel.apply(&ApiEvent::BackgroundProgress {
        session_id: "expandable".into(), task_id: "task-1".into(), label: "Build".into(),
        summary: "✓ completed · 2.0s · exit 0".into(),
        detail: Some("**Background task** `task-1` · `Build` · ✓ completed · 2.0s · exit 0\n\n```text\nactual output tail\n```".into()),
        percent: None, done: true,
    }, cx));
    vcx.run_until_parked();
    click(vcx, "background-task-expand");
    assert!(vcx.debug_bounds("background-task-output").is_some());
    assert!(document_text(&panel, vcx).contains("actual output tail"));
}

#[gpui::test]
fn bg_result_shows_structured_details(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = panel_with(cx, vec![Item::Tool {
        call_id: "bg-result".into(), name: "bg".into(),
        input: r#"{"action":"wait","task_id":"abc"}"#.into(),
        output: "Background task finished.\n\nTask: abc\nName: build\nStatus: completed\nDuration: 2s\nExit code: 0\n\nOutput preview:\n```text\nhello\n```".into(),
        done: true, error: None,
    }]);
    click(vcx, "tool-header");
    assert!(vcx.debug_bounds("tool-detail-status").is_some());
    assert!(vcx.debug_bounds("selectable-text-tool-detail-0").is_some());
    let text = document_text(&panel, vcx);
    assert!(text.contains("completed") && text.contains("task_id") && text.contains("hello"));
    assert!(!text.contains("Tool call\n") && !text.contains("{\n"));
}

#[gpui::test]
fn tool_row_click_and_full_output_scroll(cx: &mut gpui::TestAppContext) {
    let output = format!("{}last output line", "output line\n".repeat(550));
    let (panel, vcx) = panel_with(
        cx,
        vec![Item::Tool {
            call_id: "bash-row".into(),
            name: "bash".into(),
            input: r#"{"command":"echo test","intent":"Run a command"}"#.into(),
            output,
            done: true,
            error: None,
        }],
    );
    click(vcx, "tool-header");
    assert!(vcx.debug_bounds("tool-detail").is_some());
    assert!(vcx.debug_bounds("tool-title-0").is_none());
    assert!(vcx.debug_bounds("tool-detail-copy").is_none());
    let viewport = vcx.debug_bounds("tool-detail-0-scroll").unwrap();
    let handle = panel.read_with(&*vcx, |panel, _| {
        panel
            .swarm_scrolls
            .borrow()
            .get("bash-row")
            .unwrap()
            .handle
            .clone()
    });
    assert!(handle.max_offset().y > px(0.));
    vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
    for _ in 0..200 {
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
            ..Default::default()
        });
        vcx.run_until_parked();
    }
    assert_eq!(handle.offset().y, -handle.max_offset().y);
    let end = vcx.debug_bounds("tool-detail-end").unwrap();
    assert!(end.bottom() <= viewport.bottom() && end.top() >= viewport.top());
    assert!(document_text(&panel, vcx).contains("last output line"));
}

/// The daemon's refusal for an irreversible command, as the user sees it: the
/// first sentence is visible and the paragraphs that follow were unreachable.
fn irreversible_error() -> String {
    let mut text = String::from("Error: This command was not run. It is irreversible:\n\n");
    text.push_str("The target cannot be confirmed as the one the user named.\n");
    for n in 0..12 {
        text.push_str(&format!(
            "- Check {n}: did the user name this path, or did you infer it?\n"
        ));
    }
    text.push_str(
        "Re-issue the same call with a justification field explaining which request it serves.",
    );
    text
}

fn panel_with(
    cx: &mut gpui::TestAppContext,
    items: Vec<Item>,
) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "expandable".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = items;
        panel.stick_to_bottom = false;
        panel
    });
    vcx.run_until_parked();
    (panel, vcx)
}

fn click(vcx: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = vcx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
}

/// The text a drag selection or Ctrl-C would reach, built from the same items
/// and disclosure state the transcript paints.
fn document_text(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> String {
    panel.read_with(&*vcx, |panel, _| {
        let rows = panel.transcript_render_rows();
        let mut cache = text_document::TranscriptTextDocument::default();
        cache
            .sync(
                &panel.items,
                &rows,
                &panel.expanded_prompts,
                &panel.expanded_regions,
            )
            .unwrap_or_default()
            .into_iter()
            .map(|(_, text)| text.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

#[gpui::test]
fn long_prompt_is_bounded_scrollable_and_collapsible_in_short_window(
    cx: &mut gpui::TestAppContext,
) {
    let prompt: String = (0..100)
        .map(|n| format!("Prompt line {n}: enough text to wrap.\n\n"))
        .collect();
    let (panel, vcx) = panel_with(cx, vec![Item::User(prompt)]);
    let window = vcx.window_handle();
    vcx.simulate_window_resize(window, gpui::size(px(600.), px(400.)));
    vcx.run_until_parked();
    let compact = vcx.debug_bounds("user-prompt-0").unwrap();
    click(vcx, "prompt-expand-0");
    let viewport = panel.read_with(&*vcx, |panel, _| panel.transcript_list.viewport_bounds());
    let expanded = vcx.debug_bounds("user-prompt-0").unwrap();
    let scroll = vcx
        .debug_bounds("prompt-scroll-0")
        .expect("expanded prompt scrolls");
    let control = vcx
        .debug_bounds("prompt-expand-0")
        .expect("collapse control paints");
    assert!(
        vcx.debug_bounds("prompt-scrollbar").is_some(),
        "scrollbar is visible immediately"
    );
    assert!(
        expanded.size.height <= viewport.size.height * 0.6,
        "{expanded:?} vs {viewport:?}"
    );
    assert!(scroll.size.height < expanded.size.height);
    assert!(control.top() >= viewport.top() && control.bottom() <= viewport.bottom());
    let handle = panel.read_with(&*vcx, |panel, _| {
        panel
            .prompt_scrolls
            .borrow()
            .get(&(0, false))
            .unwrap()
            .handle
            .clone()
    });
    assert!(handle.max_offset().y > px(0.));
    vcx.simulate_mouse_move(scroll.center(), None, gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: scroll.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
        ..Default::default()
    });
    vcx.run_until_parked();
    assert!(handle.offset().y < px(0.), "wheel must move the prompt");
    assert!(
        vcx.debug_bounds("prompt-scrollbar").is_some(),
        "overflow has a visible scrollbar"
    );
    assert_eq!(
        vcx.debug_bounds("prompt-expand-0").unwrap().origin,
        control.origin,
        "collapse stays fixed"
    );
    vcx.simulate_window_resize(window, gpui::size(px(600.), px(300.)));
    vcx.run_until_parked();
    let resized_viewport =
        panel.read_with(&*vcx, |panel, _| panel.transcript_list.viewport_bounds());
    let resized = vcx.debug_bounds("user-prompt-0").unwrap();
    assert!(
        resized.size.height <= resized_viewport.size.height * 0.6,
        "{resized:?} vs {resized_viewport:?}"
    );
    click(vcx, "prompt-expand-0");
    assert_eq!(
        vcx.debug_bounds("user-prompt-0").unwrap().size.height,
        compact.size.height
    );
}

#[gpui::test]
fn prompt_wheel_hands_off_to_transcript_at_bottom(cx: &mut gpui::TestAppContext) {
    let mut items = vec![Item::User("A long prompt paragraph.\n\n".repeat(100))];
    items.extend((0..40).map(|n| Item::Assistant(format!("Response {n}"))));
    let (panel, vcx) = panel_with(cx, items);
    let window = vcx.window_handle();
    vcx.simulate_window_resize(window, gpui::size(px(600.), px(400.)));
    vcx.run_until_parked();
    click(vcx, "prompt-expand-0");
    let scroll = vcx.debug_bounds("prompt-scroll-0").unwrap();
    let handle = panel.read_with(&*vcx, |panel, _| {
        panel
            .prompt_scrolls
            .borrow()
            .get(&(0, false))
            .unwrap()
            .handle
            .clone()
    });
    handle.set_offset(gpui::point(px(0.), -handle.max_offset().y));
    let before = panel.read_with(&*vcx, |panel, _| panel.transcript_list.logical_scroll_top());
    vcx.simulate_mouse_move(scroll.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: scroll.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
        ..Default::default()
    });
    vcx.run_until_parked();
    super::scroll_momentum_tests::settle(vcx);
    let after = panel.read_with(&*vcx, |panel, _| panel.transcript_list.logical_scroll_top());
    assert!(
        before.item_ix != after.item_ix || before.offset_in_item != after.offset_in_item,
        "wheel at prompt bottom moves the transcript"
    );
}

#[gpui::test]
fn pinned_expanded_prompt_scrolls_without_moving_transcript(cx: &mut gpui::TestAppContext) {
    let mut items = vec![Item::User("A long prompt paragraph.\n\n".repeat(100))];
    items.extend((0..40).map(|n| Item::Assistant(format!("Response {n}"))));
    let (panel, vcx) = panel_with(cx, items);
    let window = vcx.window_handle();
    vcx.simulate_window_resize(window, gpui::size(px(600.), px(500.)));
    panel.update(vcx, |panel, cx| {
        panel.stick_to_bottom = true;
        panel.transcript_list.scroll_to_end();
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("pinned-latest-prompt").is_some());
    click(vcx, "prompt-expand-0");
    let bounds = vcx.debug_bounds("prompt-scroll-0").unwrap();
    let handle = panel.read_with(&*vcx, |panel, _| {
        panel
            .prompt_scrolls
            .borrow()
            .get(&(0, true))
            .unwrap()
            .handle
            .clone()
    });
    let before = panel.read_with(&*vcx, |panel, _| panel.transcript_list.logical_scroll_top());
    vcx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: bounds.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
        ..Default::default()
    });
    vcx.run_until_parked();
    assert!(handle.offset().y < px(0.));
    let after = panel.read_with(&*vcx, |panel, _| panel.transcript_list.logical_scroll_top());
    assert_eq!(
        (before.item_ix, before.offset_in_item),
        (after.item_ix, after.offset_in_item)
    );
    click(vcx, "prompt-expand-0");
    panel.read_with(&*vcx, |panel, _| {
        assert!(!panel.expanded_prompts.contains(&(0, true)))
    });
}

#[gpui::test]
fn tool_error_is_available_in_expanded_detail(cx: &mut gpui::TestAppContext) {
    let message = irreversible_error();
    let (panel, vcx) = panel_with(
        cx,
        vec![Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: r#"{"command":"rm -rf /tmp/x"}"#.into(),
            output: String::new(),
            done: true,
            error: Some(message),
        }],
    );
    assert!(!document_text(&panel, vcx).contains("justification field"));
    click(vcx, "tool-header");
    let full = document_text(&panel, vcx);
    assert!(full.contains("justification field") && full.contains("Check 11"));
    assert!(vcx.debug_bounds("tool-detail-0-scroll").is_some());
    assert!(vcx.debug_bounds("tool-error-copy").is_none());
    click(vcx, "tool-header");
    assert!(!panel.read_with(&*vcx, |panel, _| panel.expanded_regions.contains("call")));
}

#[gpui::test]
fn tool_output_expands_past_the_old_clip_and_scrolls(cx: &mut gpui::TestAppContext) {
    let output: String = (0..200).map(|n| format!("output line {n}\n")).collect();
    let (panel, vcx) = panel_with(
        cx,
        vec![Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: r#"{"command":"make build"}"#.into(),
            output,
            done: true,
            error: None,
        }],
    );
    assert!(vcx.debug_bounds("tool-detail").is_none());
    click(vcx, "tool-output-size");
    let detail = vcx.debug_bounds("tool-detail").unwrap();
    assert!(vcx.debug_bounds("tool-detail-0-scroll").is_some());
    assert!(detail.size.height <= px(expandable::MAX_HEIGHT + 64.0));
    assert!(vcx.debug_bounds("tool-detail-copy").is_none());
    let text = document_text(&panel, vcx);
    assert!(text.contains("output line 0") && text.contains("output line 199"));
}

#[gpui::test]
fn tool_title_stays_one_line_when_expanded(cx: &mut gpui::TestAppContext) {
    let command = format!(
        "{} --and-then-some-more",
        "cargo test --workspace ".repeat(8)
    );
    let (panel, vcx) = panel_with(
        cx,
        vec![Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: serde_json::json!({ "command": command }).to_string(),
            output: "ok".into(),
            done: true,
            error: None,
        }],
    );
    let compact = vcx.debug_bounds("tool-header").unwrap().size;
    assert!(vcx.debug_bounds("tool-title-0").is_none());
    click(vcx, "tool-header");
    assert_eq!(
        vcx.debug_bounds("tool-header").unwrap().size.height,
        compact.height
    );
    assert!(document_text(&panel, vcx).contains("--and-then-some-more"));
    click(vcx, "tool-header");
    assert!(!panel.read_with(&*vcx, |panel, _| panel.expanded_regions.contains("call")));
}

#[gpui::test]
fn background_task_disclosure_stays_in_header_at_narrow_and_wide_widths(
    cx: &mut gpui::TestAppContext,
) {
    for done in [false, true] {
        let items = vec![Item::BackgroundTask {
            task_id: "task-1".into(),
            label: "Rollout outcome + snail push".into(),
            summary: "completed · 0.6s · exit 0".into(),
            detail: None,
            percent: None,
            done,
        }];
        let (_, vcx) = panel_with(cx, items);
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [400., 1200.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.)));
            vcx.run_until_parked();
            let header = vcx
                .debug_bounds("background-task-expand")
                .expect("header");
            let chevron = vcx
                .debug_bounds("background-task-chevron-0")
                .expect("chevron");
            assert!(
                chevron.top() >= header.top() && chevron.bottom() <= header.bottom(),
                "width {width}: {header:?} {chevron:?}"
            );
            assert!(
                chevron.left() > header.left(),
                "chevron must follow the header: {header:?} {chevron:?}"
            );
        }
        click(vcx, "background-task-chevron-0");
        assert!(vcx.debug_bounds("background-task-card").is_some());
    }
}

#[gpui::test]
fn background_task_row_expands_to_its_whole_summary(cx: &mut gpui::TestAppContext) {
    let summary = format!("completed · 12s · {}", "indexed files ".repeat(20));
    let items = vec![Item::BackgroundTask {
        task_id: "task-1".into(),
        label: "Rebuild the search index across every collection".into(),
        summary,
        detail: None,
        percent: None,
        done: true,
    }];
    let (panel, vcx) = panel_with(cx, items);
    let compact = vcx
        .debug_bounds("background-task-card")
        .expect("card paints")
        .size;
    click(vcx, "background-task-chevron-0");
    panel.read_with(&*vcx, |panel, _| {
        assert!(
            panel
                .expanded_regions
                .contains(&expandable::key::task("task-1"))
        );
    });
    let expanded = vcx
        .debug_bounds("background-task-card")
        .expect("card paints")
        .size;
    assert!(
        expanded.height > compact.height,
        "the row wraps: {compact:?} -> {expanded:?}"
    );
    // The document always carries the complete text.
    let text = document_text(&panel, vcx);
    assert!(text.contains("Rebuild the search index across every collection"));
    click(vcx, "background-task-chevron-0");
    assert_eq!(
        vcx.debug_bounds("background-task-card")
            .unwrap()
            .size
            .height,
        compact.height
    );
}

#[gpui::test]
fn composer_worker_row_expands_to_the_whole_label(cx: &mut gpui::TestAppContext) {
    let label = format!(
        "{} worker",
        "rewire the sidebar swarm rows and keep every label readable ".repeat(3)
    );
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("expandable-swarm", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    let session: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
        "session_id": "session_rose_1_a",
        "status": "idle",
        "parent_session_id": "lead",
        "swarm_status": "running",
        "agent_label": label,
    }))
    .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.set_swarm_workers(
            vec![crate::swarm_view::Worker::from_session(&session, None, 0)],
            cx,
        );
        panel.swarm_list_open = true;
        cx.notify();
    });
    vcx.run_until_parked();
    let compact = vcx
        .debug_bounds("swarm-list-worker-0")
        .expect("worker row paints")
        .size;
    click(vcx, "swarm-list-worker-expand-0");
    panel.read_with(&*vcx, |panel, _| {
        assert!(
            panel
                .expanded_regions
                .contains(&expandable::key::worker("session_rose_1_a")),
            "the same worker key the sidebar uses"
        );
    });
    let expanded = vcx
        .debug_bounds("swarm-list-worker-0")
        .expect("worker row paints")
        .size;
    assert!(
        expanded.height > compact.height,
        "the label wraps: {compact:?} -> {expanded:?}"
    );
    click(vcx, "swarm-list-worker-expand-0");
    panel.read_with(&*vcx, |panel, _| {
        assert!(
            !panel
                .expanded_regions
                .contains(&expandable::key::worker("session_rose_1_a"))
        );
    });
}

#[gpui::test]
fn recovery_error_reveals_the_raw_provider_message(cx: &mut gpui::TestAppContext) {
    let raw = format!(
        "provider stream failed: {}",
        "unexpected frame from the upstream gateway ".repeat(10)
    );
    let (panel, vcx) = panel_with(cx, vec![Item::Error(raw.clone())]);
    let compact = vcx
        .debug_bounds("recovery-error")
        .expect("error card paints")
        .size;
    assert!(vcx.debug_bounds("0-error-raw-scroll").is_none());
    click(vcx, "0-error-expand");
    panel.read_with(&*vcx, |panel, _| {
        assert!(
            panel
                .expanded_regions
                .contains(&expandable::key::details(0))
        );
    });
    let expanded = vcx
        .debug_bounds("recovery-error")
        .expect("error card paints");
    assert!(expanded.size.height > compact.height);
    assert!(
        vcx.debug_bounds("0-error-raw-scroll").is_some(),
        "the raw message is painted in place"
    );
    let text = document_text(&panel, vcx);
    assert!(text.contains("unexpected frame from the upstream gateway"));
    click(vcx, "0-error-expand");
    assert_eq!(
        vcx.debug_bounds("recovery-error").unwrap().size.height,
        compact.height
    );
}

#[test]
fn every_truncated_class_shares_one_budget_shape() {
    // Each class cuts at a character or line budget and reports the cut, so the
    // toggle wording and the state key are the only per-site differences.
    for budget in [
        expandable::TITLE,
        expandable::ERROR,
        expandable::NOTICE,
        expandable::LABEL,
    ] {
        let long = "x".repeat(1000);
        assert!(expandable::collapsed(&long, budget).is_some());
        assert!(expandable::collapsed("short", budget).is_none());
    }
}
