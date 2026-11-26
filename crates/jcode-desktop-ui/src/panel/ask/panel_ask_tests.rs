use super::*;
use crate::harness::AskResponseError;
use jcode_sdk::AskOption;

fn settle_test_ask(
    panel: &mut Panel,
    request_id: &str,
    result: Result<(), AskResponseError>,
    cx: &mut Context<Panel>,
) {
    let attempt_id = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
    panel.ask_response_settled(request_id, attempt_id, result, cx);
}

fn question(multi: bool) -> AskQuestion {
    AskQuestion {
        question: "Q?".into(),
        header: None,
        options: vec![
            AskOption {
                label: "A".into(),
                description: None,
            },
            AskOption {
                label: "B".into(),
                description: None,
            },
        ],
        multi_select: multi,
        allow_other: true,
    }
}

#[test]
fn repeated_header_is_removed_only_at_a_boundary() {
    assert_eq!(
        question_without_header("Sidebar: what works?", Some("Sidebar")),
        "what works?"
    );
    assert_eq!(
        question_without_header("Sidebar layout?", Some("Sidebar")),
        "Sidebar layout?"
    );
    assert_eq!(
        question_without_header("Sidebar: ", Some("Sidebar")),
        "Sidebar: "
    );
}

#[test]
fn single_select_replaces_and_other_clears_choice() {
    let mut card = AskCard::new("r".into(), vec![question(false), question(true)]);
    card.toggle(0, 0);
    card.toggle(0, 1);
    assert_eq!(card.chosen[0], vec![1]);
    assert_eq!(card.response().answers.unwrap()[1], AskAnswer::default());
    card.toggle(1, 1);
    card.toggle(1, 0);
    card.set_other(1, " extra ");
    let answers = card.response().answers.unwrap();
    assert_eq!(answers[0].selected, vec!["B"]);
    assert_eq!(answers[1].selected, vec!["A", "B"]);
    assert_eq!(answers[1].other.as_deref(), Some("extra"));
    card.set_other(0, "mine");
    assert!(card.chosen[0].is_empty());
}

/// The wire frame the Desktop harness emits for the real `ask` call a model
/// made in an isolated jcode session (question text and options verbatim
/// from the session journal). Proves the card paints and answers end to end.
const REAL_ASK_REQUEST: &str = r#"{
    "ev": "ask_request", "session_id": "session-a",
    "request_id": "ask-toolu_01J29QR2NCg12HueHJkMWtZU",
    "tool_call_id": "toolu_01J29QR2NCg12HueHJkMWtZU",
    "questions": [{"header": "Database", "question": "Which database should the new web service use?",
        "options": [
            {"description": "Relational, scalable, rich features", "label": "Postgres"},
            {"description": "Embedded, zero-ops, single node", "label": "SQLite"},
            {"description": "Managed NoSQL on AWS", "label": "DynamoDB"}]}]
}"#;

/// A long option description (the real ask that collapsed: each label was
/// painted one character per line beside an empty spacer).
#[gpui::test]
fn long_option_text_keeps_the_row_width(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    // Verbatim from the session journal of the ask that collapsed.
    let value = serde_json::json!({
        "ev": "ask_request", "session_id": "session-a", "request_id": "ask-1",
        "tool_call_id": "toolu_1",
        "questions": [{"header": "Старый кеш", "options": [
            {"description": "Новая версия этот файл не использует, данные в общей базе.", "label": "Удалить (Recommended)"},
            {"description": "Файл останется на диске, приложение его не трогает.", "label": "Оставить"}],
          "question": "Удалить старый кеш ~/Library/Application Support/ReddyMac/90813.sqlite (около 2 МБ)? Новая версия его не читает: все данные теперь в ~/.local/share/reddy/history.sqlite3. Восстановить после удаления нельзя, но это только копия того, что есть на сервере."}]
    });
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_value(value).unwrap(), cx);
    });
    vcx.run_until_parked();
    let row = vcx.debug_bounds("ask-option-0-0").unwrap();
    let label = vcx.debug_bounds("ask-label-0-0").unwrap();
    let description = vcx.debug_bounds("ask-description-0-0").unwrap();
    let comment = vcx.debug_bounds("ask-comment-button-0-0").unwrap();
    let radio = vcx.debug_bounds("ask-mark-0-0").unwrap();
    assert!(
        description.size.width > px(150.),
        "the description takes the rest of the line: {:?} of {:?}",
        description.size.width,
        row.size.width
    );
    assert!(label.size.height < px(40.), "the label stays on one line");
    assert!(
        description.size.height < px(40.),
        "the description stays on one line"
    );
    assert_eq!(label.center().y, radio.center().y);
    assert_eq!(label.center().y, comment.center().y);
    assert!(label.right() <= description.left());
    assert!(comment.right() <= row.right() && comment.left() >= description.right() - px(1.));
    assert!(
        // This description is long enough to also carry the expand button.
        comment.left() - description.right() < px(40.),
        "the text fills the row up to the buttons"
    );
    assert!(
        comment.size.width <= px(28.),
        "the comment button is an icon, not a word"
    );
    assert!(
        vcx.debug_bounds("ask-tab-1").is_none(),
        "one question needs no review tab"
    );
    // One question: pick, then Enter sends. No review step.
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("1 enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert!(
            panel.ask_card.as_ref().is_none_or(|card| card.submitting()),
            "Enter on the only question submits it"
        );
    });
}

