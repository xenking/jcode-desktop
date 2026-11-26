//! Orchestration panel: live sessions, run state and todos, never chat.
use super::tests::click_sidebar_navigation;
use super::*;
use crate::harness::{LiveSession, UnfinishedTodo};
use crate::panel::orchestration::SESSION_ID;

fn live(id: &str, running: bool, todos: &[(&str, &str)]) -> LiveSession {
    LiveSession {
        session_id: id.into(),
        title: format!("{id} title"),
        working_dir: Some("/work".into()),
        running,
        todos: todos
            .iter()
            .map(|(content, status)| UnfinishedTodo {
                content: (*content).into(),
                status: (*status).into(),
                group: None,
            })
            .collect(),
    }
}

fn open(cx: &mut gpui::TestAppContext) -> (Entity<Workspace>, &mut gpui::VisualTestContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("current-chat", cx);
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_active(window, cx));
    });
    vcx.run_until_parked();
    (workspace, vcx)
}

fn orchestration(workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> Entity<Panel> {
    workspace.read_with(cx, |w, cx| {
        w.slots
            .iter()
            .find(|slot| slot.panel.read(cx).session_id == SESSION_ID)
            .expect("orchestration panel is open")
            .panel
            .chat()
            .cloned()
            .unwrap()
    })
}

#[gpui::test]
fn shortcut_opens_one_orchestration_panel_and_reuses_it(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = open(cx);
    for _ in 0..2 {
        vcx.simulate_keystrokes("super-shift-a");
        vcx.run_until_parked();
    }
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots.len(), 2, "reuse never duplicates the panel");
        let panel = w.slots[w.active].panel.read(cx);
        assert_eq!(panel.session_id, SESSION_ID);
        assert!(!panel.can_fork());
        assert!(!panel.supports_voice());
    });
    let panel = orchestration(&workspace, vcx);
    panel.update(vcx, |panel, cx| {
        panel.set_live_sessions(
            vec![
                live(
                    "busy",
                    true,
                    &[("Ship it", "in_progress"), ("Plan", "completed")],
                ),
                live("quiet", false, &[]),
            ],
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("orchestration-list").is_some());
    assert!(vcx.debug_bounds("orchestration-status-0-running").is_some());
    assert!(vcx.debug_bounds("orchestration-status-1-idle").is_some());
    // The panel is a normal slot and restores from the workspace snapshot.
    let snapshot = panel.read_with(vcx, |panel, cx| panel.snapshot(cx));
    assert_eq!(snapshot.session_id, SESSION_ID);
}

#[gpui::test]
fn sidebar_navigation_opens_orchestration(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = open(cx);
    click_sidebar_navigation(&workspace, vcx, "sidebar-orchestration");
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots[w.active].panel.read(cx).session_id, SESSION_ID);
    });
}

#[gpui::test]
fn todo_pill_loads_a_session_plan_without_resuming_the_session(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = open(cx);
    let (bridge, commands) = harness::spawn_recording();
    workspace.update(vcx, |w, cx| {
        w.set_test_bridge(bridge);
        let mut background = super::tests::session_info("background", Some("Background project"));
        background.user_connected = true;
        w.sessions.push(background);
        w.sidebar_todo_progress.insert("background".into(), (0, 1));
        cx.notify();
    });
    vcx.run_until_parked();
    let row = workspace.read_with(vcx, |w, _| {
        w.sidebar_session_layout
            .iter()
            .position(|row| row.session_id == "background")
            .unwrap()
    });
    let pill = vcx
        .debug_bounds(format!("sidebar-todos-{row}").leak())
        .unwrap();
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots[w.active].panel.read(cx).session_id, SESSION_ID);
        assert!(
            w.slots
                .iter()
                .all(|slot| slot.panel.read(cx).session_id != "background")
        );
    });
    let (session_id, reply) = commands
        .try_iter()
        .find_map(|command| match command {
            Command::ReadTodoSnapshot { session_id, reply } => Some((session_id, reply)),
            _ => None,
        })
        .expect("requested the selected session's todo state");
    assert_eq!(session_id, "background");
    let snapshot = serde_json::from_value(serde_json::json!({
        "session_id": session_id,
        "todos": [{"id": "check", "content": "Check", "priority": "high", "status": "in_progress", "stage": 3, "group": "Validation"}]
    })).unwrap();
    assert!(reply.try_send(Some(snapshot)).is_ok());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("orchestration-todos-background").is_some());
    assert!(vcx.debug_bounds("pinned-todo-phase-3").is_some());
    click_sidebar_navigation(&workspace, vcx, "sidebar-orchestration");
    assert!(vcx.debug_bounds("orchestration-todos-background").is_none());
    workspace.read_with(vcx, |w, _| assert_eq!(w.slots.len(), 2));
}

#[gpui::test]
fn clicking_a_live_session_focuses_its_chat(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = open(cx);
    vcx.simulate_keystrokes("super-shift-a");
    vcx.run_until_parked();
    let panel = orchestration(&workspace, vcx);
    panel.update(vcx, |panel, cx| {
        panel.set_live_sessions(vec![live("current-chat", true, &[("Work", "pending")])], cx)
    });
    vcx.run_until_parked();
    let card = vcx
        .debug_bounds("orchestration-session-0")
        .expect("live session row paints");
    vcx.simulate_click(card.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots.len(), 2, "an open chat is reused");
        assert_eq!(w.slots[w.active].panel.read(cx).session_id, "current-chat");
    });
}

#[test]
fn live_sessions_read_titles_and_todos_without_transcripts() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("sessions")).unwrap();
    std::fs::create_dir_all(home.path().join("todos")).unwrap();
    std::fs::write(
        home.path().join("sessions/session_a_1700000000000_x.json"),
        r#"{"title":"Old","custom_title":"Renamed","working_dir":"/repo","messages":[{"text":"secret chat"}]}"#,
    )
    .unwrap();
    std::fs::write(
        home.path().join("todos/session_a_1700000000000_x.json"),
        r#"[{"content":"Done","status":"completed"},{"content":"Doing","status":"in_progress"}]"#,
    )
    .unwrap();
    std::fs::write(
        home.path().join("todos/session_b_1800000000000_y.json"),
        r#"[{"content":"Only todo","status":"pending","group":"Group title"}]"#,
    )
    .unwrap();
    let sessions = crate::harness::live_sessions_from(
        home.path(),
        [
            ("session_a_1700000000000_x".to_owned(), false),
            ("session_b_1800000000000_y".to_owned(), false),
            ("session_c_1600000000000_z".to_owned(), true),
        ],
    );
    let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "session_c_1600000000000_z",
            "session_b_1800000000000_y",
            "session_a_1700000000000_x"
        ],
        "running first, then newest"
    );
    assert_eq!(sessions[2].title, "Renamed");
    assert_eq!(sessions[2].working_dir.as_deref(), Some("/repo"));
    assert_eq!(sessions[2].completed_todos(), 1);
    assert_eq!(sessions[2].todos.len(), 2);
    assert_eq!(sessions[1].title, "Group title", "falls back to todo title");
    assert_eq!(sessions[0].title, "session_c_1600000000000_z");
    assert!(sessions[0].todos.is_empty());
}
