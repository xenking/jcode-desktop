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
