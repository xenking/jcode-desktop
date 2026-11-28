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

fn cursor(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> (usize, usize) {
    card(panel, vcx, |card| {
        (card.cursor_question, card.cursor_option)
    })
}

fn composer_focused(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> bool {
    vcx.update(|window, cx| panel.read(cx).input.read(cx).is_focused(window))
}

const AB: [(&str, &str); 3] = [("A", ""), ("B (Recommended)", ""), ("C", "")];

#[gpui::test]
fn arrows_walk_the_options_and_the_own_answer_row_and_stop_at_the_ends(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    assert_eq!(
        cursor(&panel, vcx),
        (0, 1),
        "the recommended option starts highlighted"
    );
    for (keys, expected) in [
        ("up", 0),
        ("up", 0),
        ("down", 1),
        ("down", 2),
        ("down", 3),
        ("down", 3),
    ] {
        vcx.simulate_keystrokes(keys);
        assert_eq!(cursor(&panel, vcx).1, expected, "after {keys}");
    }
    assert!(
        card(&panel, vcx, |card| card.on_own_answer()),
        "the row after the last option is the own-answer row"
    );
    assert!(
        card(&panel, vcx, |card| card.chosen[0].is_empty()),
        "moving the highlight picks nothing"
    );
    // Keys the reference does not have do nothing.
    for keys in ["j", "k", "home", "end"] {
        vcx.simulate_keystrokes(keys);
        assert_eq!(cursor(&panel, vcx), (0, 3), "{keys} moves nothing");
    }
}

#[gpui::test]
fn arrows_cross_from_one_question_to_the_next_and_on_to_the_review_rows(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(
        cx,
        None,
        vec![
            question(false, &AB),
            question(false, &[("X", ""), ("Y", "")]),
        ],
    );
    vcx.simulate_keystrokes("down down");
    assert_eq!(cursor(&panel, vcx), (0, 3));
    vcx.simulate_keystrokes("down");
    assert_eq!(
        cursor(&panel, vcx),
        (1, 0),
        "down off the last row opens the next question"
    );
    vcx.simulate_keystrokes("up");
    assert_eq!(
        cursor(&panel, vcx),
        (0, 3),
        "up off the first row returns to the last row"
    );
    vcx.simulate_keystrokes("down down down");
    assert_eq!(
        cursor(&panel, vcx),
        (1, 2),
        "the second question's own-answer row"
    );
    vcx.simulate_keystrokes("down");
    assert_eq!(
        cursor(&panel, vcx),
        (2, 0),
        "the review's first row, one row per question"
    );
    vcx.simulate_keystrokes("down down down");
    assert_eq!(
        cursor(&panel, vcx),
        (2, 2),
        "then its submit stop, where the card ends"
    );
    vcx.simulate_keystrokes("up up");
    assert_eq!(cursor(&panel, vcx), (2, 0));
    vcx.simulate_keystrokes("up");
    assert_eq!(
        cursor(&panel, vcx),
        (1, 2),
        "up off the first review row returns to the questions"
    );
}

#[gpui::test]
fn left_right_and_tab_walk_the_tabs_without_wrapping_and_the_last_tab_hands_the_keys_on(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB), question(false, &AB)]);
    let tab = |vcx: &mut gpui::VisualTestContext| cursor(&panel, vcx).0;
    for (keys, expected) in [
        ("left", 0),
        ("right", 1),
        ("tab", 2),
        ("shift-tab", 1),
        ("shift-tab", 0),
        ("shift-tab", 0),
        ("right right", 2),
        ("left", 1),
    ] {
        vcx.simulate_keystrokes(keys);
        assert_eq!(tab(vcx), expected, "after {keys}");
    }
    assert!(!composer_focused(&panel, vcx));
    vcx.simulate_keystrokes("right");
    assert_eq!(tab(vcx), 2, "right on the review has nowhere to go");
    assert!(
        !composer_focused(&panel, vcx),
        "right never leaves the card"
    );
    vcx.simulate_keystrokes("tab");
    assert!(
        composer_focused(&panel, vcx),
        "tab on the last tab hands the keys to the composer"
    );
    assert!(
        panel.read_with(vcx, |panel, _| panel.ask_card.is_some()),
        "the card stays pending"
    );
}

