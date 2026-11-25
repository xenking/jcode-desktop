use super::*;
use crate::applet_runtime::{AgentOutbound, agent_instance_id};
use serde_json::json;
use std::rc::Rc;

fn snapshot(revision: u64, note: &str) -> jcode_applet_types::AgentApplets {
    let state = if revision <= 2 {
        json!({})
    } else {
        json!({"note": note})
    };
    serde_json::from_value(json!({"instances": [{
        "id": "review", "applet": "jcode.agent",
        "placement": {"kind": "inline", "session_id": "server", "anchor": {"kind": "end"}},
        "document": {"revision": revision, "title": "Review", "state": state,
            "view": {"type": "stack", "children": [
                {"type": "input", "key": "note", "bind": "note", "placeholder": "Note"},
                {"type": "button", "key": "approve", "label": "Approve", "on_press": {"action": "approve"}}
            ]}}
    }]})).unwrap()
}

fn mount_call(id: &str) -> Item {
    Item::Tool {
        name: "applet".into(),
        call_id: format!("mount-{id}"),
        input: json!({"action":"mount","instance":id}).to_string(),
        output: format!("Applet `{id}` revision 1."),
        done: true,
        error: None,
    }
}

#[gpui::test]
fn restored_cards_outside_the_loaded_page_never_appear_at_tail(cx: &mut gpui::TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_,cx| {
        let mut panel = Panel::new("session-a".into(),None,None,crate::harness::spawn_inert(),cx);
        let mut state=snapshot(2, "");
        let mut inline=state.instances[0].clone(); inline.id="inline".into();
        inline.placement=serde_json::from_value(json!({"kind":"inline","session_id":"session-a","anchor":{"kind":"tool_call","call_id":"mount-inline"}})).unwrap();
        state.instances.push(inline);
        panel.items.push(Item::User("latest unrelated prompt".into()));
        for instance in &state.instances {panel.applet_positions.insert(agent_instance_id("session-a", &instance.id),1);}
        crate::applet_runtime::get(cx).sync_agent("session-a", &state);
        for clean in [false,true] {panel.set_clean_chat(Some(clean),cx);panel.sync_applet_rows(cx);assert!(panel.applet_rows.is_empty(), "unloaded cards reappeared: {:?}",panel.applet_rows);}
        panel.items.splice(0..0,[mount_call("review"),mount_call("inline")]);
        panel.sync_applet_rows(cx);assert_eq!(panel.applet_rows.len(),2);assert!(panel.applet_rows.iter().all(|(i,_)| *i==1));
        panel
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn old_applets_stay_at_mount_when_one_is_updated(cx: &mut gpui::TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        let mut state = snapshot(1, "");
        let mut other = state.instances[0].clone();
        other.id = "second".into();
        state.instances.push(other);
        for (id, rev) in [("review", 1), ("second", 1), ("second", 2)] {
            panel.items.push(Item::Tool {
                name: "applet".into(),
                call_id: format!("{id}-{rev}"),
                input: String::new(),
                output: format!("Applet `{id}` revision {rev}."),
                done: true,
                error: None,
            });
            panel.items.push(Item::Assistant("Next message".into()));
        }
        let runtime = crate::applet_runtime::get(cx);
        runtime.sync_agent("session-a", &state);
        let expected = vec![
            (1, agent_instance_id("session-a", "review")),
            (3, agent_instance_id("session-a", "second")),
        ];
        for clean in [false, true] {
            panel.set_clean_chat(Some(clean), cx);
            panel.sync_applet_rows(cx);
            assert_eq!(panel.applet_rows, expected);
            state.instances[1].document.revision += 1;
            crate::applet_runtime::get(cx).sync_agent("session-a", &state);
            panel.sync_applet_rows(cx);
            assert_eq!(panel.applet_rows, expected);
        }
        panel
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn session_tasks_follow_todo_events_without_applet_revision(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.items.push(Item::User("Tasks".into()));
        panel.items.push(mount_call("tasks"));
        panel
    });
    let snapshot: jcode_applet_types::AgentApplets = serde_json::from_value(json!({"instances": [{
        "id": "tasks", "applet": "jcode.agent",
        "placement": {"kind": "inline", "session_id": "server", "anchor": {"kind": "end"}},
        "document": {"revision": 1, "title": "Tasks", "view": {"type": "session_tasks", "title": "Tasks"}}
    }]})).unwrap();
    let id = agent_instance_id("session-a", "tasks");
    vcx.update(|_, cx| assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot)));
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("task-count-0/0").is_some());
    vcx.update(|_, cx| {
        cx.set_global(crate::applet_view::CatalogTasks {
            rows: Rc::new([(
                "session-a".into(),
                vec![("Saved task".into(), "completed".into())],
            )]
            .into()),
            progress: Rc::new([("session-a".into(), (1, 1))].into()),
        })
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("task-count-1/1").is_some());
    assert!(
        vcx.debug_bounds("applet-table-row-Saved task|completed")
            .is_some()
    );
    vcx.update(|_, cx| cx.set_global(crate::applet_view::CatalogTasks::default()));
    for (call, output, row, progress) in [
        (
            "one",
            r#"[{"content":"Alpha","status":"pending"}]"#,
            "applet-table-row-Alpha|pending",
            "task-count-0/1",
        ),
        (
            "two",
            r#"[{"content":"Beta","status":"completed"},{"content":"Gamma","status":"pending"}]"#,
            "applet-table-row-Beta|completed",
            "task-count-1/2",
        ),
    ] {
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &jcode_sdk::ApiEvent::ToolDone {
                    session_id: "session-a".into(),
                    call_id: call.into(),
                    name: "todo".into(),
                    output: output.into(),
                    error: None,
                    codemode: None,
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(row).is_some(), "missing {row}");
        assert!(vcx.debug_bounds(progress).is_some(), "missing {progress}");
        vcx.update(|_, cx| {
            assert_eq!(
                crate::applet_runtime::get(cx)
                    .host()
                    .borrow()
                    .instance(&id)
                    .unwrap()
                    .instance
                    .document
                    .revision,
                1
            )
        });
    }
    assert!(vcx.debug_bounds("applet-table-row-Alpha|pending").is_none());
}

