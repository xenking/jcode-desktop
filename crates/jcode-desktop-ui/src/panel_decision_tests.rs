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
    let header = vcx.debug_bounds("decision-header").unwrap();
    vcx.simulate_click(header.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("decision-detail").is_some());
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
        *panel.items = items;
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("decision-card").is_some());
    assert!(vcx.debug_bounds("decision-choice").is_some());
}
