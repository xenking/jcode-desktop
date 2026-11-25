use super::*;

#[gpui::test]
fn workspace_commands_route_fork_and_transfer_without_replacing_source(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    while commands.try_recv().is_ok() {}
    for (content, expected_prompt) in [
        ("/fork", None),
        ("/fork side question", Some("side question")),
    ] {
        vcx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let source = workspace.slots[0].panel.entity_id();
                workspace.workspace_slash_command(
                    &WorkspaceSlashCommand {
                        source,
                        content: content.into(),
                    },
                    window,
                    cx,
                );
                assert_eq!(workspace.slots[0].panel.read(cx).session_id, "source");
                assert_eq!(workspace.slots.len(), 1);
            })
        });
        match commands.try_recv().expect("real lifecycle command") {
            Command::Fork { session_id } if content == "/fork" => assert_eq!(session_id, "source"),
            Command::ForkWithPrompt { session_id, prompt } => {
                assert_eq!(session_id, "source");
                assert_eq!(Some(prompt.as_str()), expected_prompt);
            }
            _ => panic!("slash command must not become a generic model prompt"),
        }
    }
    // `/btw` asks in a side card: it must never open a chat of its own.
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: "/btw side question".into(),
                },
                window,
                cx,
            );
            assert_eq!(workspace.slots.len(), 1);
        })
    });
    match commands.try_recv().expect("side question command") {
        Command::SideQuestion {
            session_id,
            question,
        } => {
            assert_eq!(session_id, "source");
            assert_eq!(question, "side question");
        }
        _ => panic!("/btw must ask a side question, not fork a chat"),
    }
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: "/transfer".into(),
                },
                window,
                cx,
            );
        })
    });
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Transfer { session_id }) if session_id == "source"
    ));
}

#[gpui::test]
fn workspace_mirror_is_read_only_live_and_has_independent_scroll(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    while commands.try_recv().is_ok() {}
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: "/splitview on".into(),
                },
                window,
                cx,
            );
        })
    });
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2);
        let mirror = workspace.slots[1].panel.clone();
        assert!(mirror.read(cx).is_side_document());
        assert!(!mirror.read(cx).can_fork());
        workspace.slots[0].panel.update(cx, |panel, cx| {
            panel.workspace_command_notice("New source content", cx)
        });
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        let mirror = workspace.slots[1]
            .panel
            .read(cx)
            .snapshot(cx)
            .side_document
            .unwrap();
        assert!(mirror.page.content.contains("New source content"));
        assert_eq!(mirror.owner_session_id, "desktop-live://source");
    });
    assert!(
        commands.try_recv().is_err(),
        "a mirror does not create a runtime session"
    );
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: "/splitview off".into(),
                },
                window,
                cx,
            );
            assert_eq!(workspace.slots[0].panel.read(cx).session_id, "source");
        })
    });
}

#[gpui::test]
fn workspace_gateway_commands_use_the_source_runtime_transport(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("ssh://host/source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    while commands.try_recv().is_ok() {}
    for (content, command, args) in [
        (
            "/remote status",
            jcode_sdk::RuntimeCommand::Remote,
            vec!["status".to_owned()],
        ),
        (
            "/server-reload",
            jcode_sdk::RuntimeCommand::ServerReload,
            Vec::new(),
        ),
    ] {
        vcx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                let source = workspace.slots[0].panel.entity_id();
                workspace.workspace_slash_command(
                    &WorkspaceSlashCommand {
                        source,
                        content: content.into(),
                    },
                    window,
                    cx,
                );
            })
        });
        match commands.try_recv().expect("server-owned command") {
            Command::RuntimeCommand {
                session_id,
                command: actual,
                args: actual_args,
            } => {
                assert_eq!(session_id, "ssh://host/source");
                assert_eq!(actual, command);
                assert_eq!(actual_args, args);
            }
            _ => panic!(
                "gateway control must use the source server, not local Desktop process state"
            ),
        }
    }
}

#[gpui::test]
fn workspace_resume_preserves_search_and_active_filter(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: "/active exact query".into(),
                },
                window,
                cx,
            );
            match workspace.resume_snapshot(cx) {
                screens::Snapshot::Open {
                    query,
                    search,
                    filter,
                    ..
                } => {
                    assert_eq!(query, "exact query");
                    assert_eq!(search.content, "exact query");
                    assert_eq!(filter, screens::Filter::Active);
                }
                _ => panic!("active opens the native session picker"),
            }
            assert_eq!(workspace.slots[0].panel.read(cx).session_id, "source");
        })
    });
}
