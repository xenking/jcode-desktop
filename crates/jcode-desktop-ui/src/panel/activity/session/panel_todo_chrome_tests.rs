//! The todo card on the real `todo` tool output shape: titles under `name`.
//! Wrapping itself is not observable here (the headless text system never wraps);
//! it is checked in screenshots.
use super::*;

fn tool_output(stages: u32, per_stage: u32, title: &str, delivery: bool) -> String {
    let items: Vec<_> = (1..=stages)
        .flat_map(|stage| {
            (1..=per_stage).map(move |n| {
                serde_json::json!({
                    "name": format!("{title} {stage}.{n}"), "status": "pending",
                    "priority": "high", "group": "Desktop", "stage": stage,
                    "confidence": "validated", "lane_dependencies": [],
                })
            })
        })
        .collect();
    let mut goal = serde_json::json!({
        "group": "Desktop", "closed_feedback_loop": "closed",
        "feedback_loop_relevance": "acceptance_aligned",
        "feedback_loop_coverage": "edge_and_integration_paths",
        "feedback_loop_traceability": "complete",
    });
    if delivery {
        goal["delivery_state"] = "integrated".into();
    }
    format!(
        "{}\n\nPlan:\n{}\n\nGoals:\n{}",
        serde_json::to_string(&items).unwrap(),
        // Legacy saved plans say "complete" for what is now "clear".
        serde_json::json!({"user_intention": title, "understands_user_intent": "complete"}),
        serde_json::json!([goal]),
    )
}

#[test]
fn the_real_tool_output_shape_titles_items_by_name() {
    let payload = parse_todo_tool_output(&tool_output(2, 2, "Redacted step", true))
        .expect("name-keyed output parses");
    assert_eq!(payload.todos.len(), 4);
    assert_eq!(payload.todos[3].content, "Redacted step 2.2");
    assert_eq!(payload.todos[3].stage, Some(2));
    assert_eq!(
        payload.plan.understands_user_intent,
        Some(jcode_base::todo::IntentUnderstanding::Clear)
    );
    assert_eq!(payload.goals.len(), 1);
    // Snapshots and older outputs keep `content`.
    let legacy = parse_todo_tool_output(r#"[{"content":"Old","status":"pending"}]"#).unwrap();
    assert_eq!(legacy.todos[0].content, "Old");
}

fn pinned_card_height(
    cx: &mut gpui::VisualTestContext,
    panel: &gpui::Entity<Panel>,
    output: String,
) -> f32 {
    panel.update(cx, |panel, cx| {
        *panel.items = vec![
            Item::User("plan it".into()),
            Item::Tool {
                call_id: "todo-1".into(),
                name: "todo".into(),
                input: "{}".into(),
                output,
                done: true,
                error: None,
                ask: None,
            },
        ];
        cx.notify();
    });
    cx.run_until_parked();
    let paper = cx
        .debug_bounds("pinned-todo-details-paper")
        .expect("the todo card is pinned");
    assert!(paper.right() <= cx.debug_bounds("transcript").unwrap().right());
    f32::from(paper.size.height)
}

#[gpui::test]
fn pinned_card_paints_the_real_tool_output_at_narrow_and_wide_windows(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("todo-chrome", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    let long = "a very long task title that would need several lines in a narrow card ".repeat(6);
    for width in [900.0, 1800.0] {
        vcx.simulate_resize(gpui::size(px(width), px(900.)));
        for (stages, per_stage) in [(3, 3), (12, 1)] {
            for delivery in [true, false] {
                let short = pinned_card_height(
                    vcx,
                    &panel,
                    tool_output(stages, per_stage, "Step", delivery),
                );
                let wrapped = pinned_card_height(
                    vcx,
                    &panel,
                    tool_output(stages, per_stage, &long, delivery),
                );
                assert_eq!(
                    short, wrapped,
                    "a long title or intention must not change the card height: {stages} stages, delivery {delivery}, {width}px"
                );
            }
        }
    }
}
