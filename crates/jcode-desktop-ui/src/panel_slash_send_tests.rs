use super::*;
use gpui::{ClipboardItem, TestAppContext};

fn composer(
    cx: &mut TestAppContext,
) -> (
    &mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Command>,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("literal-slash", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.load_history(vec![], vec![], cx));
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    (vcx, commands)
}

fn assert_literal(cx: &mut TestAppContext, text: &str, paste: bool, escape: bool) {
    let (vcx, commands) = composer(cx);
    if paste {
        vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.into())));
        vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-v"
        } else {
            "ctrl-v"
        });
    } else {
        vcx.simulate_input(text);
    }
    if escape {
        vcx.simulate_keystrokes("escape");
    }
    vcx.simulate_keystrokes("enter");
    let request = commands.try_recv();
    assert!(
        matches!(&request, Ok(Command::Send { content, .. }) if content == text),
        "not sent literally: {text:?}"
    );
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn unknown_leading_slash_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "/not-a-command", false, false);
}

#[gpui::test]
fn unselected_command_prefix_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "/compac", false, false);
}

#[gpui::test]
fn leading_space_prevents_command_execution(cx: &mut TestAppContext) {
    assert_literal(cx, " /compact", false, false);
}

#[gpui::test]
fn embedded_known_slash_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "Explain /compact please", false, false);
}

#[gpui::test]
fn embedded_slash_opens_a_menu_and_only_explicit_selection_runs(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    vcx.simulate_input("вывывы /");
    assert!(vcx.debug_bounds("slash-command-overlay").is_some());
    vcx.simulate_input("skills");
    vcx.simulate_keystrokes("up enter");
    assert!(matches!(commands.try_recv(), Ok(Command::RuntimeCommand {
        command: jcode_sdk::api::RuntimeCommand::Skills, args, ..
    }) if args.is_empty()));
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn escaped_embedded_slash_sends_the_original_draft(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    let text = "вывывы /skills";
    vcx.simulate_input(text);
    assert!(vcx.debug_bounds("slash-command-overlay").is_some());
    vcx.simulate_keystrokes("up escape enter");
    assert!(matches!(commands.try_recv(), Ok(Command::Send {content, ..}) if content == text));
}

#[gpui::test]
fn embedded_paths_and_fractions_are_text(cx: &mut TestAppContext) {
    assert_literal(cx, "Compare src/main.rs and 1/2", false, false);
}

#[gpui::test]
fn absolute_path_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "/usr/local/bin", false, false);
}

#[gpui::test]
fn space_after_slash_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "/ compact", false, false);
}

#[gpui::test]
fn multiline_with_known_name_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "Explain this:\n/compact", false, false);
}

#[gpui::test]
fn pasted_known_name_is_text(cx: &mut TestAppContext) {
    assert_literal(cx, "/compact", true, false);
}

#[gpui::test]
fn pasted_multiline_with_slashes_preserves_lines(cx: &mut TestAppContext) {
    assert_literal(cx, "/compact\n/path/to/file", true, false);
}

#[gpui::test]
fn escape_makes_known_name_literal(cx: &mut TestAppContext) {
    assert_literal(cx, "/compact", false, true);
}

#[gpui::test]
fn escape_makes_mode_literal(cx: &mut TestAppContext) {
    assert_literal(cx, "/mode", false, true);
}

#[gpui::test]
fn tab_completes_then_enter_runs_exactly_one_command(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    vcx.simulate_input("/skill");
    vcx.simulate_keystrokes("tab");
    assert!(commands.try_recv().is_err());
    vcx.simulate_keystrokes("enter");
    let result = commands.try_recv();
    match result {
        Ok(Command::RuntimeCommand {
            command: jcode_sdk::api::RuntimeCommand::Skills,
            ..
        }) => (),
        Ok(Command::Send { content, .. }) => panic!("sent literal {content:?}"),
        Ok(_) => panic!("sent a different command"),
        Err(error) => panic!("nothing sent: {error}"),
    }
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn escape_overrides_tab_completion(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    vcx.simulate_input("/skill");
    vcx.simulate_keystrokes("tab");
    vcx.simulate_keystrokes("escape enter");
    let result = commands.try_recv();
    match result {
        Ok(Command::Send { content, .. }) => assert_eq!(content, "/skills"),
        Ok(_) => panic!("executed a command"),
        Err(error) => panic!("nothing sent: {error}"),
    }
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn selected_resume_opens_the_picker(cx: &mut TestAppContext) {
    let (vcx, _) = composer(cx);
    vcx.simulate_input("/resume");
    vcx.simulate_keystrokes("enter");
    assert!(vcx.debug_bounds("resume-panel").is_some());
}

#[gpui::test]
fn selected_rewind_retains_its_argument(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    vcx.simulate_input("/rewind 1");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::SessionOperation {
            operation: SessionOperation::Rewind(1),
            ..
        })
    ));
}

#[gpui::test]
fn leading_command_does_not_execute_later_slash_tokens(cx: &mut TestAppContext) {
    let (vcx, commands) = composer(cx);
    vcx.simulate_input("/skills /info");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::RuntimeCommand {
        command: jcode_sdk::api::RuntimeCommand::Skills, args, ..
    }) if args == ["/info"]));
    assert!(commands.try_recv().is_err());
}