#[gpui::test]
fn enter_picks_the_highlighted_row_and_moves_on(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(
        cx,
        None,
        vec![
            question(false, &AB),
            question(true, &AB),
            question(true, &AB),
        ],
    );
    assert!(card(&panel, vcx, |card| card.chosen[0].is_empty()));
    vcx.simulate_keystrokes("enter");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[0].clone()),
        [1],
        "nothing chosen: Enter takes the highlighted row, it does not skip"
    );
    assert_eq!(
        cursor(&panel, vcx).0,
        1,
        "and the card moves to the next question"
    );
    vcx.simulate_keystrokes("down enter");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[1].clone()),
        [2],
        "a multi-select with nothing chosen takes the highlighted row"
    );
    vcx.simulate_keystrokes("1 enter");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[2].clone()),
        [0],
        "a multi-select that has an answer keeps it: Enter only moves on"
    );
    assert_eq!(
        cursor(&panel, vcx).0,
        3,
        "past the last question is the review"
    );
    // Going back and pressing Enter on another row of a single-select replaces its answer.
    vcx.simulate_keystrokes("left left left down enter");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[0].clone()),
        [2],
        "Enter on another row of a single-select picks that row"
    );
}

#[gpui::test]
fn enter_on_the_own_answer_row_opens_its_field_and_escape_comes_back(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    vcx.simulate_keystrokes("down down enter");
    assert!(
        editing(&panel, vcx),
        "Enter on the own-answer row opens the field"
    );
    // Digits are typing there, not picks.
    vcx.simulate_input("2");
    assert_eq!(
        card(&panel, vcx, |card| card.other[0].clone()).as_deref(),
        Some("2")
    );
    assert!(card(&panel, vcx, |card| card.chosen[0].is_empty()));
    vcx.simulate_keystrokes("escape");
    assert!(
        !editing(&panel, vcx),
        "Escape in the field returns to the options"
    );
    assert!(
        !composer_focused(&panel, vcx),
        "and does not leave the card"
    );
    vcx.simulate_keystrokes("enter");
    vcx.simulate_keystrokes("shift-tab");
    assert!(
        !editing(&panel, vcx),
        "Shift+Tab returns to the options too"
    );
    vcx.simulate_keystrokes("1");
    assert_eq!(
        card(&panel, vcx, |card| card.chosen[0].clone()),
        [0],
        "the options take number keys again"
    );
}

#[gpui::test]
fn tab_in_the_own_answer_field_goes_on_to_the_next_question(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB), question(false, &AB)]);
    vcx.simulate_keystrokes("down down enter");
    assert!(editing(&panel, vcx));
    vcx.simulate_keystrokes("tab");
    assert!(!editing(&panel, vcx));
    assert_eq!(cursor(&panel, vcx).0, 1);
}

#[gpui::test]
fn escape_hands_the_keys_to_the_composer_and_the_card_stays_pending(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    vcx.simulate_keystrokes("escape");
    assert!(composer_focused(&panel, vcx), "the composer has the keys");
    vcx.run_until_parked();
    assert!(
        composer_focused(&panel, vcx),
        "the card does not take them straight back"
    );
    assert!(
        panel.read_with(vcx, |panel, _| panel
            .ask_card
            .as_ref()
            .is_some_and(|card| !card.submitting())),
        "Escape answers nothing: the card is still waiting"
    );
    assert!(
        vcx.debug_bounds("ask-confirm-decline").is_none(),
        "and asks nothing"
    );
    // Clicking the card gives it the keys again.
    let row = vcx.debug_bounds("ask-option-0-2").unwrap().center();
    vcx.simulate_click(row, Default::default());
    vcx.run_until_parked();
    vcx.simulate_keystrokes("up");
    assert_eq!(cursor(&panel, vcx), (0, 1));
}