#[gpui::test]
fn selecting_an_option_preserves_option_text_geometry(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.simulate_resize(gpui::size(px(900.), px(700.)));
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let second = AskQuestion {
        question: "Какие идеи Hax стоит добавить в бэклог?".into(),
        header: Some("Из Hax".into()),
        ..question(true)
    };
    let event = serde_json::from_value(serde_json::json!({
        "ev": "ask_request", "session_id": "session-a", "request_id": "ask-1",
        "tool_call_id": "toolu_1", "questions": [{
            "allow_other": true, "header": "Из Codex", "multi_select": true,
            "question": "Что забираем из Codex в первую очередь?",
            "options": [
                {"label": "CLI для CI (Recommended)", "description": "stdin, отдельный файл ответа, запуск без сохранения сессии. JSON/NDJSON не ломаем."},
                {"label": "Ревью по Git", "description": "Ревью по base, commit или uncommitted с файлами, строками и серьёзностью. Нынешний /review остаётся."},
                {"label": "Worktree и Git diff", "description": "Новая сессия в отдельном worktree одной командой; из TUI видно staged, unstaged и untracked diff."},
                {"label": "AGENTS.md и хуки", "description": "Инструкции от корня до cwd с override и показом источников; хуки до отправки и после ответа."},
                {"label": "Пока ничего", "description": "Оставить Codex-аудит как бэклог без выбора сейчас."}
            ]
        }, second]
    }))
    .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();

    let rows = [
        ("ask-label-0-0", "ask-description-0-0"),
        ("ask-label-0-1", "ask-description-0-1"),
        ("ask-label-0-2", "ask-description-0-2"),
        ("ask-label-0-3", "ask-description-0-3"),
        ("ask-label-0-4", "ask-description-0-4"),
    ];
    let before = rows.map(|(label, _)| vcx.debug_bounds(label).unwrap().size);
    let first = vcx.debug_bounds("ask-label-0-0").unwrap().center();
    vcx.simulate_click(first, gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], [0]);
    });

    for ((selector, description), before) in rows.into_iter().zip(before) {
        let label = vcx.debug_bounds(selector).unwrap();
        let description = vcx.debug_bounds(description).unwrap();
        assert!(
            label.size == before && label.right() <= description.left(),
            "{selector}: label was {before:?}, now {label:?}; description {description:?}"
        );
    }
}

/// Every option label paints the lines the layout made room for. The
/// installed app laid a label out as one line and then painted it one
/// letter per line over the descriptions below.
fn assert_labels_paint_inside_their_rows(
    panel: &Entity<Panel>,
    vcx: &mut gpui::VisualTestContext,
    stage: &str,
) {
    let options = panel.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().unwrap();
        card.questions[card.cursor_question].options.len()
    });
    let q = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().cursor_question
    });
    for o in 0..options {
        let selector: &'static str = format!("ask-label-{q}-{o}").leak();
        let bounds = vcx.debug_bounds(selector).unwrap();
        let (lines, line_height, text) = panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            let layouts = card.label_layouts.0.borrow();
            let layout = &layouts[&(q, o)];
            let wrapped = layout.wrapped_text();
            (wrapped.lines().count(), layout.line_height(), wrapped)
        });
        assert!(
            line_height * lines as f32 <= bounds.size.height + px(1.),
            "{stage}: label {o} paints {lines} lines into {bounds:?}: {text:?}"
        );
    }
}

