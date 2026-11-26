//! The ask card's keys, size and focus, as the user meets them.
use super::*;
use serde_json::{Value, json};

fn options(labels: &[(&str, &str)]) -> Vec<Value> {
    labels
        .iter()
        .map(|(label, description)| {
            if description.is_empty() {
                json!({"label": label})
            } else {
                json!({"label": label, "description": description})
            }
        })
        .collect()
}

fn question(multi: bool, labels: &[(&str, &str)]) -> Value {
    json!({"question": "Which?", "multi_select": multi, "options": options(labels)})
}

fn show(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext, id: &str, questions: Vec<Value>) {
    let event = json!({"ev": "ask_request", "session_id": "session-a", "request_id": id,
        "tool_call_id": format!("tool-{id}"), "questions": questions});
    panel.update(vcx, |panel, cx| {
        panel.apply(&serde_json::from_value(event).unwrap(), cx)
    });
    vcx.run_until_parked();
}

/// A chat panel with `questions` open on the card and the keyboard on it.
fn open(
    cx: &mut gpui::TestAppContext,
    bridge: Option<crate::harness::Bridge>,
    questions: Vec<Value>,
) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        if let Some(bridge) = bridge {
            workspace.set_test_bridge(bridge);
        }
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    vcx.run_until_parked();
    show(&panel, vcx, "ask-1", questions);
    focus_card(&panel, vcx);
    (panel, vcx)
}

fn focus_card(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) {
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
}

fn card<R>(
    panel: &Entity<Panel>,
    vcx: &gpui::VisualTestContext,
    f: impl FnOnce(&AskCard) -> R,
) -> R {
    panel.read_with(vcx, |panel, _| {
        f(panel.ask_card.as_ref().expect("an ask card"))
    })
}

fn editing(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> bool {
    vcx.update(|window, cx| {
        panel
            .read(cx)
            .ask_card
            .as_ref()
            .is_some_and(|card| card.focused_editor(window, cx).is_some())
    })
}

fn card_height(vcx: &mut gpui::VisualTestContext) -> gpui::Pixels {
    vcx.debug_bounds("ask-card")
        .expect("the card paints")
        .size
        .height
}

const REGIONS: [(&str, &str); 12] = [
    ("us-east", ""),
    ("us-west", ""),
    ("eu-west", ""),
    ("eu-central", ""),
    ("ap-south", ""),
    ("ap-northeast", ""),
    ("sa-east", ""),
    ("af-south", ""),
    ("me-central", ""),
    ("ca-central", ""),
    ("ap-southeast", ""),
    ("eu-north", ""),
];

#[gpui::test]
fn number_keys_pick_the_first_nine_options_and_ignore_the_rest(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(cx, None, vec![question(false, &REGIONS)]);
    vcx.simulate_keystrokes("9");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [8]);
    vcx.simulate_keystrokes("1");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [0]);
    // There is no key for a tenth row: neither 0 nor 1 then 0 reaches it.
    vcx.simulate_keystrokes("0");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [0]);
    let labels = card(&panel, vcx, |card| {
        let layouts = card.label_layouts.0.borrow();
        (
            layouts[&(0, 8)].wrapped_text(),
            layouts[&(0, 9)].wrapped_text(),
        )
    });
    assert_eq!(labels.0, "9. me-central");
    assert_eq!(
        labels.1, "ca-central",
        "options past the ninth carry no number"
    );
}

#[gpui::test]
fn a_number_key_toggles_in_a_multi_select_and_always_picks_in_a_single_select(
    cx: &mut gpui::TestAppContext,
) {
    let labels = [("A", ""), ("B", ""), ("C", "")];
    let (panel, vcx) = open(
        cx,
        None,
        vec![question(true, &labels), question(false, &labels)],
    );
    vcx.simulate_keystrokes("1 3 1");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [2]);
    vcx.simulate_keystrokes("right 2 2");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[1].clone()),
        [1],
        "picking the picked option again keeps it"
    );
}

#[gpui::test]
fn arrows_wrap_and_start_on_the_recommended_option(cx: &mut gpui::TestAppContext) {
    let labels = [("A", ""), ("B (Recommended)", ""), ("C", ""), ("D", "")];
    let (panel, vcx) = open(cx, None, vec![question(false, &labels)]);
    let cursor = |vcx: &mut gpui::VisualTestContext| card(&panel, vcx, |card| card.cursor_option);
    assert_eq!(cursor(vcx), 1, "the recommended option starts highlighted");
    for (keys, expected) in [
        ("up", 0),
        ("up", 3),
        ("down", 0),
        ("end", 3),
        ("home", 0),
        ("j", 1),
        ("k", 0),
    ] {
        vcx.simulate_keystrokes(keys);
        assert_eq!(cursor(vcx), expected, "after {keys}");
    }
    assert!(
        card(&panel, vcx, |card| card.chosen[0].is_empty()),
        "moving the highlight picks nothing"
    );
}

