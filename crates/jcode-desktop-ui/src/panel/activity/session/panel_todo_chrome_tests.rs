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

fn item(name: &str, status: &str, group: &str, stage: u32) -> TodoCardItem {
    TodoCardItem {
        content: name.into(),
        status: status.into(),
        group: Some(group.into()),
        blocked_by: vec![],
        confidence: None,
        completion_confidence: None,
        stage: Some(stage),
        assigned_to: Some("session_flamingo_1_a".into()),
    }
}

/// A swarm plan the size of the owner's: ten finished groups, then one with work.
fn swarm_plan() -> Vec<TodoCardItem> {
    let mut todos: Vec<_> = (0..10)
        .flat_map(|g| {
            (0..3).map(move |n| item(&format!("done {g}.{n}"), "completed", &format!("G{g}"), 6))
        })
        .collect();
    todos.push(item("shipped", "completed", "ACTIVE", 6));
    todos.push(item("doing", "in_progress", "ACTIVE", 6));
    todos.push(item("next", "pending", "ACTIVE", 6));
    todos.push(item("later", "pending", "WAITING", 6));
    todos
}

#[test]
fn the_compact_card_lists_only_the_active_groups_open_rows() {
    let todos = swarm_plan();
    let sections = todo_sections(&todos, true);
    assert_eq!(sections.len(), 12, "every group keeps its header line");
    assert!(sections[0].active, "work in hand leads the card");
    assert_eq!(sections[0].group, Some("ACTIVE"));
    let listed: Vec<_> = sections
        .iter()
        .flat_map(|s| s.items.iter().map(|t| t.content.as_str()))
        .collect();
    assert_eq!(
        listed,
        ["doing", "next"],
        "finished groups and completed rows collapse"
    );
    assert_eq!(
        (sections[0].done, sections[0].total),
        (1, 3),
        "counts stay whole"
    );
    assert_eq!((sections[1].done, sections[1].total), (3, 3));
    assert_eq!(sections[1].group, Some("G0"), "the rest keep their order");
}

#[test]
fn the_active_group_is_the_one_in_progress_else_the_first_with_open_work() {
    let todos = vec![
        item("a", "pending", "A", 1),
        item("b", "in_progress", "B", 1),
        item("c", "pending", "C", 1),
    ];
    let active = |todos: &[TodoCardItem]| {
        todo_sections(todos, true)
            .iter()
            .find(|s| s.active)
            .and_then(|s| s.group)
            .map(str::to_owned)
    };
    assert_eq!(active(&todos).as_deref(), Some("B"));
    let idle = vec![item("a", "completed", "A", 1), item("b", "pending", "B", 1)];
    assert_eq!(active(&idle).as_deref(), Some("B"));
    let finished = vec![item("a", "completed", "A", 1)];
    assert_eq!(active(&finished), None);
}

#[test]
fn a_phase_line_is_drawn_once_per_stage_not_once_per_group() {
    let todos = swarm_plan();
    for compact in [true, false] {
        let phases: Vec<_> = todo_sections(&todos, compact)
            .iter()
            .filter(|s| s.phase)
            .map(|s| s.stage)
            .collect();
        assert_eq!(phases, [Some(6)], "compact {compact}");
    }
    let staged = vec![
        item("a", "pending", "A", 1),
        item("b", "pending", "B", 2),
        item("c", "pending", "C", 2),
    ];
    let phases: Vec<_> = todo_sections(&staged, false)
        .iter()
        .map(|s| s.phase)
        .collect();
    assert_eq!(phases, [true, true, false]);
}

#[test]
fn the_full_plan_keeps_every_row() {
    let todos = swarm_plan();
    let rows: usize = todo_sections(&todos, false)
        .iter()
        .map(|s| s.items.len())
        .sum();
    assert_eq!(rows, todos.len());
}

#[gpui::test]
fn the_pinned_card_stays_a_few_rows_tall_however_large_the_plan(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("todo-swarm", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    for width in [900.0, 1800.0] {
        vcx.simulate_resize(gpui::size(px(width), px(900.)));
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Todos(TodoCardPayload {
                todos: swarm_plan(),
                plan: TodoCardPlan::default(),
                goals: Vec::new(),
            })];
            cx.notify();
        });
        vcx.run_until_parked();
        let body = vcx.debug_bounds("todo-card-body").expect("pinned body");
        assert!(
            f32::from(body.size.height) <= 100.5,
            "{width}px: {:?}",
            body.size
        );
        assert!(
            vcx.debug_bounds("selectable-text-pinned-todo-group-0")
                .is_some()
        );
        assert!(
            vcx.debug_bounds("selectable-text-pinned-todo-task-0-0")
                .is_some(),
            "the active group leads"
        );
        assert!(
            vcx.debug_bounds("selectable-text-pinned-todo-task-1-0")
                .is_none(),
            "a finished group lists no rows"
        );
    }
}

#[gpui::test]
fn a_pinned_card_that_fits_does_not_fade(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("todo-small", cx);
        workspace
    });
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::Todos(TodoCardPayload {
            todos: vec![
                item("one", "in_progress", "A", 1),
                item("two", "pending", "A", 1),
            ],
            plan: TodoCardPlan::default(),
            goals: Vec::new(),
        })];
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("todo-card-body").is_some());
    assert!(vcx.debug_bounds("pinned-todo-fade").is_none());
}