#[gpui::test]
fn option_labels_paint_the_lines_they_were_laid_out_for(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.simulate_resize(gpui::size(px(120.), px(800.)));
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    // The ask from the report: one question, three long descriptions.
    let event = serde_json::from_value(serde_json::json!({
        "ev": "ask_request", "session_id": "session-a", "request_id": "ask-htpc",
        "tool_call_id": "toolu_htpc", "questions": [{
            "header": "Очистка", "multi_select": false, "allow_other": true,
            "question": "Какой из проверенных наборов удалить с root HTPC?",
            "options": [
                {"label": "Очистить root (Recommended)", "description": "Освободить около 2,2 ГиБ на root: неиспользуемый Docker-образ node:22, кэш пакетов APT и два неполных rollout-снимка SQLite без манифеста. Сохранить три полных отката, действующие сервисы, видеозаписи и все данные SSD."},
                {"label": "Только Docker и APT", "description": "Удалить только неиспользуемый Docker-образ node:22 и кэш загрузок APT. Около 1 ГиБ, все бэкапы остаются."},
                {"label": "Ничего не удалять", "description": "Ничего не удалять. На SSD свободно около 388 ГиБ, пригодных для безопасного удаления копий не нашёл."}
            ]
        }]
    }))
    .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();
    assert_labels_paint_inside_their_rows(&panel, vcx, "shown");

    for width in [260., 520., 900., 640.] {
        vcx.simulate_resize(gpui::size(px(width), px(800.)));
        vcx.run_until_parked();
        assert_labels_paint_inside_their_rows(&panel, vcx, &format!("width {width}"));
    }
    let first = vcx.debug_bounds("ask-label-0-0").unwrap().center();
    vcx.simulate_click(first, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_labels_paint_inside_their_rows(&panel, vcx, "selected");
    let second = vcx.debug_bounds("ask-label-0-1").unwrap().center();
    vcx.simulate_mouse_move(second, None, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_labels_paint_inside_their_rows(&panel, vcx, "hovered");
}

#[gpui::test]
fn question_name_is_in_the_tab_strip(cx: &mut gpui::TestAppContext) {
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
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
    });
    vcx.run_until_parked();
    let tabs = vcx.debug_bounds("ask-tabs").unwrap();
    let label = vcx
        .debug_bounds("ask-tab-label-1 Database")
        .expect("question name is rendered in the tab strip");
    assert!(tabs.contains(&label.center()));
    let question = vcx.debug_bounds("ask-question-0").unwrap();
    assert!(
        question.top() - tabs.bottom() < px(20.),
        "no duplicate heading between tabs and question"
    );
}

#[gpui::test]
fn tabs_digits_and_comment_keep_answers(cx: &mut gpui::TestAppContext) {
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
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    vcx.run_until_parked();
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let mut second = value["questions"][0].clone();
    second["multi_select"] = true.into();
    value["questions"].as_array_mut().unwrap().push(second);
    panel.update(vcx, |panel, cx| {
        panel.apply(&serde_json::from_value(value).unwrap(), cx)
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    assert!(vcx.debug_bounds("ask-question-0").is_some());
    assert!(vcx.debug_bounds("ask-question-1").is_none());
    assert!(vcx.debug_bounds("ask-comment-0-0").is_none());
    let tabs = vcx.debug_bounds("ask-tabs").unwrap();
    let card_bounds = vcx.debug_bounds("ask-card").unwrap();
    assert!(
        tabs.top() - card_bounds.top() <= px(16.),
        "tabs must be the card header"
    );
    vcx.simulate_keystrokes("2 n");
    vcx.run_until_parked();
    let editor = vcx.debug_bounds("ask-comment-0-1").unwrap();
    let option = vcx.debug_bounds("ask-option-0-1").unwrap();
    assert!(
        option.contains(&editor.center()),
        "selected option must contain its own comment editor"
    );
    vcx.simulate_click(editor.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let panel = panel.read(cx);
        assert!(
            panel.ask_card.as_ref().unwrap().comment_inputs[0][1]
                .read(cx)
                .is_focused(window),
            "N should focus comment"
        );
    });
    vcx.simulate_input("comment");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let panel = panel.read(cx);
        assert!(
            panel.focus_handle.is_focused(window),
            "escape should focus options"
        );
    });
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-question-1").is_some());
    vcx.simulate_keystrokes("left right 1 down n");
    vcx.run_until_parked();
    vcx.simulate_input("second comment");
    vcx.simulate_keystrokes("shift-tab");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-question-1").is_some(),
        "Tab navigates controls without switching questions"
    );
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![1]);
        assert_eq!(card.chosen[1], vec![0, 1]);
        assert_eq!(card.comments[0][1], "comment");
        assert_eq!(card.comments[1][1], "second comment");
        assert_eq!(card.cursor_question, 0);
    });
    vcx.simulate_keystrokes("right right");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-review").is_some());
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-question-0").is_some(),
        "right wraps review to first question"
    );
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-review").is_some(),
        "left wraps first question to review"
    );
    assert!(
        vcx.debug_bounds("ask-review-comment-0-1").is_some(),
        "review should show the selected option's comment"
    );
}

#[gpui::test]
fn escape_requires_confirmation_to_decline(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-confirm-decline").is_some());
    assert!(
        !commands
            .try_iter()
            .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-confirm-decline").is_none());
    vcx.simulate_keystrokes("escape enter");
    vcx.run_until_parked();
    assert!(commands.try_iter().any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { response, .. } if response.answers.is_none())));
}

