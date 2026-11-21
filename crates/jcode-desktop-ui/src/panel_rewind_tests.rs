use super::*;
use jcode_sdk::api::RewindMessageKey;

#[gpui::test]
fn rewind_uses_transcript_rows_and_returns_focus_to_composer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("rewind", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.load_history(
            [
                ("user", "First prompt"),
                ("assistant", "First answer"),
                ("user", "Second prompt"),
                ("assistant", "Second answer"),
            ]
            .into_iter()
            .map(|(role, text)| {
                serde_json::from_value(serde_json::json!({"role": role, "content": text})).unwrap()
            })
            .collect(),
            vec![],
            cx,
        );
    });
    let enter_mode = |vcx: &mut gpui::VisualTestContext| {
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input_focus_handle(cx);
            window.focus(&focus, cx);
        });
        vcx.simulate_input("/rewind");
        vcx.simulate_keystrokes("enter");
    };
    enter_mode(vcx);
    assert!(vcx.debug_bounds("rewind-keys").is_some());
    assert!(vcx.debug_bounds("rewind-selected-3").is_some());
    vcx.simulate_keystrokes("up");
    assert!(vcx.debug_bounds("rewind-selected-2").is_some());
    vcx.simulate_keystrokes("down");
    assert!(vcx.debug_bounds("rewind-selected-3").is_some());
    vcx.simulate_keystrokes("up left");
    assert!(vcx.debug_bounds("rewind-selected-0").is_some());
    vcx.simulate_keystrokes("right");
    assert!(vcx.debug_bounds("rewind-selected-2").is_some());
    vcx.simulate_keystrokes("escape");
    assert!(vcx.debug_bounds("rewind-keys").is_none());
    assert!(commands.try_recv().is_err());
    vcx.update(|window, cx| {
        assert!(panel.read(cx).input_focus_handle(cx).is_focused(window));
    });

    enter_mode(vcx);
    vcx.simulate_keystrokes("left left");
    let assistant = vcx.debug_bounds("transcript-row-1").unwrap();
    let viewport = panel.read_with(vcx, |panel, _| panel.transcript_list.viewport_bounds());
    assert!(
        viewport.contains(&assistant.center()),
        "{assistant:?} outside {viewport:?}"
    );
    vcx.simulate_click(assistant.center(), gpui::Modifiers::default());
    assert!(vcx.debug_bounds("rewind-selected-1").is_some());
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::SessionOperation {
        ref session_id, operation: SessionOperation::RewindTranscript(target),
    }) if session_id == "rewind" && Some(target.selected) == RewindMessageKey::new("assistant", "First answer")));
    assert!(vcx.debug_bounds("rewind-keys").is_none());
    vcx.update(|window, cx| {
        assert!(panel.read(cx).input_focus_handle(cx).is_focused(window));
    });
}
