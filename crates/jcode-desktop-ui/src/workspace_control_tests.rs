use super::*;

#[gpui::test]
fn click_ask_option_requires_permission_and_exact_request(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.slots[0].panel.update(cx, |p, _| p.control_test_ask());
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
        let click = |selector: &str, allow| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::Click {
                target: target.clone(),
                selector: selector.into(),
                allow_user_window: allow,
            },
        };
        assert!(matches!(
            w.handle_desktop_request(click("ask:request-1:0:1", false), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(click("ask:wrong:0:1", true), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert!(matches!(
            w.handle_desktop_request(click("ask:request-1:0:1", true), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert_eq!(
            w.slots[0].panel.read(cx).control_state(window, cx).unwrap()["ask"]["chosen"],
            serde_json::json!([[1]])
        );
    });
}
use jcode_sdk::desktop_control::{
    DESKTOP_CONTROL_VERSION, DesktopAction, DesktopRequest, DesktopResponse, DesktopTarget,
};

include!("workspace_control_open_tests.rs");

#[cfg(unix)]
#[gpui::test]
fn state_returns_visible_applet_without_adding_it_to_history(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.slots[0].panel.update(cx, |p, _| p.control_test_tool());
        let applets = serde_json::from_value(serde_json::json!({"instances": [{
            "id": "review", "applet": "jcode.agent",
            "placement": {"kind": "inline", "session_id": "session-a", "anchor": {"kind": "end"}},
            "document": {"revision": 1, "title": "Review", "view": {"type": "text", "text": "Check this"}}
        }]})).unwrap();
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &applets));
        w
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let response = control::handle_window_request(
            &workspace,
            DesktopRequest {
                v: DESKTOP_CONTROL_VERSION,
                action: DesktopAction::State {
                    target: DesktopTarget {
                        window_id: format!(
                            "{}:{}",
                            std::process::id(),
                            window.window_handle().window_id().as_u64()
                        ),
                        session_id: "session-a".into(),
                    },
                },
            },
            false,
            window,
            cx,
        );
        let DesktopResponse::Ok { result } = response else {
            panic!("state failed: {response:?}");
        };
        let panel = &result["panel"];
        let item = panel["visible_transcript_items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["item"]["Applet"]
                    == crate::applet_runtime::agent_instance_id("session-a", "review")
            })
            .expect("visible native applet");
        assert!(item["selector"].as_str().unwrap().starts_with("row:"));
        assert_eq!(
            panel["loaded_transcript_items"].as_array().unwrap().len(),
            1
        );
    });
}