#[gpui::test]
fn the_review_rows_take_enter_to_reopen_a_question_or_send_the_answers(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = open(
        cx,
        Some(bridge),
        vec![question(false, &AB), question(false, &AB)],
    );
    vcx.simulate_keystrokes("1 right 3 right");
    assert_eq!(
        cursor(&panel, vcx),
        (2, 2),
        "the review opens on its submit stop"
    );
    vcx.simulate_keystrokes("up up enter");
    assert_eq!(
        cursor(&panel, vcx).0,
        0,
        "Enter on a review row reopens that question"
    );
    vcx.simulate_keystrokes("right right down down enter");
    vcx.run_until_parked();
    let sent = commands.try_iter().find_map(|command| match command {
        crate::harness::Command::AskResponse { response, .. } => Some(response),
        _ => None,
    });
    let answers = sent
        .and_then(|response| response.answers)
        .expect("Enter on the submit stop sent the answers");
    assert_eq!(answers[0].selected, ["A"]);
    assert_eq!(answers[1].selected, ["C"]);
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

/// The room between the last thing in the card's body and the answer row under it.
fn gap_above_reply_row(vcx: &mut gpui::VisualTestContext, last: &'static str) -> gpui::Pixels {
    vcx.debug_bounds("ask-reply-row").unwrap().top() - vcx.debug_bounds(last).unwrap().bottom()
}

const A_FEW_PIXELS: gpui::Pixels = px(16.);

#[gpui::test]
fn the_card_is_as_tall_as_its_content_with_one_question_two_and_the_review(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    let window = vcx.update(|window, _| window.viewport_size().height);
    assert!(card_height(vcx) < window * 0.4, "short content, short card");
    assert!(
        gap_above_reply_row(vcx, "ask-option-0-2") < A_FEW_PIXELS,
        "nothing blank between the options and the answer row"
    );
    show(
        &panel,
        vcx,
        "ask-2",
        vec![question(false, &AB), question(false, &AB[..2])],
    );
    vcx.simulate_keystrokes("1 enter");
    settle(&panel, vcx, "ask-1");
    let two = card_height(vcx);
    vcx.simulate_keystrokes("right right");
    let review = card_height(vcx);
    assert!(
        gap_above_reply_row(vcx, "ask-review-row-1") < A_FEW_PIXELS,
        "nothing blank under the review rows"
    );
    assert!(
        review < two + px(80.),
        "the review does not keep the questions' room: {two:?} then {review:?}"
    );
}

#[gpui::test]
fn a_height_the_user_dragged_taller_than_the_content_adds_no_blank_area(
    cx: &mut gpui::TestAppContext,
) {
    let (_panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    let content = card_height(vcx);
    drag_handle(vcx, -300.);
    assert!(
        (card_height(vcx) - content).abs() <= px(1.),
        "dragging past the content does not stretch the card"
    );
    assert!(gap_above_reply_row(vcx, "ask-option-0-2") < A_FEW_PIXELS);
}

#[gpui::test]
fn a_remembered_height_never_exceeds_what_the_next_card_needs(cx: &mut gpui::TestAppContext) {
    let mut tall = question(false, &REGIONS);
    tall["question"] = "Paragraph of rich context.\n\n".repeat(12).into();
    let (panel, vcx) = open(cx, None, vec![tall]);
    drag_handle(vcx, -200.);
    let dragged = card_height(vcx);
    show(&panel, vcx, "ask-2", vec![question(false, &AB[..2])]);
    vcx.simulate_keystrokes("1 enter");
    settle(&panel, vcx, "ask-1");
    assert!(
        card_height(vcx) < dragged - px(100.),
        "the short question takes only its own height, not the {dragged:?} remembered"
    );
    assert!(gap_above_reply_row(vcx, "ask-option-0-1") < A_FEW_PIXELS);
}

#[gpui::test]
fn a_drag_below_the_content_height_caps_the_card_and_the_content_scrolls(
    cx: &mut gpui::TestAppContext,
) {
    let (_panel, vcx) = open(cx, None, vec![question(false, &REGIONS)]);
    let full = card_height(vcx);
    drag_handle(vcx, 120.);
    let capped = card_height(vcx);
    assert!(
        capped < full - px(60.),
        "dragging down shortens the card: {full:?} -> {capped:?}"
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
fn a_long_description_shows_in_full_under_its_label_with_no_expand_control(
    cx: &mut gpui::TestAppContext,
) {
    let long = "A description far too long to share a line with its label in a narrow card, \
                so the row has to wrap it onto more lines instead of cutting it short";
    let (panel, vcx) = open(cx, None, vec![question(false, &[("Option", long)])]);
    vcx.simulate_resize(gpui::size(px(360.), px(700.)));
    vcx.run_until_parked();
    let line = card(&panel, vcx, |card| {
        card.label_layouts.0.borrow()[&(0, 0)].line_height()
    });
    assert!(
        vcx.debug_bounds("ask-description-0-0").unwrap().size.height > line * 2.,
        "the whole description is there, wrapped"
    );
    assert!(
        vcx.debug_bounds("ask-expand-button-0-0").is_none(),
        "there is nothing to expand"
    );
    vcx.simulate_keystrokes("e");
    assert_eq!(
        cursor(&panel, vcx),
        (0, 0),
        "the `e` key is not a key of the card"
    );
}

#[gpui::test]
fn the_card_reads_at_the_chat_text_size_and_has_no_key_hint_line(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open(
        cx,
        None,
        vec![question(false, &[("Option", "A description")])],
    );
    let size = card(&panel, vcx, |card| {
        let layouts = card.label_layouts.0.borrow();
        layouts[&(0, 0)].line_layouts()[0]
            .unwrapped_layout
            .font_size
    });
    assert_eq!(size, px(crate::markdown::CHAT_TEXT_SIZE));
    assert!(vcx.debug_bounds("ask-hint").is_none());
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

#[gpui::test]
fn arrows_still_move_the_highlight_after_clicking_the_questions_text(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB)]);
    let text = vcx.debug_bounds("ask-question-0").unwrap().center();
    vcx.simulate_click(text, Default::default());
    vcx.run_until_parked();
    let before = cursor(&panel, vcx);
    vcx.simulate_keystrokes("down");
    assert_ne!(
        cursor(&panel, vcx),
        before,
        "the card still takes the arrows"
    );
}

#[gpui::test]
fn arrows_still_work_after_the_transcript_scrolls_or_streams_and_after_the_first_answer(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = open(cx, None, vec![question(false, &AB), question(false, &AB)]);
    vcx.simulate_keystrokes("down");
    let transcript = vcx.debug_bounds("transcript").unwrap().center();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: transcript,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-120.))),
        ..Default::default()
    });
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.items.push(Item::Assistant("streaming text".into()));
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("up");
    assert_eq!(
        cursor(&panel, vcx),
        (0, 1),
        "after a scroll and a streaming update"
    );
    vcx.simulate_keystrokes("enter");
    assert_eq!(cursor(&panel, vcx).0, 1);
    vcx.simulate_keystrokes("down");
    assert_eq!(
        cursor(&panel, vcx),
        (1, 2),
        "after answering the first question"
    );
    vcx.simulate_keystrokes("right");
    assert_eq!(cursor(&panel, vcx).0, 2);
    vcx.simulate_keystrokes("up");
    assert_eq!(cursor(&panel, vcx), (2, 1), "on the review");
}

#[gpui::test]
fn the_second_ask_of_a_session_takes_the_arrows_from_the_composer(cx: &mut gpui::TestAppContext) {
    let one = question(false, &AB);
    let (panel, vcx) = open(cx, None, vec![one.clone()]);
    vcx.simulate_keystrokes("1 enter");
    settle(&panel, vcx, "ask-1");
    vcx.update(|window, cx| {
        let input = panel.read(cx).input_focus_handle(cx);
        window.focus(&input, cx);
    });
    show(&panel, vcx, "ask-2", vec![one]);
    vcx.simulate_keystrokes("down");
    assert_eq!(cursor(&panel, vcx), (0, 2));
}
