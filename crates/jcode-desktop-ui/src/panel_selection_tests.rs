use super::*;

#[gpui::test]
fn transcript_drag_highlights_and_copies_text(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "selection-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Assistant("Select this transcript text".into())];
        panel
    });
    vcx.run_until_parked();
    let bounds = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let start = point(bounds.left() + px(1.), bounds.center().y);
    let end = point(bounds.right() + px(20.), bounds.center().y);
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: start,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    vcx.run_until_parked();
    vcx.simulate_event(gpui::MouseMoveEvent {
        position: end,
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: gpui::Modifiers::default(),
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, cx| {
        assert!(
            panel
                .transcript_selection
                .read(cx)
                .highlight("0-0", 26)
                .is_some(),
            "drag must paint a text highlight"
        );
    });
    vcx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position: end,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
    });
    vcx.simulate_keystrokes("ctrl-c");
    let copied = vcx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("Select this transcript text"));
}

#[gpui::test]
fn tab_from_selected_transcript_text_reaches_the_first_tool(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "selection-tab".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.clean_chat_override = Some(false);
        *panel.items = vec![
            Item::Assistant("Select this paragraph".into()),
            Item::Tool {
                call_id: "call".into(),
                name: "bash".into(),
                input: r#"{"command":"printf hello"}"#.into(),
                output: "hello".into(),
                done: true,
                error: None,
            },
            Item::Assistant("A later paragraph".into()),
        ];
        panel
    });
    assert_eq!(
        drag_copy(vcx, "selectable-text-0-0"),
        "Select this paragraph"
    );
    vcx.update(|window, cx| window.focus_next(cx));
    vcx.simulate_keystrokes("enter");
    panel.read_with(vcx, |panel, _| {
        assert!(panel.expanded_regions.contains("call"))
    });
}

#[gpui::test]
fn transcript_selection_stops_after_release_outside_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "selection-release".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Assistant("Select this transcript text".into())];
        panel
    });
    vcx.run_until_parked();
    let bounds = vcx.debug_bounds("selectable-text-0-0").unwrap();
    for release_delivered in [true, false] {
        let start = point(bounds.left() + px(1.), bounds.center().y);
        let end = point(bounds.right() + px(20.), bounds.center().y);
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: start,
            modifiers: gpui::Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.simulate_event(gpui::MouseMoveEvent {
            position: end,
            pressed_button: Some(gpui::MouseButton::Left),
            modifiers: gpui::Modifiers::default(),
        });
        if release_delivered {
            vcx.simulate_event(gpui::MouseUpEvent {
                button: gpui::MouseButton::Left,
                position: point(px(-10.), px(-10.)),
                modifiers: gpui::Modifiers::default(),
                click_count: 1,
            });
        }
        // No button held: neither a delivered nor a missed mouse-up may leave
        // a sticky selection that changes when merely hovering the transcript.
        vcx.simulate_event(gpui::MouseMoveEvent {
            position: start,
            pressed_button: None,
            modifiers: gpui::Modifiers::default(),
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            panel
                .transcript_selection
                .update(cx, |selection, cx| selection.copy(cx))
        });
        let copied = vcx
            .update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text());
        assert_eq!(
            copied.as_deref(),
            Some("Select this transcript text"),
            "release delivered: {release_delivered}"
        );
    }
}

fn drag_copy(vcx: &mut gpui::VisualTestContext, key: &'static str) -> String {
    drag_copy_between(vcx, key, key)
}

#[gpui::test]
fn answered_decision_question_and_answer_are_selectable(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "decision-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Tool {
            call_id: "decision".into(),
            name: "ask".into(),
            input:
                r#"{"questions":[{"question":"Which database?","options":[{"label":"Postgres"}]}]}"#
                    .into(),
            output: "User answers:\n1. Which database? -> Postgres (comment: reliable)".into(),
            done: true,
            error: None,
        }];
        panel
    });
    assert_eq!(
        drag_copy(vcx, "selectable-text-decision-question-0-0"),
        "Which database?"
    );
    assert_eq!(
        drag_copy(vcx, "selectable-text-decision-answer-0-0"),
        "✓ Postgres\nUser comment: reliable"
    );
}

fn drag_copy_between(
    vcx: &mut gpui::VisualTestContext,
    first: &'static str,
    last: &'static str,
) -> String {
    vcx.run_until_parked();
    let first = vcx.debug_bounds(first).unwrap();
    let last = vcx.debug_bounds(last).unwrap();
    let start = point(first.left() + px(1.), first.top() + px(2.));
    let end = point(last.right() + px(20.), last.bottom() - px(2.));
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: start,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    vcx.simulate_event(gpui::MouseMoveEvent {
        position: end,
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: gpui::Modifiers::default(),
    });
    vcx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position: end,
        modifiers: gpui::Modifiers::default(),
        click_count: 1,
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("ctrl-c");
    vcx.update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .unwrap()
}

