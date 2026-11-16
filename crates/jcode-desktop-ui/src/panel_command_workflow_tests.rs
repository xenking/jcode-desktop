use super::*;

#[gpui::test]
fn workflow_stop_cancels_immediately_and_status_is_read_only(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    for name in ["/improve", "/refactor"] {
        let panel =
            cx.new(|cx| Panel::new("workflow-control".into(), None, None, bridge.clone(), cx));
        panel.update(cx, |panel, cx| {
            assert!(panel.handle_slash_command(&format!("{name} stop"), cx));
            assert!(panel.prompt_queue.paused);
            assert!(panel.pending_users.is_empty());
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::Cancel { session_id }) if session_id == "workflow-control")
        );
        panel.update(cx, |panel, cx| {
            assert!(panel.handle_slash_command(&format!("{name} status"), cx));
            assert!(panel.pending_users.is_empty());
        });
        assert!(commands.try_recv().is_err());
    }
}

#[gpui::test]
fn skills_is_a_runtime_operation_not_a_model_turn(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("skills-session".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        assert!(panel.handle_slash_command("/skills", cx));
        assert!(panel.pending_users.is_empty());
        assert!(!panel.items.iter().any(|item| matches!(item, Item::User(_))));
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::RuntimeCommand { session_id, command: jcode_sdk::api::RuntimeCommand::Skills, args }) if session_id == "skills-session" && args.is_empty())
    );
}

#[gpui::test]
fn skill_invocation_is_not_reported_as_unknown(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("skill-session".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.skills = vec![("poteto-mode".into(), "Test skill".into())];
        assert!(panel.handle_slash_command("/poteto-mode hello", cx));
        assert!(
            !panel
                .items
                .iter()
                .any(|item| matches!(item, Item::Error(_)))
        );
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::SendSkill { session_id, content, skill, .. })
        if session_id == "skill-session" && content == "hello" && skill == "poteto-mode")
    );
}

#[gpui::test]
fn selfdev_skill_does_not_require_a_desktop_checkout(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("remote-selfdev".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.skills = vec![("selfdev".into(), "Self development".into())];
        assert!(panel.handle_slash_command("/selfdev fix this", cx));
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::SendSkill { skill, content, .. })
        if skill == "selfdev" && content == "fix this")
    );
}

#[gpui::test]
fn late_skill_catalog_does_not_clear_a_locally_selected_skill(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("catalog-race".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        panel.skills = vec![("selfdev".into(), "Self development".into())];
        assert!(panel.handle_slash_command("/selfdev", cx));
        panel.apply_event(&ApiEvent::RuntimeCommandResult {
            session_id: "catalog-race".into(),
            command: jcode_sdk::api::RuntimeCommand::Skills,
            data: serde_json::json!({"skills": [{"name":"selfdev","description":"Self development"}], "active_skill": null}),
            message: String::new(),
        }, cx);
        assert_eq!(panel.active_skill.as_deref(), Some("selfdev"));
    });
}

#[gpui::test]
fn composer_bare_selfdev_activates_for_next_prompt(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("bare-selfdev", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.skills = vec![("selfdev".into(), "Self development".into())];
        panel
            .input
            .update(cx, |input, cx| input.set_skills(panel.skills.clone(), cx));
    });
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/selfdev");
    vcx.simulate_keystrokes("enter enter");
    assert!(commands.try_recv().is_err());
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.active_skill.as_deref(), Some("selfdev"))
    });
    vcx.simulate_input("hello");
    vcx.simulate_keystrokes("enter");
    assert!(
        matches!(commands.try_recv(), Ok(Command::SendSkill { skill, content, .. })
        if skill == "selfdev" && content == "hello")
    );
}

#[gpui::test]
fn embedded_skill_uses_surrounding_text_and_ignores_paths(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("skill-session".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.skills = vec![("poteto-mode".into(), "Test skill".into())];
        assert!(
            panel
                .embedded_command_sequence("visit https://a/b and /usr/bin")
                .is_none()
        );
        let commands = panel
            .embedded_command_sequence("fix this /poteto-mode please")
            .unwrap();
        for command in commands {
            panel.handle_slash_command(&command, cx);
        }
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::SendSkill { content, skill, .. })
        if content == "fix this please" && skill == "poteto-mode")
    );
}

#[gpui::test]
fn composer_sends_embedded_skill_prompt(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("skill-composer", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.skills = vec![("poteto-mode".into(), "Test skill".into())];
    });
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("fix this /poteto-mode please");
    vcx.simulate_keystrokes("enter");
    assert!(
        matches!(commands.try_recv(), Ok(Command::SendSkill { content, skill, .. })
        if content == "fix this please" && skill == "poteto-mode")
    );
}

#[gpui::test]
fn composer_runs_compact_as_a_session_operation(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("compact-composer", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/compact");
    vcx.simulate_keystrokes("enter enter");
    match commands.try_recv() {
        Ok(Command::SessionOperation {
            session_id,
            operation: SessionOperation::Compact,
        }) => assert_eq!(session_id, "compact-composer"),
        Ok(_) => panic!("received unexpected command"),
        Err(_) => panic!("no command"),
    }
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn composer_dispatches_embedded_command_even_when_prompts_are_queued(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("queued-commands", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.seed_screenshot_queued(1, 0);
    });
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("please /skills now");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::RuntimeCommand {
            command: jcode_sdk::api::RuntimeCommand::Skills,
            args,
            ..
        }) if args == ["please", "now"]
    ));
}

#[gpui::test]
fn composer_dispatches_multiple_commands_in_order(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("command-sequence", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/skills /info");
    vcx.simulate_keystrokes("enter enter");
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::RuntimeCommand {
            command: jcode_sdk::api::RuntimeCommand::Skills,
            ..
        })
    ));
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::RuntimeCommand {
            command: jcode_sdk::api::RuntimeCommand::Info,
            ..
        })
    ));
}

#[gpui::test]
fn composer_routes_embedded_workspace_command(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("workspace-command", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("ask /btw please");
    vcx.simulate_keystrokes("enter");
    assert!(
        matches!(commands.try_recv(), Ok(Command::SideQuestion { question, .. }) if question == "ask please")
    );
}

#[gpui::test]
fn composer_unknown_slash_is_an_inline_error(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("unknown-composer", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/foo");
    vcx.simulate_keystrokes("enter");
    panel.read_with(vcx, |panel, _| assert!(matches!(panel.items.last(), Some(Item::Error(message)) if message.contains("Unknown command"))));
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn unknown_commands_stay_local_and_do_not_start_model_work(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("unknown-session".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        assert!(panel.handle_slash_command("/not-a-command", cx));
        assert!(panel.pending_users.is_empty());
        assert!(matches!(panel.items.last(), Some(Item::Error(message)) if message.contains("Unknown command")));
    });
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn workflows_dispatch_expanded_prompts_and_preserve_arguments(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("workflow-session".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| panel.load_history(vec![], vec![], cx));
    for name in [
        "/plan",
        "/test",
        "/improve",
        "/refactor",
        "/triage",
        "/feedback",
    ] {
        panel.update(cx, |panel, cx| {
            panel.pending_users.clear();
            panel.handle_slash_command(&format!("{name} target 日本語"), cx);
        });
        let Command::Send {
            session_id,
            content,
            images,
        } = commands.try_recv().unwrap()
        else {
            panic!("{name} did not dispatch a prompt");
        };
        assert_eq!(session_id, "workflow-session");
        assert!(content.contains("target 日本語"));
        assert!(!content.starts_with(name));
        assert!(images.is_empty());
    }
}
