#[gpui::test]
fn background_open_preserves_focused_panel(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.sessions.push(jcode_sdk::SessionInfo {
            session_id: "agent".into(),
            title: Some("agent".into()),
            working_dir: None,
            status: "idle".into(),
            user_connected: false,
            transcript_bytes: None,
            saved: false,
            save_label: None,
            updated_at_ms: None,
            last_active_at_ms: None,
            archived: false,
            archived_at_ms: None,
            parent_session_id: None,
            agent_label: None,
            swarm_status: None,
            edit_stats: None,
            context: None,
        });
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let prior = w.slots[w.active].panel.entity_id();
        let target_window = format!(
            "{}:{}",
            std::process::id(),
            window.window_handle().window_id().as_u64()
        );
        let agent = DesktopTarget { window_id: target_window.clone(), session_id: "agent".into() };
        let user = w.slots[w.active].panel.read(cx).input.clone();
        user.update(cx, |input, cx| input.set_content("unsent draft".into(), cx));
        let action = DesktopAction::OpenSession {
            session_id: "agent".into(),
            destination: jcode_sdk::desktop_control::OpenDestination::BackgroundPanel {
                window_id: target_window,
                workspace: None,
            },
        };
        let response = w.handle_desktop_request(
            DesktopRequest {
                v: DESKTOP_CONTROL_VERSION,
                action,
            },
            window,
            cx,
        );
        assert!(
            matches!(response, DesktopResponse::Ok { .. }),
            "{response:?}"
        );
        assert_eq!(w.slots[w.active].panel.entity_id(), prior);
        assert_eq!(
            w.slots
                .iter()
                .filter(|slot| slot.panel.read(cx).session_id == "agent")
                .count(),
            1
        );
        for (allow_user_window, expected_success) in [(false, false), (true, true)] {
            let response = w.handle_desktop_request(
                DesktopRequest {
                    v: DESKTOP_CONTROL_VERSION,
                    action: DesktopAction::Focus { target: agent.clone(), allow_user_window },
                },
                window,
                cx,
            );
            assert_eq!(matches!(response, DesktopResponse::Ok { .. }), expected_success);
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, if expected_success { "agent" } else { "user" });
            assert_eq!(user.read(cx).snapshot().content, "unsent draft");
        }
    });
}

fn catalog_session(session_id: &str, title: &str) -> jcode_sdk::SessionInfo {
    jcode_sdk::SessionInfo {
        session_id: session_id.into(),
        title: Some(title.into()),
        working_dir: None,
        status: "idle".into(),
        user_connected: false,
        transcript_bytes: None,
        saved: false,
        save_label: None,
        updated_at_ms: None,
        last_active_at_ms: None,
        archived: false,
        archived_at_ms: None,
        parent_session_id: None,
        agent_label: None,
        swarm_status: None,
        edit_stats: None,
        context: None,
    }
}

fn open_in_workspace(
    w: &mut Workspace,
    session_id: &str,
    workspace: Option<usize>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> DesktopResponse {
    let window_id = format!(
        "{}:{}",
        std::process::id(),
        window.window_handle().window_id().as_u64()
    );
    w.handle_desktop_request(
        DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::OpenSession {
                session_id: session_id.into(),
                destination: jcode_sdk::desktop_control::OpenDestination::BackgroundPanel {
                    window_id,
                    workspace,
                },
            },
        },
        window,
        cx,
    )
}

#[gpui::test]
fn open_in_an_inactive_workspace_leaves_the_active_one_untouched(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.sessions.push(catalog_session("review", "Reviewer c!592"));
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let (active, active_row) = (w.active, w.active_row);
        let focused = w.slots[w.active].panel.entity_id();
        let width = w.slots[0].width_fraction;
        let response = open_in_workspace(w, "review", Some(4), window, cx);
        let DesktopResponse::Ok { result } = response else {
            panic!("{response:?}");
        };
        assert_eq!(result["workspace"], 4);
        assert_eq!(result["session_id"], "review");
        assert_eq!((w.active, w.active_row), (active, active_row));
        assert_eq!(w.slots[w.active].panel.entity_id(), focused);
        assert_eq!(w.slots[0].width_fraction, width);
        let opened = w
            .slots
            .iter()
            .find(|slot| slot.panel.read(cx).session_id == "review")
            .unwrap();
        assert_eq!(opened.row, 3);
        assert_eq!(result["panel_id"], opened.panel.entity_id().as_u64());
        assert_eq!(opened.panel.read(cx).title.as_ref(), "Reviewer c!592");
        assert_eq!(w.row_indices(active_row).count(), 1);
    });
}

#[gpui::test]
fn workspaces_outside_the_strip_are_rejected(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.sessions.push(catalog_session("review", "Reviewer"));
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        for number in [0, ROWS_PER_SPACE + 1] {
            let response = open_in_workspace(w, "review", Some(number), window, cx);
            assert!(
                matches!(&response, DesktopResponse::Error { code, .. } if code == "workspace_out_of_range"),
                "{response:?}"
            );
        }
        assert_eq!(w.slots.len(), 1);
    });
}

#[gpui::test]
fn reopening_in_another_workspace_reports_already_open(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.sessions.push(catalog_session("review", "Reviewer"));
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        assert!(matches!(
            open_in_workspace(w, "review", Some(4), window, cx),
            DesktopResponse::Ok { .. }
        ));
        for workspace in [Some(4), Some(2), None] {
            let response = open_in_workspace(w, "review", workspace, window, cx);
            assert!(
                matches!(&response, DesktopResponse::Error { code, .. } if code == "already_open"),
                "{response:?}"
            );
        }
        assert_eq!(w.slots.len(), 2);
    });
}

#[gpui::test]
fn a_missing_workspace_defaults_to_the_active_one(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.active_row = 1;
        w.sessions.push(catalog_session("review", "Reviewer"));
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let response = open_in_workspace(w, "review", None, window, cx);
        let DesktopResponse::Ok { result } = response else {
            panic!("{response:?}");
        };
        assert_eq!(result["workspace"], 2);
        assert_eq!(w.active_row, 1);
        assert_eq!(w.slots.last().unwrap().row, 1);
    });
}

#[gpui::test]
fn a_panel_opened_in_an_inactive_workspace_survives_a_restart(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("user", cx);
        w.sessions.push(catalog_session("review", "Reviewer"));
        w
    });
    let snapshot = workspace.update_in(vcx, |w, window, cx| {
        assert!(matches!(
            open_in_workspace(w, "review", Some(4), window, cx),
            DesktopResponse::Ok { .. }
        ));
        w.snapshot(window, cx).unwrap()
    });
    let snapshot: WorkspaceSnapshot =
        serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap();
    let (restored, vcx) = cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    restored.update(vcx, |w, cx| {
        w.apply_snapshot(snapshot, cx);
        assert_eq!(w.active_row, 0);
        let review = w
            .slots
            .iter()
            .find(|slot| slot.panel.read(cx).session_id == "review")
            .unwrap();
        assert_eq!(review.row, 3);
    });
}