#[gpui::test]
fn wheel_over_option_scrolls_ask_body(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
    vcx.run_until_parked();
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let options = value["questions"][0]["options"].as_array().unwrap().clone();
    value["questions"][0]["options"] =
        serde_json::Value::Array((0..8).flat_map(|_| options.clone()).collect());
    panel.update(vcx, |panel, cx| {
        panel.apply(&serde_json::from_value(value).unwrap(), cx)
    });
    vcx.run_until_parked();
    let option = vcx.debug_bounds("ask-option-0-0").unwrap();
    let before = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    let transcript = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    vcx.simulate_mouse_move(option.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: option.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-80.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    let after = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    assert!(
        after < before,
        "wheel over option should scroll ask: {before:?} -> {after:?}"
    );
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
        transcript
    );
    panel.update(vcx, |panel, cx| {
        panel
            .ask_card
            .as_ref()
            .unwrap()
            .scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    });
    vcx.run_until_parked();
    let option = vcx.debug_bounds("ask-option-0-0").unwrap();
    vcx.simulate_mouse_move(option.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: option.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., -2.)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    assert!(
        panel.read_with(vcx, |panel, _| panel
            .ask_card
            .as_ref()
            .unwrap()
            .scroll
            .offset()
            .y)
            < px(0.),
        "line wheel over option should scroll the card"
    );
    panel.update(vcx, |panel, cx| {
        let scroll = &panel.ask_card.as_ref().unwrap().scroll;
        scroll.set_offset(gpui::point(px(0.), -scroll.max_offset().y));
        cx.notify();
    });
    vcx.run_until_parked();
    let own = panel.read_with(vcx, |panel, cx| {
        panel.ask_card.as_ref().unwrap().other_inputs[0]
            .read(cx)
            .test_editor_scroll()
            .bounds()
    });
    let before = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    vcx.simulate_mouse_move(own.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: own.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(80.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    let after = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    assert!(
        after > before,
        "wheel over short editor should scroll ask: {before:?} -> {after:?}"
    );
    let input = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().other_inputs[0].clone()
    });
    input.update(vcx, |input, cx| {
        input.set_content("long answer\n".repeat(100), cx)
    });
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        let scroll = &panel.ask_card.as_ref().unwrap().scroll;
        scroll.set_offset(gpui::point(px(0.), -scroll.max_offset().y));
        cx.notify();
    });
    vcx.run_until_parked();
    let editor = input.read_with(vcx, |input, _| input.test_editor_scroll());
    assert!(
        editor.max_offset().y > px(100.),
        "long editor must overflow"
    );
    editor.set_offset(gpui::point(px(0.), px(-100.)));
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let own = editor.bounds();
    let before_card = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    let before_editor = editor.offset().y;
    vcx.simulate_mouse_move(own.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: own.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(40.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    assert!(
        editor.offset().y > before_editor,
        "overflowing editor should scroll first"
    );
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel
            .ask_card
            .as_ref()
            .unwrap()
            .scroll
            .offset()
            .y),
        before_card,
        "editor wheel must not also scroll the card"
    );
    editor.set_offset(gpui::point(px(0.), px(0.)));
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: own.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(40.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    let after_card = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().scroll.offset().y
    });
    assert!(
        after_card > before_card,
        "wheel at editor edge should scroll the card: {before_card:?} -> {after_card:?}"
    );
}

#[gpui::test]
fn space_selects_ask_when_transcript_has_focus(cx: &mut gpui::TestAppContext) {
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
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let focus = panel
            .read(cx)
            .transcript_selection
            .read(cx)
            .focus_handle()
            .clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("space");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, cx| {
        assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![0]);
        assert!(panel.input.read(cx).snapshot().content.is_empty());
    });
}