#[gpui::test]
fn tab_reaches_the_free_text_field_and_shift_tab_or_escape_return(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(cx, None, vec![question(false, &[("A", ""), ("B", "")])]);
    assert!(!editing(&panel, vcx));
    vcx.simulate_keystrokes("tab");
    assert!(editing(&panel, vcx), "Tab moves to the free-text field");
    // Digits are typing there, not picks.
    vcx.simulate_input("2");
    assert_eq!(
        card(&panel, vcx, |card| card.other[0].clone()).as_deref(),
        Some("2")
    );
    assert!(card(&panel, vcx, |card| card.chosen[0].is_empty()));
    vcx.simulate_keystrokes("shift-tab");
    assert!(!editing(&panel, vcx), "Shift+Tab returns to the options");
    assert!(panel.read_with(vcx, |panel, _| panel.ask_card.is_some()));
    vcx.simulate_keystrokes("tab escape");
    assert!(
        !editing(&panel, vcx),
        "Escape in the field returns to the options"
    );
    assert!(
        !card(&panel, vcx, |card| card.confirming_decline),
        "Escape in the field is not a dismissal"
    );
    vcx.simulate_keystrokes("2");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[0].clone()),
        [1],
        "the options take number keys again"
    );
}

#[gpui::test]
fn the_expand_key_opens_only_descriptions_that_need_it(cx: &mut gpui::TestAppContext) {
    let labels = [
        ("Short", "Fits on a line"),
        ("Long", "Two steps\n\n- first\n- second"),
    ];
    let (panel, vcx) = open(cx, None, vec![question(false, &labels)]);
    let open_descriptions = |vcx: &mut gpui::VisualTestContext| {
        card(&panel, vcx, |card| {
            let mut open: Vec<_> = card.expanded.iter().copied().collect();
            open.sort_unstable();
            open
        })
    };
    vcx.simulate_keystrokes("e");
    assert!(
        open_descriptions(vcx).is_empty(),
        "a one-line description has nothing to expand"
    );
    vcx.simulate_keystrokes("down e");
    assert_eq!(open_descriptions(vcx), [(0, 1)]);
    let expanded = vcx.debug_bounds("ask-description-0-1").unwrap().size.height;
    vcx.simulate_keystrokes("e");
    assert!(open_descriptions(vcx).is_empty());
    assert!(vcx.debug_bounds("ask-description-0-1").unwrap().size.height < expanded);
}

fn drag_handle(vcx: &mut gpui::VisualTestContext, delta_y: f32) {
    let start = vcx.debug_bounds("ask-resize").unwrap().center();
    let end = gpui::point(start.x, start.y + px(delta_y));
    vcx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    vcx.simulate_mouse_move(end, gpui::MouseButton::Left, Default::default());
    vcx.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
    vcx.run_until_parked();
}