#[gpui::test]
fn tool_text_drag_copies_summary_output_and_error(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let input = r#"{"command":"printf βeta"}"#;
    let output = "βeta\nFinished";
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "tool-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Tool {
            call_id: "call".into(),
            name: "bash".into(),
            input: input.into(),
            output: output.into(),
            done: true,
            error: Some("A useful error".into()),
        }];
        panel.expanded_regions.insert("call".into());
        panel
    });
    assert_eq!(
        drag_copy_between(
            vcx,
            "selectable-text-tool-detail-0",
            "selectable-text-tool-bash-output-0"
        ),
        format!(
            "{}\n\nError\nA useful error",
            tool_detail("bash", input, output)
        )
    );
    assert!(panel.read_with(vcx, |panel, _| panel.expanded_regions.contains("call")));
    // Text gestures must not steal the adjacent disclosure control.
    let toggle = vcx.debug_bounds("tool-output-size").unwrap();
    vcx.simulate_click(toggle.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(!panel.read_with(vcx, |panel, _| panel.expanded_regions.contains("call")));
}

#[gpui::test]
fn pinned_task_selection_copies_without_collapsing_card(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (_panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "todo-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Todos(TodoCardPayload {
            todos: vec![TodoCardItem {
                content: "Make βeta selectable".into(),
                status: "pending".into(),
                group: Some("Selection".into()),
                blocked_by: vec![],
                confidence: None,
                completion_confidence: None,
                stage: None,
                assigned_to: None,
            }],
            plan: TodoCardPlan {
                user_intention: Some("Copy task text".into()),
                ..Default::default()
            },
            goals: Vec::new(),
        })];
        panel
    });
    assert_eq!(
        drag_copy(vcx, "selectable-text-pinned-todo-task-0-0"),
        "Make βeta selectable"
    );
    // Group headers render uppercase (TUI style, be2bc0e); the copy is
    // what is shown.
    assert_eq!(
        drag_copy(vcx, "selectable-text-pinned-todo-group-0"),
        "SELECTION"
    );
    assert_eq!(
        drag_copy(vcx, "selectable-text-pinned-todo-intention"),
        "Copy task text"
    );
}

#[gpui::test]
fn pinned_todos_show_phase_lane_and_assignee(cx: &mut gpui::TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("todo-owner".into(), None, None, crate::harness::spawn_inert(), cx);
        let payload = parse_todo_tool_output(r#"[{"content":"Ship","status":"in_progress","group":"desktop","stage":2,"assigned_to":"session_fox_123_a"}]"#).unwrap();
        assert_eq!(payload.todos[0].stage, Some(2));
        assert_eq!(payload.todos[0].assigned_to.as_deref(), Some("session_fox_123_a"));
        let legacy = parse_todo_tool_output(r#"[{"content":"Legacy","status":"pending","lane":"desktop","phase":"Phase 2"}]"#).unwrap();
        assert_eq!(legacy.todos[0].stage, Some(2));
        assert_eq!(legacy.todos[0].group.as_deref(), Some("desktop"));
        *panel.items = vec![Item::Todos(payload)];
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("pinned-todo-phase-2").is_some());
    assert!(vcx.debug_bounds("pinned-todo-assignee-0-0").is_some());
}

#[gpui::test]
fn markdown_tool_output_copies_rendered_text(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "tool-markdown-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::Tool {
            call_id: "md".into(),
            name: "webfetch".into(),
            input: "{}".into(),
            output: "# Report\n\nA **useful** result.".into(),
            done: true,
            error: None,
        }];
        panel.expanded_regions.insert("md".into());
        panel
    });
    let key = format!("selectable-text-{}-1", tool_output::markdown_row(0));
    assert_eq!(
        drag_copy(vcx, Box::leak(key.into_boxed_str())),
        "A useful result."
    );
}

#[gpui::test]
fn background_task_selection_copies_expanded_details(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "background-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![Item::BackgroundTask {
            task_id: "task".into(),
            label: "Run tests".into(),
            summary: "All tests passed".into(),
            detail: Some("**Background task** `task`\n\n```text\nAll tests passed\n```".into()),
            percent: Some(100.),
            done: true,
        }];
        panel.expanded_regions.insert(expandable::key::task("task"));
        panel
    });
    assert_eq!(
        drag_copy(vcx, "selectable-text-background-detail-0"),
        "All tests passed"
    );
}

