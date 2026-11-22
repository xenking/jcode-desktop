use super::*;
use crate::applet_runtime::{AgentOutbound, agent_instance_id};
use serde_json::json;

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
    assert!(
        vcx.debug_bounds("task-count-0/0")
            .is_some()
    );
    vcx.update(|_, cx| cx.set_global(crate::applet_view::CatalogTasks {
        rows: [("session-a".into(), vec![("Saved task".into(), "completed".into())])].into(),
        progress: [("session-a".into(), (1,1))].into(),
    }));
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("task-count-1/1").is_some());
    assert!(vcx.debug_bounds("applet-table-row-Saved task|completed").is_some());
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
                    .host
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
                .host
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
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("note!"))
        )
    });
    let button = vcx.debug_bounds("applet-action").expect("action painted");
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        let outbox = crate::applet_runtime::get(cx).agent_outbox.borrow();
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
                .host
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
                .host
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
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("new"))
        )
    });
}
