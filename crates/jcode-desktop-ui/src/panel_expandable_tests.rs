//! Red-first coverage for truncated text: every cut region must toggle open and
//! keep the complete text reachable, per class of truncation.

use super::*;

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
    text.push_str("Re-issue the same call with a justification field explaining which request it serves.");
    text
}

fn panel_with<'a>(
    cx: &'a mut gpui::TestAppContext,
    items: Vec<Item>,
) -> (Entity<Panel>, &'a mut gpui::VisualTestContext) {
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
fn tool_error_preview_expands_to_the_whole_refusal(cx: &mut gpui::TestAppContext) {
    let message = irreversible_error();
    let items = vec![Item::Tool {
        call_id: "call".into(),
        name: "bash".into(),
        input: r#"{"command":"rm -rf /tmp/x"}"#.into(),
        output: String::new(),
        done: true,
        error: Some(message.clone()),
    }];
    let (panel, vcx) = panel_with(cx, items);
    let compact = vcx.debug_bounds("tool-error").expect("error line paints");
    // Collapsed: the opening claim only, and never the last paragraph.
    let collapsed = document_text(&panel, vcx);
    assert!(collapsed.contains("This command was not run"));
    assert!(!collapsed.contains("justification field"));
    assert!(collapsed.contains('…'), "the cut is marked: {collapsed}");
    click(vcx, "tool-error-0-expand");
    panel.read_with(&*vcx, |panel, _| {
        assert!(panel.expanded_regions.contains("call"));
    });
    let expanded = vcx.debug_bounds("tool-error").expect("region paints");
    assert!(
        expanded.size.height > compact.size.height,
        "the region grew: {compact:?} -> {expanded:?}"
    );
    let full = document_text(&panel, vcx);
    assert!(full.contains("justification field"), "full text reachable");
    assert!(full.contains("Check 11"));
    assert!(!full.contains('…'));
    // The scrolling region keeps a long refusal inside the row.
    let scroll = vcx
        .debug_bounds("tool-error-0-scroll")
        .expect("scrolling region paints");
    assert!(scroll.size.height <= px(expandable::MAX_HEIGHT + 12.0));
    assert!(
        vcx.debug_bounds("tool-error-copy").is_some(),
        "the raw message is copyable"
    );
    click(vcx, "tool-error-0-expand");
    panel.read_with(&*vcx, |panel, _| {
        assert!(!panel.expanded_regions.contains("call"));
    });
    assert_eq!(
        vcx.debug_bounds("tool-error").unwrap().size.height,
        compact.size.height
    );
}

#[gpui::test]
fn tool_output_expands_past_the_old_clip_and_scrolls(cx: &mut gpui::TestAppContext) {
    let output: String = (0..200).map(|n| format!("output line {n}\n")).collect();
    let items = vec![Item::Tool {
        call_id: "call".into(),
        name: "bash".into(),
        input: r#"{"command":"make build"}"#.into(),
        output: output.clone(),
        done: true,
        error: None,
    }];
    let (panel, vcx) = panel_with(cx, items);
    assert!(vcx.debug_bounds("tool-detail").is_none(), "collapsed by default");
    click(vcx, "tool-output-size");
    let detail = vcx
        .debug_bounds("tool-detail")
        .expect("expanded output paints");
    assert!(vcx.debug_bounds("tool-detail-0-scroll").is_some());
    assert!(detail.size.height <= px(expandable::MAX_HEIGHT + 64.0));
    assert!(vcx.debug_bounds("tool-detail-copy").is_some());
    let text = document_text(&panel, vcx);
    assert!(text.contains("output line 0"));
    assert!(text.contains("output line 199"), "nothing is clipped away");
}

#[gpui::test]
fn tool_title_expands_from_one_line_to_the_whole_intent(cx: &mut gpui::TestAppContext) {
    let command = format!("{} --and-then-some-more", "cargo test --workspace ".repeat(8));
    let items = vec![Item::Tool {
        call_id: "call".into(),
        name: "bash".into(),
        input: serde_json::json!({ "command": command }).to_string(),
        output: "ok".into(),
        done: true,
        error: None,
    }];
    let (panel, vcx) = panel_with(cx, items);
    let compact = vcx.debug_bounds("tool-header").expect("header paints").size;
    let text = document_text(&panel, vcx);
    assert!(text.contains('…'), "the cut is marked: {text}");
    assert!(!text.contains("--and-then-some-more"));
    click(vcx, "tool-title-0");
    let wrapped = vcx.debug_bounds("tool-header").expect("header paints").size;
    assert!(
        wrapped.height > compact.height,
        "the title wraps: {compact:?} -> {wrapped:?}"
    );
    let full = document_text(&panel, vcx);
    assert!(full.contains("--and-then-some-more"), "full intent reachable");
    click(vcx, "tool-title-0");
    assert_eq!(
        vcx.debug_bounds("tool-header").unwrap().size.height,
        compact.height
    );
}

#[gpui::test]
fn background_task_row_expands_to_its_whole_summary(cx: &mut gpui::TestAppContext) {
    let summary = format!("completed · 12s · {}", "indexed files ".repeat(20));
    let items = vec![Item::BackgroundTask {
        task_id: "task-1".into(),
        label: "Rebuild the search index across every collection".into(),
        summary,
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
        assert!(panel.expanded_regions.contains(&expandable::key::task("task-1")));
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
        vcx.debug_bounds("background-task-card").unwrap().size.height,
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
            vec![crate::swarm_view::Worker::from_session(&session, 0)],
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
        assert!(panel.expanded_regions.contains(&expandable::key::details(0)));
    });
    let expanded = vcx.debug_bounds("recovery-error").expect("error card paints");
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