#[gpui::test]
fn keyboard_navigates_multiple_groups_and_submits(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    vcx.run_until_parked();
    for _ in 0..120 {
        if vcx.debug_bounds("composer-dock").is_some() {
            break;
        }
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    vcx.update(|window, cx| window.focus(&panel.read(cx).input_focus_handle(cx), cx));
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let second = value["questions"][0].clone();
    value["questions"].as_array_mut().unwrap().push(second);
    let event: jcode_sdk::ApiEvent = serde_json::from_value(value).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("j k up space right j space enter");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(commands.try_iter().any(|command| matches!(command,
        crate::harness::Command::AskResponse { response, .. }
        if response.answers.as_ref().is_some_and(|answers|
            answers.len() == 2 && answers[0].selected == ["DynamoDB"] && answers[1].selected == ["SQLite"])
    )), "keyboard selection must submit both groups");
}

#[gpui::test]
fn ask_can_submit_after_skipping_a_question(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    vcx.run_until_parked();
    vcx.update(|window, cx| window.focus(&panel.read(cx).input_focus_handle(cx), cx));
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let second = value["questions"][0].clone();
    value["questions"].as_array_mut().unwrap().push(second);
    let event = serde_json::from_value(value).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("1 enter enter");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-review").is_some());
    let submit = vcx.debug_bounds("ask-submit").unwrap();
    let back = vcx.debug_bounds("ask-back").unwrap();
    assert!((back.center().y - submit.center().y).abs() <= px(1.));
    assert!(back.right() <= submit.left());
    vcx.simulate_click(back.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("ask-question-1").is_some());
    let next = vcx.debug_bounds("ask-next").unwrap();
    let input = vcx.debug_bounds("ask-other-editor-1").unwrap();
    let back = vcx.debug_bounds("ask-back").unwrap();
    assert!((back.center().y - input.center().y).abs() <= px(1.));
    assert!(input.right() <= back.left() && back.right() <= next.left());
    vcx.simulate_click(next.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let submit = vcx.debug_bounds("ask-submit").unwrap();
    let unanswered = vcx.debug_bounds("ask-review-unanswered-1");
    vcx.simulate_click(submit.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        commands.try_iter().any(|command| matches!(command,
            Command::AskResponse { response, .. }
            if response.answers == Some(vec![AskAnswer {
                selected: vec!["Postgres".into()],
                ..Default::default()
            }, AskAnswer::default()])
        )),
        "submit must preserve the skipped question without selecting a default"
    );
    let unanswered = unanswered.expect("review labels the skipped question");
    assert!((unanswered.center().y - submit.center().y).abs() <= px(1.));
}

#[gpui::test]
fn large_card_scrolls_without_moving_footer(cx: &mut gpui::TestAppContext) {
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
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
    vcx.run_until_parked();
    for _ in 0..120 {
        if vcx.debug_bounds("composer-dock").is_some() {
            break;
        }
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let mut q = value["questions"][0].clone();
    q["question"] = serde_json::Value::String("This question must wrap cleanly. ".repeat(12));
    q["options"][0]["label"] =
        serde_json::Value::String("This option label must wrap. ".repeat(8));
    q["options"][0]["description"] = serde_json::Value::String(
        "This description must wrap cleanly without truncating. ".repeat(16),
    );
    value["questions"] = serde_json::Value::Array(vec![q; 4]);
    let event: jcode_sdk::ApiEvent = serde_json::from_value(value).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    let viewport = vcx.debug_bounds("ask-scroll").unwrap();
    let footer = vcx.debug_bounds("ask-next").unwrap();
    let input = vcx.debug_bounds("ask-other-editor-0").unwrap();
    assert!((input.center().y - footer.center().y).abs() <= px(1.));
    assert!(input.right() <= footer.left());
    let description = vcx.debug_bounds("ask-description-0-0").unwrap();
    let question = vcx.debug_bounds("ask-question-0").unwrap();
    let label = vcx.debug_bounds("ask-label-0-0").unwrap();
    assert!(
        question.size.height > px(24.),
        "long question must wrap: {question:?}"
    );
    assert!(
        label.size.height > px(24.),
        "long option label must wrap: {label:?}"
    );
    assert!(
        description.size.height < px(24.),
        "a collapsed description is one line: {description:?}"
    );
    let expand = vcx.debug_bounds("ask-expand-button-0-0").unwrap().center();
    vcx.simulate_click(expand, gpui::Modifiers::default());
    vcx.run_until_parked();
    let expanded = vcx.debug_bounds("ask-description-0-0").unwrap();
    assert!(
        expanded.size.height > px(24.),
        "an expanded description wraps whole: {expanded:?}"
    );
    let transcript_before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    let first_before = vcx.debug_bounds("ask-option-0-0").unwrap();
    vcx.simulate_mouse_move(footer.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: footer.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("ask-option-0-0").unwrap().top() < first_before.top(),
        "wheel over sticky footer must scroll the card"
    );
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
        transcript_before
    );
    vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6000.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
        transcript_before,
        "wheel over ask must not scroll transcript"
    );
    assert_eq!(vcx.debug_bounds("ask-other-editor-0"), Some(input));
    assert!(input.top() >= viewport.bottom());
    assert_eq!(
        vcx.debug_bounds("ask-next"),
        Some(footer),
        "footer must remain fixed"
    );
    vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: viewport.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6000.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    let own = vcx.debug_bounds("ask-other-editor-0").unwrap();
    vcx.simulate_click(own.center(), gpui::Modifiers::default());
    vcx.simulate_input("saved draft");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, cx| {
        let card = panel
            .ask_card
            .as_ref()
            .expect("incomplete card remains open");
        assert_eq!(
            card.other_inputs[0].read(cx).snapshot().content,
            "saved draft"
        );
    });
}