#[gpui::test]
fn code_file_selection_copies_multiple_lines_without_gutter(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("example.rs");
    std::fs::write(&path, "first βeta\n\nlast line").unwrap();
    cx.update(crate::input::bind_keys);
    cx.update(crate::code_editor::bind_keys);
    let (_, vcx) = cx.add_window_view(|_, cx| {
        Panel::new_code_file(path.clone(), crate::harness::spawn_inert(), cx)
    });
    vcx.run_until_parked();
    let editor = vcx.debug_bounds("code-editor-text").unwrap();
    vcx.simulate_click(editor.center(), gpui::Modifiers::default());
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-c"
    } else {
        "ctrl-a ctrl-c"
    });
    assert_eq!(
        vcx.update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
            .as_deref(),
        Some("first βeta\n\nlast line")
    );
    assert_eq!(
        drag_copy(vcx, "selectable-text-code-file-path"),
        path.display().to_string()
    );
}

#[gpui::test]
fn document_header_selection_copies_outside_body_context(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (_, vcx) = cx.add_window_view(|_, cx| {
        Panel::new_side_document(
            "owner",
            &jcode_sdk::SidePanelPage {
                id: "notes".into(),
                title: "Working notes".into(),
                file_path: "/workspace/notes.md".into(),
                content: "Some text".into(),
                ..Default::default()
            },
            crate::harness::spawn_inert(),
            cx,
        )
    });
    assert_eq!(
        drag_copy(vcx, "selectable-text-document-title"),
        "Working notes"
    );
    assert_eq!(
        drag_copy(vcx, "selectable-text-document-source"),
        "/workspace/notes.md · read-only"
    );
}

fn begin_drag(vcx: &mut gpui::VisualTestContext, position: gpui::Point<gpui::Pixels>, shift: bool) {
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position,
        modifiers: gpui::Modifiers {
            shift,
            ..Default::default()
        },
        click_count: 1,
        first_mouse: false,
    });
    vcx.run_until_parked();
}

fn finish_drag_copy(
    vcx: &mut gpui::VisualTestContext,
    position: gpui::Point<gpui::Pixels>,
) -> String {
    vcx.simulate_event(gpui::MouseMoveEvent {
        position,
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: Default::default(),
    });
    vcx.run_until_parked();
    vcx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position,
        modifiers: Default::default(),
        click_count: 1,
    });
    // Releasing the pointer copies without an explicit shortcut.
    let auto = vcx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .unwrap();
    vcx.simulate_keystrokes("ctrl-c");
    let copied = vcx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text())
        .unwrap();
    assert_eq!(auto, copied, "mouse release must auto-copy the selection");
    copied
}

#[gpui::test]
fn clean_chat_filters_mixed_transcript_and_clipboard(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "clean-chat-fixture".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        let history = serde_json::from_value(serde_json::json!([
            {"role":"user","content":"Real question","timestamp_ms":1795326000000_i64},
            {"role":"assistant","content":format!("{}Public progress with no final marker", jcode_render_core::reasoning_line_markup("## Приватный заголовок"))},
            {"role":"tool","content":"private output","tool":{"id":"tool","name":"bash","input":{"command":"printf hidden"}}},
            {"role":"user","content":"DM from Worker: private coordination"},
            {"role":"memory","content":"private recall"},
            {"role":"assistant","content":"Answer without a heading","response_stats":{"output_tokens":32}}
        ]))
        .expect("valid SDK history");
        let (items, times) = Panel::history_items(history, vec![]);
        panel.user_times = times;
        *panel.items = items;
        panel
            .items
            .push(Item::Reasoning("private reasoning".into()));
        panel.items.push(Item::ProviderGuardrailRetry {
            turn_id: "fixture".into(),
            text: "private retry".into(),
        });
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-time-0").is_some());
    assert!(vcx.debug_bounds("tool-header").is_some());
    assert!(vcx.debug_bounds("restored-reasoning").is_some());
    let toggle = vcx
        .debug_bounds("clean-chat-toggle")
        .expect("the chat exposes its display toggle");
    vcx.simulate_click(toggle.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("restored-reasoning").is_none());
    assert!(vcx.debug_bounds("clean-chat-update").is_some());
    assert!(vcx.debug_bounds("clean-chat-final").is_some());
    let first = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let last = vcx.debug_bounds("selectable-text-5-0").unwrap();
    begin_drag(vcx, point(first.left() + px(0.1), first.center().y), false);
    assert_eq!(
        finish_drag_copy(vcx, point(last.right() - px(0.1), last.center().y)),
        "Real question\nPublic progress with no final marker\nAnswer without a heading"
    );
    assert!(vcx.debug_bounds("provider-guardrail-retry").is_none());
    assert!(vcx.debug_bounds("agent-message-sender").is_none());
    assert!(vcx.debug_bounds("prompt-time-0").is_none());
    assert!(vcx.debug_bounds("tool-header").is_none());
    panel.read_with(vcx, |panel, _| assert_eq!(panel.items.len(), 9));
}