#[gpui::test]
fn state_reports_actually_rendered_tool_rows(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.slots[0].panel.update(cx, |p, _| p.control_test_tool());
        w
    });
    vcx.run_until_parked();
    workspace.update_in(vcx, |w, window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-a".into(),
        };
        let result = w.handle_desktop_request(
            DesktopRequest {
                v: DESKTOP_CONTROL_VERSION,
                action: DesktopAction::State { target },
            },
            window,
            cx,
        );
        let DesktopResponse::Ok { result } = result else {
            panic!("state failed")
        };
        let visible = result["panel"]["visible_transcript_items"]
            .as_array()
            .expect("visible rows");
        assert!(
            visible
                .iter()
                .any(|item| item.to_string().contains("call-1")),
            "{visible:?}"
        );
        assert!(
            w.slots[0]
                .panel
                .read(cx)
                .control_element_bounds("tool:call-1", window)
                .is_some()
        );
        assert!(
            w.slots[0]
                .panel
                .read(cx)
                .control_element_bounds("tool:missing", window)
                .is_none()
        );
    });
}

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
        assert!(
            w.slots[0].panel.read(cx).control_state(window, cx).unwrap()["expanded_regions"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("call-1"))
        );
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
        assert!(
            !w.slots[0].panel.read(cx).control_state(window, cx).unwrap()["expanded_regions"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("call-1"))
        );
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
        assert!(!w.resume_open());
        assert!(matches!(
            w.handle_desktop_request(req(true), window, cx),
            DesktopResponse::Ok { .. }
        ));
        let state = w.handle_desktop_request(
            DesktopRequest {
                v: DESKTOP_CONTROL_VERSION,
                action: DesktopAction::State {
                    target: target.clone(),
                },
            },
            window,
            cx,
        );
        assert!(
            matches!(state, DesktopResponse::Ok { result } if result["search"]["query"] == "needle")
        );
        let escape = |allow| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::PressKey {
                target: target.clone(),
                key: "escape".into(),
                allow_user_window: allow,
            },
        };
        assert!(matches!(
            w.handle_desktop_request(escape(false), window, cx),
            DesktopResponse::Error { .. }
        ));
        assert!(w.resume_open());
        assert!(matches!(
            w.handle_desktop_request(escape(true), window, cx),
            DesktopResponse::Ok { .. }
        ));
        assert!(!w.resume_open());
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
        let replace = req(DesktopAction::SetComposer {
            target: target.clone(),
            text: "overwrite".into(),
            allow_user_window: false,
        });
        assert!(matches!(
            w.handle_desktop_request(replace, window, cx),
            DesktopResponse::Error { ref code, .. } if code == "draft_not_empty"
        ));
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
        input.update(cx, |input, cx| {
            gpui::EntityInputHandler::replace_text_in_range(input, None, "x", window, cx);
            gpui::EntityInputHandler::replace_text_in_range(input, Some(11..12), "", window, cx);
        });
        assert_eq!(input.read(cx).snapshot().content, "agent draft");
        assert!(matches!(
            w.handle_desktop_request(send(), window, cx),
            DesktopResponse::Error { ref code, .. } if code == "draft_not_owned"
        ));
        let replace = req(DesktopAction::SetComposer {
            target: target.clone(),
            text: "overwrite".into(),
            allow_user_window: false,
        });
        assert!(matches!(
            w.handle_desktop_request(replace, window, cx),
            DesktopResponse::Error { ref code, .. } if code == "draft_not_empty"
        ));
        input.update(cx, |input, cx| input.set_content(String::new(), cx));
        let set = req(DesktopAction::SetComposer {
            target: target.clone(), text: "agent draft".into(), allow_user_window: false,
        });
        assert!(matches!(w.handle_desktop_request(set, window, cx), DesktopResponse::Ok { .. }));
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
            w.slots[0].panel.read(cx).control_state(window, cx).unwrap()["expanded_regions"]
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
fn desktop_control_pointer_move_can_update_workspace(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session-a", cx);
        w.push_test_panel("session-b", cx);
        w
    });
    vcx.run_until_parked();
    let position = vcx.debug_bounds("live-session-tab-1-slot").unwrap().center();
    vcx.update(|window, cx| {
        let target = DesktopTarget {
            window_id: format!(
                "{}:{}",
                std::process::id(),
                window.window_handle().window_id().as_u64()
            ),
            session_id: "session-b".into(),
        };
        let selector = format!(
            "proof-pointer:{},{}",
            f32::from(position.x),
            f32::from(position.y)
        );
        let request = |allow_user_window| DesktopRequest {
            v: DESKTOP_CONTROL_VERSION,
            action: DesktopAction::Click {
                target: target.clone(),
                selector: selector.clone(),
                allow_user_window,
            },
        };
        assert_ne!(window.mouse_position(), position);
        for (allow_user_window, fixture_enabled) in [(false, true), (true, false)] {
            assert!(matches!(
                control::handle_window_request(
                    &workspace,
                    request(allow_user_window),
                    fixture_enabled,
                    window,
                    cx
                ),
                DesktopResponse::Error { code, .. } if code == "user_window_protected"
            ));
        }
        assert_ne!(window.mouse_position(), position);
        assert!(matches!(
            control::handle_window_request(&workspace, request(true), true, window, cx),
            DesktopResponse::Ok { result } if result["selector"] == selector
        ));
        assert_eq!(window.mouse_position(), position);
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
    let response = vcx.update(|window, cx| {
        control::handle_window_request(&workspace, pending.request, false, window, cx)
    });
    pending.reply.send(response).unwrap();
    let result = client.join().unwrap();
    assert!(
        matches!(result, DesktopResponse::Ok { result } if result["panel"]["session_id"] == "session-a")
    );
    drop(server);
}