#[gpui::test]
fn ask_result_is_visible_and_survives_history_reload(cx: &mut gpui::TestAppContext) {
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
    let input = serde_json::json!({"questions":[
        {"question":"Which?","options":[{"label":"A"},{"label":"B"}]},
        {"question":"Where?","options":[{"label":"Here (Recommended)"},{"label":"There"}]}
    ]});
    let history: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
        {"role":"user","content":"Choose"},
        {"role":"tool","content":"User answers:\n1. Which? -> B (comment: reason)\n2. Where? -> unanswered (user skipped)\n","tool":{"id":"ask-1","name":"ask","input":input}}
    ])).unwrap();
    let image = jcode_sdk::RenderedImage {
        session_image: None,
        history_message_index: Some(2),
        media_type: "image/png".into(),
        data: base64::engine::general_purpose::STANDARD
            .encode(include_bytes!("../../../../../assets/previews/image-preview.png")),
        label: Some("Answer image".into()),
        source: jcode_sdk::RenderedImageSource::ToolResult {
            tool_name: "ask".into(),
        },
        anchor: Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: "ask-1".into() }),
    };
    let mut second = image.clone();
    second.history_message_index = None;
    second.label = Some("Comment image".into());
    panel.update(vcx, |panel, cx| {
        panel.load_history(history, vec![image], cx);
        let event = jcode_sdk::ApiEvent::SidePaneImages {
            session_id: "session-a".into(),
            images: vec![second],
        };
        panel.apply(&event, cx);
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| assert!(panel.items.iter().any(|item| matches!(item, Item::Tool { name, output, .. } if name == "ask" && output.contains("reason")))));
    let row = vcx
        .debug_bounds("decision-header")
        .expect("compact decision row");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("decision-detail").is_some(),
        "reopened history must render ask result"
    );
    assert!(
        vcx.debug_bounds("decision-answer-Unanswered (user skipped)")
            .is_some(),
        "history must show the skipped answer, not the recommended option"
    );
    panel.read_with(vcx, |panel, _| {
        let indices = panel.session_image_indices();
        assert_eq!(
            indices.len(),
            2,
            "distinct labels survive, replay is deduplicated"
        );
        assert!(
            !panel
                .transcript_render_rows()
                .iter()
                .any(|row| indices.contains(&row.index))
        );
    });
    let chip = vcx
        .debug_bounds("ask-result-image")
        .expect("image appears inside ask result");
    assert!(vcx.debug_bounds("ask-result-image-thumbnail").is_some());
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn ask_submission_remains_visible_until_confirmed(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        let card = panel.ask_card.as_mut().unwrap();
        card.toggle(0, 1);
        let response = card.response();
        panel.answer_ask(response, cx);
    });
    panel.read_with(vcx, |panel, _| {
        assert_eq!(
            panel
                .ask_card
                .as_ref()
                .expect("must wait for server acknowledgment")
                .chosen[0],
            vec![1]
        );
    });
    let (request_id, first_attempt) = panel.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().unwrap();
        (card.request_id.clone(), card.attempt_id.unwrap())
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            crate::harness::Update::AskResponseSettled {
                session_id: "session-a".into(),
                request_id: request_id.clone(),
                attempt_id: first_attempt,
                result: Err(AskResponseError::Failed("offline".into())),
            },
            cx,
        )
    });
    panel.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().unwrap();
        assert!(!card.submitting());
        assert_eq!(card.submit_error.as_deref(), Some("offline"));
        assert_eq!(card.chosen[0], vec![1]);
    });
    panel.update(vcx, |panel, cx| {
        let response = panel.ask_card.as_ref().unwrap().response();
        panel.answer_ask(response, cx);
    });
    let attempt_id = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().attempt_id.unwrap()
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            crate::harness::Update::AskResponseSettled {
                session_id: "session-a".into(),
                request_id,
                attempt_id,
                result: Ok(()),
            },
            cx,
        )
    });
    panel.read_with(vcx, |panel, _| assert!(panel.ask_card.is_none()));
}

#[gpui::test]
fn ask_draft_survives_snapshot_and_matching_request(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace.push_test_panel("session-b", cx);
        workspace
    });
    vcx.run_until_parked();
    let first = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let second = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(1))
        .unwrap();
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    first.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        let card = panel.ask_card.as_mut().unwrap();
        card.toggle(0, 1);
        card.comments[0][1] = "important note".into();
    });
    let bytes = first.read_with(vcx, |panel, cx| {
        serde_json::to_vec(&panel.snapshot(cx)).unwrap()
    });
    let snapshot: PanelSnapshot = serde_json::from_slice(&bytes).unwrap();
    second.update(vcx, |panel, cx| {
        panel.restore_snapshot(snapshot, cx);
        panel.apply(&event, cx);
    });
    second.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![1]);
        assert_eq!(card.comments[0][1], "important note");
        assert_eq!(
            card.comment_inputs[0][1].read(cx).snapshot().content,
            "important note"
        );
    });
    second.update(vcx, |panel, cx| panel.apply(&event, cx));
    second.read_with(vcx, |panel, _| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![1]);
        assert_eq!(card.comments[0][1], "important note");
    });
}

