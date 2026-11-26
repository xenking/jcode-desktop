use super::*;
use gpui::{Bounds, ListOffset, MouseButton, Pixels, VisualTestContext};

fn chat(cx: &mut gpui::TestAppContext) -> (Entity<Panel>, &mut VisualTestContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "overlay-parent".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.test_settled_conversation(60, cx);
        panel.model = Some("claude-opus-5-5".into());
        panel.provider = Some("anthropic".into());
        panel.reasoning_effort = Some("high".into());
        panel.available_models = vec!["anthropic:claude-opus-5-5".into()];
        panel
    });
    vcx.update(|_, cx| Panel::connect_input(&panel, cx));
    panel.update(vcx, |panel, cx| {
        panel.apply_event(
            &ApiEvent::RuntimeCommandResult {
                session_id: "overlay-parent".into(),
                command: jcode_sdk::api::RuntimeCommand::Skills,
                data: serde_json::json!({"pinned_skills": [], "skills": [
                    {"name":"selfdev", "description":"Development"}
                ]}),
                message: String::new(),
            },
            cx,
        );
    });
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(840.), px(700.)));
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.stick_to_bottom = false;
        panel.transcript_list.scroll_to(ListOffset {
            item_ix: 16,
            offset_in_item: px(4.),
        });
        cx.notify();
    });
    vcx.run_until_parked();
    (panel, vcx)
}

#[derive(Debug, PartialEq)]
struct ChatGeometry {
    viewport: Bounds<Pixels>,
    scroll: (usize, Pixels),
    dock: Bounds<Pixels>,
}

fn transcript(panel: &Entity<Panel>, cx: &mut VisualTestContext) -> ChatGeometry {
    let dock = cx.debug_bounds("composer-dock").expect("composer dock");
    panel.read_with(cx, |panel, _| {
        let top = panel.transcript_list.logical_scroll_top();
        ChatGeometry {
            viewport: panel.transcript_list.viewport_bounds(),
            scroll: (top.item_ix, top.offset_in_item),
            dock,
        }
    })
}

fn open_side_question(panel: &Entity<Panel>, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| {
        let session = serde_json::from_value(serde_json::json!({
            "session_id": "overlay-child", "status": "idle", "context": null
        }))
        .unwrap();
        panel.start_side_question(session, "What did the parent do?".into(), cx);
    });
    cx.run_until_parked();
}

fn drag_side_question(cx: &mut VisualTestContext, delta_y: f32) {
    let position = cx.debug_bounds("side-question-resize").unwrap().center();
    cx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position,
        click_count: 1,
        ..Default::default()
    });
    let position = position + point(px(0.), px(delta_y));
    cx.simulate_event(gpui::MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        ..Default::default()
    });
    cx.simulate_event(gpui::MouseUpEvent {
        button: MouseButton::Left,
        position,
        click_count: 1,
        ..Default::default()
    });
    cx.run_until_parked();
}

#[gpui::test]
fn side_question_overlays_transcript_and_remembers_drag_height(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = chat(cx);
    let baseline = transcript(&panel, vcx);
    assert!(baseline.scroll.0 > 0, "history is scrolled");

    open_side_question(&panel, vcx);
    assert!(vcx.debug_bounds("side-question-follow-up").is_some());
    assert_eq!(
        transcript(&panel, vcx),
        baseline,
        "opening /btw moves the chat"
    );

    drag_side_question(vcx, -2_000.);
    let full = vcx.debug_bounds("side-question-card").unwrap();
    assert!((full.top() - baseline.viewport.top()).abs() <= px(1.));
    assert!(full.bottom() <= vcx.debug_bounds("composer").unwrap().top());
    assert_eq!(
        transcript(&panel, vcx),
        baseline,
        "maximizing moves the chat"
    );

    drag_side_question(vcx, 2_000.);
    let collapsed = vcx.debug_bounds("side-question-card").unwrap();
    assert!(collapsed.size.height < px(60.));
    assert_eq!(collapsed.bottom(), full.bottom());
    assert!(vcx.debug_bounds("side-question-turns").is_none());
    assert!(vcx.debug_bounds("side-question-follow-up").is_none());
    for selector in [
        "side-question-history-open",
        "side-question-copy",
        "side-question-promote",
        "side-question-dismiss",
    ] {
        assert!(collapsed.contains(&vcx.debug_bounds(selector).unwrap().center()));
    }
    assert_eq!(
        transcript(&panel, vcx),
        baseline,
        "collapsing moves the chat"
    );

    let close = vcx.debug_bounds("side-question-dismiss").unwrap().center();
    vcx.simulate_click(close, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("side-question-card").is_none());
    assert_eq!(
        transcript(&panel, vcx),
        baseline,
        "closing /btw moves the chat"
    );
    open_side_question(&panel, vcx);
    assert_eq!(vcx.debug_bounds("side-question-card"), Some(collapsed));
    assert!(vcx.debug_bounds("side-question-follow-up").is_none());
}

#[gpui::test]
fn composer_picker_inputs_and_lists_overlay_without_moving_the_chat(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = chat(cx);
    let baseline = transcript(&panel, vcx);
    let editor = vcx.debug_bounds("composer-text-slot").unwrap();
    for selector in ["panel-model", "panel-model-effort", "panel-mode"] {
        let trigger = vcx.debug_bounds(selector).unwrap().center();
        vcx.simulate_click(trigger, Default::default());
        vcx.run_until_parked();
        let search = vcx.debug_bounds("picker-search").unwrap();
        let rows = vcx.debug_bounds("slash-command-overlay").unwrap();
        assert!(
            search.bottom() <= editor.top(),
            "{selector} search covers the editor"
        );
        assert!(
            rows.bottom() <= search.top(),
            "{selector} results cover search"
        );
        assert_eq!(vcx.debug_bounds("composer-text-slot"), Some(editor));
        assert_eq!(
            transcript(&panel, vcx),
            baseline,
            "{selector} moves the chat"
        );
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("picker-search").is_none());
        assert_eq!(transcript(&panel, vcx), baseline);
    }

    vcx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_provider_picker(window, cx));
    });
    vcx.run_until_parked();
    let provider = vcx.debug_bounds("composer-provider-picker").unwrap();
    assert!(provider.bottom() <= editor.top());
    assert!(vcx.debug_bounds("provider-picker-search").is_some());
    assert_eq!(transcript(&panel, vcx), baseline);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();

    vcx.simulate_click(editor.center(), Default::default());
    vcx.simulate_input("/help");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").unwrap().bottom() <= editor.top());
    assert_eq!(
        transcript(&panel, vcx),
        baseline,
        "the slash palette moves the chat"
    );
}
