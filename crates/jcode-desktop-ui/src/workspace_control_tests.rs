use super::*;
use jcode_sdk::desktop_control::{
    DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest, DesktopResponse, DesktopTarget,
};

include!("workspace_control_open_tests.rs");

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