#[gpui::test]
fn ask_draft_restores_if_request_arrives_before_snapshot(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace.push_test_panel("session-b", cx);
        workspace
    });
    let first = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let second = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(1))
        .unwrap();
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    first.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        let card = panel.ask_card.as_mut().unwrap();
        card.toggle(0, 1);
        card.comments[0][1] = "important note".into();
    });
    let snapshot = first.read_with(vcx, |panel, cx| panel.snapshot(cx));
    second.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        panel.restore_snapshot(snapshot, cx);
    });
    second.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![1]);
        assert_eq!(
            card.comment_inputs[0][1].read(cx).snapshot().content,
            "important note"
        );
    });
}

#[gpui::test]
fn decline_sends_empty_response_and_renders_question_status(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    for _ in 0..120 {
        if vcx.debug_bounds("ask-decline").is_some() {
            break;
        }
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    let decline = vcx.debug_bounds("ask-decline").unwrap();
    let tabs = vcx.debug_bounds("ask-tabs").unwrap();
    let card = vcx.debug_bounds("ask-card").unwrap();
    assert!(decline.top() < tabs.bottom() && decline.bottom() <= tabs.bottom() + px(1.));
    assert!(card.right() - decline.right() <= px(12.));
    assert!(
        decline.size.width <= px(28.),
        "the discard control is an icon"
    );
    vcx.simulate_click(decline.center(), gpui::Modifiers::default());
    assert!(
        commands.try_iter().any(|command| matches!(command,
            crate::harness::Command::AskResponse { response, .. } if response.answers.is_none()
        )),
        "decline must send an empty response"
    );
    panel.update(vcx, |panel, cx| {
        panel.apply(&jcode_sdk::ApiEvent::ToolDone {
            session_id: "session-a".into(), call_id: "toolu_01J29QR2NCg12HueHJkMWtZU".into(),
            name: "ask".into(), output: "User declined to answer the questions.".into(), error: None,
            codemode: None,
            ask: None,
        }, cx);
        assert!(panel.ask_card.is_none());
        let Item::Tool { input, output, error, ask, .. } = panel.items.iter().find(|item| matches!(item, Item::Tool { call_id, .. } if call_id == "toolu_01J29QR2NCg12HueHJkMWtZU")).unwrap() else { unreachable!() };
        let text = super::decision::selection_segments(0, ask.as_deref(), input, output, error.as_deref());
        let text: Vec<_> = text.iter().map(|(_, text)| text.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("Which database")) && text.contains(&"declined"), "{text:?}");
    });
}

#[gpui::test]
fn cancelled_ask_closes_and_renders_questions_and_status(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.apply(&event, cx);
        panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "unrelated".into(), name: "ask".into(), output: "cancelled".into(), error: Some("cancelled".into()), codemode: None, ask: None }, cx);
        assert!(panel.ask_card.is_some());
        panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "toolu_01J29QR2NCg12HueHJkMWtZU".into(), name: "ask".into(), output: "cancelled by the user".into(), error: Some("cancelled by the user".into()), codemode: None, ask: None }, cx);
        assert!(panel.ask_card.is_none(), "completed tool must close its card");
        let Item::Tool { input, output, error, ask, .. } = panel.items.iter().find(|item| matches!(item, Item::Tool { call_id, .. } if call_id == "toolu_01J29QR2NCg12HueHJkMWtZU")).unwrap() else { unreachable!() };
        let text = super::decision::selection_segments(0, ask.as_deref(), input, output, error.as_deref());
        let text: Vec<_> = text.iter().map(|(_, text)| text.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("Which database")) && text.contains(&"cancelled"), "{text:?}");
    });
}

#[gpui::test]
fn ask_comment_image_survives_draft_and_explicit_submit(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| {
        panel.test_settled_conversation(1, cx);
        let event = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.apply(&event, cx);
        panel.ask_card.as_mut().unwrap().toggle(0, 0);
        panel.ask_card.as_mut().unwrap().comment_open[0][0] = true;
        cx.notify();
    });
    vcx.run_until_parked();
    let input = panel.read_with(vcx, |panel, _| {
        panel.ask_card.as_ref().unwrap().comment_inputs[0][0].clone()
    });
    input.update(vcx, |input, cx| {
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: vec![2],
                width: 1,
                height: 1,
            },
            cx,
        )
    });
    let draft = panel.read_with(vcx, |panel, cx| panel.ask_card.as_ref().unwrap().draft(cx));
    let draft = serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
    panel.update(vcx, |panel, cx| {
        panel.ask_card.as_mut().unwrap().restore_draft(draft, cx)
    });
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(
            card.comment_inputs[0][0].read(cx).snapshot().attachments[0].encoded,
            "Ag=="
        );
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.focus(&input.read(cx).editor_focus(), cx));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        !commands
            .try_iter()
            .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
    );
    let submit = vcx.debug_bounds("ask-submit").unwrap().center();
    vcx.simulate_click(submit, gpui::Modifiers::default());
    vcx.run_until_parked();
    let response = commands
        .try_iter()
        .find_map(|command| match command {
            crate::harness::Command::AskResponse { response, .. } => {
                Some(serde_json::to_value(response).unwrap())
            }
            _ => None,
        })
        .expect("ask response was sent");
    assert_eq!(
        response["answers"][0]["comment_images"]["Postgres"][0][1],
        "Ag=="
    );
    assert_eq!(response["answers"][0]["comments"]["Postgres"], "[#image 1]");
}

