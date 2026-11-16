#[gpui::test]
fn queue_card_actions_delete_and_edit(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-card".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.submit_or_queue("first queued message".into(), vec![], true, cx);
        panel.submit_or_queue("second queued message".into(), vec![], true, cx);
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let card = vcx.debug_bounds("queue-card").expect("queue card");
    let composer = vcx.debug_bounds("composer").unwrap();
    assert_eq!(card.left(), composer.left());
    assert_eq!(card.size.width, composer.size.width);
    assert!(card.top() < composer.top());
    for selector in [
        "queue-row-icon-0",
        "queue-row-text-0",
        "queue-mode-button-0",
        "remove-queued-prompt-0",
        "queue-overflow-0",
    ] {
        assert!(vcx.debug_bounds(selector).is_some(), "missing {selector}");
    }
    let delete = vcx.debug_bounds("remove-queued-prompt-0").unwrap().center();
    vcx.simulate_click(delete, Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| assert_eq!(p.prompt_queue.prompts.len(), 1));
    let overflow = vcx.debug_bounds("queue-overflow-0").unwrap().center();
    vcx.simulate_click(overflow, Default::default());
    vcx.run_until_parked();
    let edit = vcx.debug_bounds("recall-queued-prompt-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert!(p.prompt_queue.prompts.is_empty());
        assert_eq!(p.input.read(cx).content, "second queued message");
    });
}

#[gpui::test]
fn queue_card_menu_keyboard_controls(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-keys".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.submit_or_queue("first".into(), vec![], true, cx);
        panel.submit_or_queue("second".into(), vec![], true, cx);
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let overflow = vcx.debug_bounds("queue-overflow-0").unwrap().center();
    vcx.simulate_click(overflow, Default::default());
    vcx.simulate_keystrokes("m alt-down");
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.prompt_queue.prompts[1].content, "first");
        assert!(!panel.prompt_queue.prompts[1].after_yield);
    });
    vcx.simulate_keystrokes("alt-up escape");
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.prompt_queue.prompts[0].content, "first");
        assert!(panel.prompt_queue.card_menu.is_none());
    });
    vcx.run_until_parked();
    let overflow = vcx.debug_bounds("queue-overflow-0").unwrap().center();
    vcx.simulate_click(overflow, Default::default());
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "first"));
}