fn settle(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext, id: &str) {
    panel.update(vcx, |panel, cx| {
        let attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
        panel.ask_response_settled(id, attempt, Ok(()), cx);
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn a_dragged_height_holds_for_the_next_question_and_double_click_resets_it(
    cx: &mut gpui::TestAppContext,
) {
    let one = question(false, &[("A", ""), ("B", "")]);
    let (panel, vcx) = open(cx, None, vec![one.clone()]);
    show(&panel, vcx, "ask-2", vec![one]);
    let automatic = card_height(vcx);
    drag_handle(vcx, -80.);
    let dragged = card_height(vcx);
    assert!(
        dragged > automatic + px(60.),
        "dragging up grows the card: {automatic:?} -> {dragged:?}"
    );
    focus_card(&panel, vcx);
    vcx.simulate_keystrokes("1 enter");
    settle(&panel, vcx, "ask-1");
    assert_eq!(
        card(&panel, vcx, |card| card.request_id.clone()),
        "ask-2",
        "the queued question took the card"
    );
    assert!(
        (card_height(vcx) - dragged).abs() <= px(1.),
        "the next question keeps the size the user chose"
    );
    let handle = vcx.debug_bounds("ask-resize").unwrap().center();
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: handle,
        click_count: 2,
        ..Default::default()
    });
    vcx.run_until_parked();
    assert!(
        (card_height(vcx) - automatic).abs() <= px(1.),
        "double click returns to automatic height"
    );
}

#[gpui::test]
fn the_card_never_covers_the_transcript_by_default_and_a_drag_stays_inside_the_window(
    cx: &mut gpui::TestAppContext,
) {
    let long = "Paragraph of rich context.\n\n".repeat(30);
    let mut tall = question(false, &REGIONS);
    tall["question"] = long.into();
    let (panel, vcx) = open(cx, None, vec![tall]);
    let window = vcx.update(|window, _| window.viewport_size().height);
    assert!(
        card_height(vcx) <= window * 0.45 + px(2.),
        "default {:?} of {window:?}",
        card_height(vcx)
    );
    drag_handle(vcx, -5_000.);
    let maximum = card_height(vcx);
    assert!(
        maximum > window * 0.6,
        "a drag can take it taller: {maximum:?}"
    );
    assert!(
        maximum <= window * 0.8 + px(2.),
        "but not past the window: {maximum:?}"
    );
    drag_handle(vcx, 5_000.);
    assert!(
        card_height(vcx) <= window * 0.45 + px(2.),
        "dragging back down returns to automatic"
    );
    assert!(panel.read_with(vcx, |panel, _| panel.ask_card.is_some()));
}

#[gpui::test]
fn the_keyboard_stays_on_the_card_after_an_answer(cx: &mut gpui::TestAppContext) {
    let one = question(false, &[("A", ""), ("B", "")]);
    let (panel, vcx) = open(cx, None, vec![one.clone()]);
    show(&panel, vcx, "ask-2", vec![one]);
    vcx.simulate_keystrokes("1 enter");
    settle(&panel, vcx, "ask-1");
    vcx.run_until_parked();
    assert!(
        vcx.update(|window, cx| panel.read(cx).focus_handle.is_focused(window)),
        "focus is still on the panel, so the next card takes keys"
    );
    vcx.simulate_keystrokes("2");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [1]);
}

#[gpui::test]
fn an_mcp_approval_card_answers_with_the_same_keys(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let approval = json!({"header": "MCP approval",
    "question": "Allow MCP tool \"write-item\" on server \"fixture\" with arguments {\"n\":1}?",
    "options": options(&[
        ("Deny", "Do not run this call"),
        ("Allow once", "Run this one call only"),
        ("Always allow this tool", "Run it now and never ask for this tool again"),
        ("Always allow every tool on this server", "Run it now and never ask for this server again"),
    ])});
    let (_panel, vcx) = open(cx, Some(bridge), vec![approval]);
    vcx.simulate_keystrokes("2 enter");
    vcx.run_until_parked();
    let sent = commands.try_iter().find_map(|command| match command {
        crate::harness::Command::AskResponse { response, .. } => Some(response),
        _ => None,
    });
    let answers = sent
        .and_then(|response| response.answers)
        .expect("an answer was sent");
    assert_eq!(answers[0].selected, ["Allow once"]);
}

#[gpui::test]
fn a_collapsed_description_that_does_not_fit_ends_in_an_ellipsis(cx: &mut gpui::TestAppContext) {
    let long = "A description far too long to share a line with its label in a narrow card, \
                so the row has to cut it short";
    let (panel, vcx) = open(cx, None, vec![question(false, &[("Option", long)])]);
    vcx.simulate_resize(gpui::size(px(360.), px(700.)));
    vcx.run_until_parked();
    let shown = card(&panel, vcx, |card| {
        card.label_layouts.0.borrow()[&(0, DESCRIPTION_LAYOUT)].wrapped_text()
    });
    assert!(shown.ends_with('…'), "cut without a mark: {shown:?}");
    assert!(!shown.contains('\n'), "one line: {shown:?}");
}

#[gpui::test]
fn the_first_card_of_a_new_session_takes_the_keys_from_the_composer(cx: &mut gpui::TestAppContext) {
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
    // The first prompt of a new session was sent: its row exists, the welcome layout has not settled.
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("hello".into())];
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        panel.read_with(vcx, |panel, _| panel.startup_layout.is_some()),
        "the welcome layout is still in place"
    );
    vcx.update(|window, cx| {
        let input = panel.read(cx).input_focus_handle(cx);
        window.focus(&input, cx);
    });
    show(
        &panel,
        vcx,
        "ask-1",
        vec![question(false, &[("A", ""), ("B", "")])],
    );
    vcx.run_until_parked();
    vcx.simulate_keystrokes("2");
    assert_eq!(card(&panel, vcx, |card| card.chosen[0].clone()), [1]);
}
