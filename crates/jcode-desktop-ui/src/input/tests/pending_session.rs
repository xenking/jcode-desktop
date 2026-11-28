use super::*;

#[gpui::test]
fn pending_commands_preserve_editor_and_explain_readiness(cx: &mut gpui::TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|_, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {
            panic!("pending commands must not invoke submit callbacks");
        });
        input.set_pending_session(true, cx);
        input
    });
    vcx.update(|window, cx| {
        window.focus(&input.read(cx).focus_handle.clone(), cx);
    });
    for command in ["/clear", "/login", "/changelog", "/onboarding-sim"] {
        input.update(vcx, |input, cx| {
            input.content = command.into();
            input.sel.range = command.len()..command.len();
            cx.notify();
        });
        vcx.simulate_keystrokes("enter");
        vcx.simulate_keystrokes("ctrl-enter");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), command);
            assert_eq!(
                input.attachment_notice.as_deref(),
                Some(PENDING_COMMAND_NOTICE)
            );
        });
    }
    input.update(vcx, |input, cx| {
        input.set_pending_session(false, cx);
        assert!(input.attachment_notice.is_none());
        assert_eq!(input.content.as_ref(), "/onboarding-sim");
    });
}

#[gpui::test]
fn shift_modified_enter_space_backspace_edit_the_composer(cx: &mut gpui::TestAppContext) {
    cx.update(bind_keys);
    let submitted = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let sink = submitted.clone();
    let (input, vcx) = cx.add_window_view(move |_, cx| {
        PromptInput::new(cx, "test", move |text, _, _, _| {
            sink.borrow_mut().push(text.to_string());
        })
    });
    vcx.update(|window, cx| {
        window.focus(&input.read(cx).focus_handle.clone(), cx);
    });
    vcx.simulate_input("ab");
    vcx.simulate_keystrokes("shift-enter");
    vcx.simulate_input("c");
    vcx.simulate_keystrokes("shift-space");
    vcx.simulate_input("dx");
    vcx.simulate_keystrokes("shift-backspace");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "ab\nc d")
    });
    assert!(submitted.borrow().is_empty());
    vcx.simulate_keystrokes("enter");
    assert_eq!(submitted.borrow().as_slice(), ["ab\nc d".to_string()]);
}
