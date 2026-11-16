use super::*;
use jcode_sdk::desktop_control::{
    DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest, DesktopResponse, DesktopTarget,
};

include!("workspace_control_open_tests.rs");

#[gpui::test]
fn click_tool_and_enter_use_targeted_semantic_actions(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.slots[0].panel.update(cx, |p, _| p.control_test_tool());
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-a".into(),
        };
        let req = |action| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action,
        };
        let focus = w.focus_handle.clone();
        window.focus(&focus, cx);
        assert!(matches!(
            w.handle_desktop_request(
                req(DesktopAction::Click {
                    target: target.clone(),
                    selector: "tool:call-1".into(),
                    allow_user_window: false
                }),
                window,
                cx
            ),
            DesktopResponse::Ok { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(
                req(DesktopAction::SetComposer {
                    target: target.clone(),
                    text: "hello".into(),
                    allow_user_window: false
                }),
                window,
                cx
            ),
            DesktopResponse::Ok { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(
                req(DesktopAction::PressKey {
                    target: target.clone(),
                    key: "enter".into(),
                    allow_user_window: false
                }),
                window,
                cx
            ),
            DesktopResponse::Ok { .. }
        ));
        assert!(
            w.slots[0]
                .panel
                .read(cx)
                .input
                .read(cx)
                .snapshot()
                .content
                .is_empty()
        );
        assert!(matches!(
            w.handle_desktop_request(
                req(DesktopAction::Click {
                    target,
                    selector: "unknown".into(),
                    allow_user_window: false
                }),
                window,
                cx
            ),
            DesktopResponse::Error { .. }
        ));
    });
}

#[gpui::test]
fn search_requires_explicit_permission_and_sets_query(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-a".into(),
        };
        let req = |allow| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::Search {
                target: target.clone(),
                query: "needle".into(),
                allow_user_window: allow,
            },
        };
        assert!(matches!(
            w.handle_desktop_request(req(false), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert!(!w.search_screen_open());
        assert!(matches!(
            w.handle_desktop_request(req(true), window, cx),
            DesktopResponse::Ok { .. }
        ));
        let state = w.handle_desktop_request(
            DesktopRequest {
                v: DESKTOP_CONTROL_VERSION,
                action: DesktopAction::State { target },
            },
            window,
            cx,
        );
        assert!(
            matches!(state, DesktopResponse::Ok { result } if result["search"]["query"] == "needle")
        );
    });
}

#[gpui::test]
fn send_requires_agent_owned_draft_and_preserves_user_draft(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-a".into(),
        };
        let req = |action| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action,
        };
        let send = || {
            req(DesktopAction::Send {
                target: target.clone(),
                allow_user_window: false,
            })
        };
        let workspace_focus = w.focus_handle.clone();
        window.focus(&workspace_focus, cx);
        let input = w.slots[0].panel.read(cx).input.clone();
        input.update(cx, |input, cx| input.set_content("user draft".into(), cx));
        assert!(matches!(
            w.handle_desktop_request(send(), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert_eq!(input.read(cx).snapshot().content, "user draft");
        input.update(cx, |input, cx| input.set_content(String::new(), cx));
        let set = req(DesktopAction::SetComposer {
            target: target.clone(),
            text: "agent draft".into(),
            allow_user_window: false,
        });
        assert!(matches!(
            w.handle_desktop_request(set, window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(send(), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert_eq!(input.read(cx).snapshot().content, "");
    });
}

#[gpui::test]
fn expand_tool_requires_exact_existing_call_and_is_idempotent(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.slots[0].panel.update(cx, |p, _| p.control_test_tool());
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-a".into(),
        };
        let req = |selector: &str| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::ExpandTool {
                target: target.clone(),
                selector: selector.into(),
                allow_user_window: false,
            },
        };
        assert!(matches!(
            w.handle_desktop_request(req("missing"), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(req("call-1"), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(req("call-1"), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert!(
            w.slots[0].panel.read(cx).control_state(cx)["expanded_regions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "call-1")
        );
    });
}

#[gpui::test]
fn state_targets_exact_window_and_session(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    workspace.update_in(vcx, |w, window, cx| {
        let window_id = format!(
            "{}:{}",
            std::process::id(),
            window.window_handle().window_id().as_u64()
        );
        let req = |session: &str| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::State {
                target: DesktopTarget {
                    window_id: window_id.clone(),
                    session_id: session.into(),
                },
            },
        };
        assert!(matches!(
            w.handle_desktop_request(req("session-a"), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(req("wrong"), window, cx),
            DesktopResponse::Error { .. }
        ));
        let target = DesktopTarget {
            window_id: window_id.clone(),
            session_id: "session-a".into(),
        };
        let set = |text: &str, allow| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::SetComposer {
                target: target.clone(),
                text: text.into(),
                allow_user_window: allow,
            },
        };
        let focused_input = w.slots[0].panel.read(cx).input_focus_handle(cx);
        window.focus(&focused_input, cx);
        assert!(matches!(
            w.handle_desktop_request(set("unsafe", false), window, cx),
            DesktopResponse::Error { ref code, .. } if code == "user_window_focused"
        ));
        assert!(matches!(
            w.handle_desktop_request(set("existing user draft", true), window, cx),
            DesktopResponse::Ok { .. }
        ));
        let workspace_focus = w.focus_handle.clone();
        window.focus(&workspace_focus, cx);
        assert!(matches!(
            w.handle_desktop_request(set("replacement", false), window, cx),
            DesktopResponse::Error { ref code, .. } if code == "draft_not_empty"
        ));
        assert_eq!(
            w.slots[0].panel.read(cx).input.read(cx).content.as_ref(),
            "existing user draft"
        );
    });
}

#[cfg(unix)]
#[gpui::test]
fn isolated_socket_routes_to_workspace(cx: &mut gpui::TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("jdc-test-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w
    });
    let window_id = vcx.update(|window, _| {
        format!(
            "{}:{}",
            std::process::id(),
            window.window_handle().window_id().as_u64()
        )
    });
    let (server, rx) =
        crate::desktop_control::Server::start_in(dir.path().to_owned(), 999).unwrap();
    let path = server.path().to_owned();
    let target = DesktopTarget {
        window_id,
        session_id: "session-a".into(),
    };
    let client = std::thread::spawn(move || {
        jcode_sdk::desktop_control::request(
            &path,
            DesktopAction::State { target },
            Duration::from_secs(3),
        )
        .unwrap()
    });
    let pending = rx.recv_blocking().unwrap();
    let response = workspace.update_in(vcx, |workspace, window, cx| {
        workspace.handle_desktop_request(pending.request, window, cx)
    });
    pending.reply.send(response).unwrap();
    let result = client.join().unwrap();
    assert!(
        matches!(result, DesktopResponse::Ok { result } if result["panel"]["session_id"] == "session-a")
    );
    drop(server);
}
