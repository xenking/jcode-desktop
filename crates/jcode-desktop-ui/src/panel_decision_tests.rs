use super::*;

#[gpui::test]
fn answered_ask_is_a_user_decision_not_a_tool_row(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "decision-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::Tool {
            call_id: "decision-1".into(), name: "ask".into(),
            input: r#"{"questions":[{"header":"Database","question":"Which database?","options":[{"label":"Postgres"},{"label":"SQLite"}]}]}"#.into(),
            output: "User answers:\n1. Which database? -> Postgres (comment: reliable)".into(),
            done: true, error: None,
        }];
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("decision-card").is_some());
    assert!(vcx.debug_bounds("decision-choice").is_some());
    panel.update(vcx, |panel, _| {
        assert_eq!(panel.transcript_render_rows()[0].role, Some("you"))
    });
    assert!(vcx.debug_bounds("decision-detail").is_some());
    assert!(
        vcx.debug_bounds("decision-question-Which database?")
            .is_some()
    );
    assert!(
        vcx.debug_bounds("decision-answer-✓ Postgres\nUser comment: reliable")
            .is_some()
    );
}

#[gpui::test]
fn history_restores_decision_and_counts_it_as_a_user_turn(cx: &mut gpui::TestAppContext) {
    let message: jcode_sdk::HistoryMessage = serde_json::from_value(serde_json::json!({
        "role": "tool", "content": "User answers:\n1. Which? -> B\n", "timestamp_ms": 1234,
        "tool": {"id": "ask-1", "name": "ask", "input": {"questions": [{"question": "Which?", "options": [{"label": "A"}, {"label": "B"}]}]}}
    })).unwrap();
    let (items, _) = Panel::history_items(vec![message], vec![]);
    assert!(
        matches!(&items[0], Item::Tool { name, output, .. } if name == "ask" && output.contains("B"))
    );
    assert_eq!(super::prompt::prompt_number(&items, 0), Some(1));
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "decision-history".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(vcx, |panel, cx| {
        panel.set_clean_chat(Some(true), cx);
        *panel.items = items;
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("decision-card").is_some());
    assert!(vcx.debug_bounds("decision-choice").is_some());
    assert!(vcx.debug_bounds("decision-question-Which?").is_some());
    assert!(vcx.debug_bounds("clean-chat-user").is_some());
    assert!(vcx.debug_bounds("tool-header").is_none());
    assert!(vcx.debug_bounds("decision-answer-✓ B").is_some());
}

#[gpui::test]
fn long_answered_question_wraps_inside_card(cx: &mut gpui::TestAppContext) {
    let question = "Which database should we use for the long term project and how should we migrate all existing records safely without interrupting users or losing any important data?";
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("decision-wrap".into(), None, None, crate::harness::spawn_inert(), cx);
        *panel.items = vec![Item::Tool {
            call_id: "wrap".into(), name: "ask".into(),
            input: serde_json::json!({"questions":[{"question":question,"options":[{"label":"Postgres"}]}]}).to_string(),
            output: format!("User answers:\n1. {question} -> Postgres"),
            done: true, error: None,
        }];
        panel
    });
    vcx.run_until_parked();
    let bounds = vcx.debug_bounds("decision-question-Which database should we use for the long term project and how should we migrate all existing records safely without interrupting users or losing any important data?").unwrap();
    let card = vcx.debug_bounds("decision-card").unwrap();
    assert!(bounds.size.height > px(25.), "question did not wrap: {bounds:?}");
    assert!(bounds.right() <= card.right(), "question escapes card: {bounds:?} {card:?}");
}