#[gpui::test]
fn ask_answer_image_is_sent_and_survives_incomplete_enter(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .unwrap();
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    let question = value["questions"][0].clone();
    value["questions"].as_array_mut().unwrap().push(question);
    let event = serde_json::from_value(value).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let input = panel.read(cx).ask_card.as_ref().unwrap().other_inputs[0].clone();
        window.focus(&input.read(cx).editor_focus(), cx);
        input.update(cx, |input, cx| {
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: vec![1],
                    width: 1,
                    height: 1,
                },
                cx,
            )
        });
    });
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        let snapshot = card.other_inputs[0].read(cx).snapshot();
        assert_eq!(
            snapshot.attachments.len(),
            1,
            "incomplete Enter must retain the image"
        );
    });
    panel.update(vcx, |panel, cx| {
        let card = panel.ask_card.as_mut().unwrap();
        card.toggle(1, 0);
        cx.notify();
    });
    vcx.run_until_parked();
    let next = vcx.debug_bounds("ask-next").unwrap().center();
    vcx.simulate_click(next, gpui::Modifiers::default());
    vcx.run_until_parked();
    let submit = vcx.debug_bounds("ask-submit").unwrap().center();
    vcx.simulate_click(submit, gpui::Modifiers::default());
    vcx.run_until_parked();
    let response = commands
        .try_iter()
        .find_map(|command| match command {
            crate::harness::Command::AskResponse { response, .. } => {
                Some(serde_json::to_value(response).unwrap())
            }
            _ => None,
        })
        .expect("ask response was sent");
    assert_eq!(response["answers"][0]["other_images"][0][1], "AQ==");
    panel.update(vcx, |panel, cx| {
        let request_id = panel.ask_card.as_ref().unwrap().request_id.clone();
        settle_test_ask(
            panel,
            &request_id,
            Err(AskResponseError::Failed("network error".into())),
            cx,
        );
    });
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert!(!card.submitting());
        assert_eq!(
            card.other_inputs[0].read(cx).snapshot().attachments[0].encoded,
            "AQ=="
        );
    });
}

#[gpui::test]
fn ask_editor_receives_space_and_printable_shortcuts(cx: &mut gpui::TestAppContext) {
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
    panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
    vcx.run_until_parked();
    let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
    panel.update(vcx, |panel, cx| panel.apply(&event, cx));
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let focus = panel.read(cx).ask_card.as_ref().unwrap().other_inputs[0]
            .read(cx)
            .editor_focus();
        window.focus(&focus, cx);
    });
    vcx.simulate_input("a");
    vcx.simulate_keystrokes("space");
    vcx.simulate_input("b");
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.other_inputs[0].read(cx).snapshot().content, "a b");
        assert!(card.chosen[0].is_empty());
    });
    vcx.simulate_keystrokes("n 1 j k shift-space shift-enter");
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(
            card.other_inputs[0].read(cx).snapshot().content,
            "a bn1jk \n"
        );
        assert!(card.chosen[0].is_empty());
        assert!(!card.submitting());
    });
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("space n");
    vcx.simulate_input("a");
    vcx.simulate_keystrokes("space");
    vcx.simulate_input("bn1jk");
    vcx.simulate_keystrokes("shift-enter");
    panel.read_with(vcx, |panel, cx| {
        let card = panel.ask_card.as_ref().unwrap();
        assert_eq!(card.chosen[0], vec![0]);
        assert_eq!(
            card.comment_inputs[0][0].read(cx).snapshot().content,
            "a bn1jk\n"
        );
        assert!(!card.submitting());
    });
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert!(!panel.ask_card.as_ref().unwrap().submitting())
    });
    vcx.simulate_keystrokes("escape enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert!(panel.ask_card.as_ref().unwrap().submitting())
    });
}

#[path = "panel_ask_flow_tests.rs"]
mod flow;

#[path = "panel_ask_record_tests.rs"]
mod record;