#[gpui::test]
fn continuous_selection_spans_prompt_response_tool_and_final_answer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "continuous-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![
            Item::User("Prompt βeta".into()),
            Item::Assistant("First paragraph.\n\nSecond paragraph.".into()),
            Item::Tool {
                call_id: "call".into(),
                name: "bash".into(),
                input: r#"{"command":"printf hello"}"#.into(),
                output: "hidden output".into(),
                done: true,
                error: None,
            },
            Item::Assistant("Final answer.".into()),
        ];
        panel
    });
    vcx.run_until_parked();
    let first = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let last = vcx.debug_bounds("selectable-text-3-0").unwrap();
    // The tool name pill reads inline with its command.
    let expected =
        "Prompt βeta\nFirst paragraph.\nSecond paragraph.\nbash printf hello\nFinal answer.";
    begin_drag(vcx, point(first.left() + px(0.1), first.center().y), false);
    assert_eq!(
        finish_drag_copy(vcx, point(last.right() - px(0.1), last.center().y)),
        expected
    );
    panel.read_with(vcx, |panel, cx| {
        for (key, len) in [
            ("0-0", 12),
            ("1-0", 16),
            ("1-1", 17),
            ("tool-name-2", 4),
            ("tool-summary-2", 12),
            ("3-0", 13),
        ] {
            assert!(
                panel
                    .transcript_selection
                    .read(cx)
                    .highlight(key, len)
                    .is_some(),
                "missing highlight {key}"
            );
        }
    });
    begin_drag(vcx, point(last.right() - px(0.1), last.center().y), false);
    assert_eq!(
        finish_drag_copy(vcx, point(first.left() + px(0.1), first.center().y)),
        expected
    );
}

#[gpui::test]
fn continuous_selection_includes_expanded_tool_output_and_code(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let input = r#"{"command":"cargo check"}"#;
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "continuous-tool-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![
            Item::User("Check this".into()),
            Item::Tool {
                call_id: "call".into(),
                name: "bash".into(),
                input: input.into(),
                output: "Finished".into(),
                done: true,
                error: None,
            },
            Item::Assistant("```rust\nlet βeta = 1;\n```\n\nDone.".into()),
        ];
        panel.expanded_regions.insert("call".into());
        panel
    });
    vcx.run_until_parked();
    let first = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let last = vcx.debug_bounds("selectable-text-2-1").unwrap();
    begin_drag(vcx, point(first.left() + px(0.1), first.center().y), false);
    let copied = finish_drag_copy(vcx, point(last.right() - px(0.1), last.center().y));
    assert_eq!(
        copied,
        format!(
            "Check this\nbash cargo check\n{}\nlet βeta = 1;\nDone.",
            tool_detail("bash", input, "Finished")
        )
    );
    assert!(panel.read_with(vcx, |panel, _| panel.expanded_regions.contains("call")));
}

#[gpui::test]
fn continuous_selection_retains_anchor_and_unmounted_intermediate_rows(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "virtual-selection".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = (0..100)
            .map(|index| Item::Assistant(format!("Row {index} text")))
            .collect();
        panel.stick_to_bottom = false;
        panel
    });
    vcx.run_until_parked();
    let first = vcx.debug_bounds("selectable-text-0-0").unwrap();
    assert!(vcx.debug_bounds("selectable-text-50-0").is_none());
    begin_drag(vcx, point(first.left() + px(0.1), first.center().y), false);
    // Jumping the virtual viewport must not discard the anchor or omit rows
    // that have never been painted. This also covers shift-select after paging.
    panel.update(vcx, |panel, cx| {
        panel.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: 99,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("selectable-text-0-0").is_none());
    let last = vcx.debug_bounds("selectable-text-99-0").unwrap();
    let end = point(last.right() - px(0.1), last.center().y);
    let copied = finish_drag_copy(vcx, end);
    assert_eq!(
        copied,
        (0..100)
            .map(|index| format!("Row {index} text"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    panel.read_with(vcx, |panel, cx| {
        assert!(!panel.transcript_selection.read(cx).is_dragging())
    });
}