#[gpui::test]
fn workspace_catalog_todos_only_refresh_the_panel_whose_rows_changed(cx: &mut gpui::TestAppContext) {
    use crate::harness::{CatalogTodos, Update};
    use crate::workspace::Workspace;
    let (other_window, _) = cx.add_window_view(|_, cx| {
        Workspace::for_test(crate::learning::Coach::new(), cx)
    });
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("tasks-session", cx);
        workspace.push_test_panel("unrelated", cx);
        let panel = workspace.test_panel(0).unwrap();
        panel.update(cx, |panel, cx| {
            panel.test_settled_conversation(0, cx);
            panel.items.push(Item::User("Tasks".into()));
            panel.items.push(mount_call("tasks"));
        });
        workspace
    });
    let snapshot: jcode_applet_types::AgentApplets = serde_json::from_value(json!({"instances": [{
        "id": "tasks", "applet": "jcode.agent",
        "placement": {"kind": "inline", "session_id": "server", "anchor": {"kind": "end"}},
        "document": {"revision": 1, "title": "Tasks", "view": {"type": "session_tasks", "title": "Tasks"}}
    }]})).unwrap();
    let unrelated = workspace.read_with(vcx, |workspace, _| workspace.test_panel(1).unwrap());
    let related = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(1440.), px(900.)));
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("tasks-session", &snapshot));
        cx.notify(related.entity_id());
    });
    vcx.run_until_parked();
    let catalog = |name: &str| CatalogTodos {
        rows: [("tasks-session".into(), vec![(name.into(), "completed".into())])].into(),
        progress: [("tasks-session".into(), (1, 1))].into(),
        unfinished: Vec::new(),
    };
    other_window.update(vcx, |workspace, cx| {
        workspace.apply(Update::CatalogTodos(catalog("First")), cx);
    });
    workspace.update(vcx, |workspace, cx| {
        assert!(workspace.apply(Update::CatalogTodos(catalog("First")), cx));
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("applet-table-row-First|completed").is_some());
    let count = |panel: &Entity<Panel>| crate::render_stats::test_renders::count(panel.entity_id());
    let before = (count(&related), count(&unrelated));
    assert!(before.0 > 0 && before.1 > 0, "both panels must render");
    other_window.update(vcx, |workspace, cx| {
        assert!(!workspace.apply(Update::CatalogTodos(catalog("Second")), cx));
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("applet-table-row-First|completed").is_some());
    for name in ["Second", "Second"] {
        workspace.update(vcx, |workspace, cx| {
            assert!(!workspace.apply(Update::CatalogTodos(catalog(name)), cx));
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("applet-table-row-Second|completed").is_some());
        assert_eq!(count(&unrelated), before.1);
    }
    assert_eq!(count(&related), before.0 + 1);
}

#[gpui::test]
fn clean_chat_paints_applet_and_answer_without_tool_log(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.set_clean_chat(Some(true), cx);
        panel.items.push(Item::User("Choose".into()));
        panel.items.push(mount_call("review"));
        panel.items.push(Item::Tool {
            call_id: "decision".into(),
            name: "ask".into(),
            input:
                r#"{"questions":[{"question":"Choose?","options":[{"label":"A"},{"label":"B"}]}]}"#
                    .into(),
            output: "Selected: B".into(),
            done: true,
            error: None,
        });
        panel
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(1, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("applet-action").is_some(),
        "missing native applet"
    );
    assert!(
        vcx.debug_bounds("decision-card").is_some(),
        "missing ask answer"
    );
    assert!(vcx.debug_bounds("tool-header").is_none());
}

#[gpui::test]
fn transcript_applet_keeps_draft_and_routes_real_click(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.items.push(Item::User("Review this".into()));
        panel.items.push(mount_call("review"));
        panel
    });
    let id = agent_instance_id("session-a", "review");
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(1, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    let input = vcx
        .debug_bounds("applet-input")
        .expect("inline input painted");
    vcx.simulate_click(input.center(), gpui::Modifiers::default());
    vcx.simulate_input("note");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host()
                .borrow()
                .state(&id, "note"),
            Some(&json!("note"))
        )
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(2, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.simulate_input("!");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host()
                .borrow()
                .state(&id, "note"),
            Some(&json!("note!"))
        )
    });
    let button = vcx.debug_bounds("applet-action").expect("action painted");
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        let outbox = crate::applet_runtime::get(cx).agent_outbox().borrow();
        assert!(matches!(&outbox[..], [AgentOutbound::Action { session_id, instance, action, state, source_key }] if session_id == "session-a" && instance == "review" && action.action == "approve" && state["note"] == "note!" && source_key.as_deref() == Some("approve")), "{outbox:?}");
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(3, "reset")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host()
                .borrow()
                .state(&id, "note"),
            Some(&json!("reset"))
        )
    });
    let input = vcx.debug_bounds("applet-input").unwrap();
    vcx.simulate_click(input.center(), gpui::Modifiers::default());
    vcx.simulate_input("!");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host()
                .borrow()
                .state(&id, "note"),
            Some(&json!("reset!"))
        )
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(4, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.simulate_input("new");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host()
                .borrow()
                .state(&id, "note"),
            Some(&json!("new"))
        )
    });
}
