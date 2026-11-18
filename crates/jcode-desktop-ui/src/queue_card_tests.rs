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

#[gpui::test]
fn queue_card_remote_actions_wait_for_confirmation(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-remote".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.seed_screenshot_queued(2, 0);
        panel.prompt_queue.remote_editing_supported = true;
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let overflow = vcx
        .debug_bounds("remote-queue-overflow-0")
        .unwrap()
        .center();
    vcx.simulate_click(overflow, Default::default());
    vcx.run_until_parked();
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    // Edit holds the prompt on the daemon and loads it into the composer; it
    // never removes it, so a turn that ends mid-edit cannot lose the slot.
    let held = std::iter::from_fn(|| commands.try_recv().ok()).find_map(|command| match command {
        Command::SetQueuedPromptEditing {
            prompt_id,
            editing: true,
            ..
        } => Some(prompt_id),
        Command::RemoveQueuedPrompt { .. } => panic!("Edit must not remove the queued prompt"),
        _ => None,
    });
    let first = panel.read_with(vcx, |p, _| p.prompt_queue.remote[0].clone());
    assert_eq!(held.as_deref(), Some(first.client_id.as_str()));
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.remote.len(), 2, "the queue keeps the held prompt");
        assert_eq!(p.input.read(cx).content.as_ref(), first.content);
    });
    panel.update_in(vcx, |p, window, cx| p.cancel_queued_prompt_edit(window, cx));
    vcx.run_until_parked();
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok()).any(|command| matches!(
            command,
            Command::SetQueuedPromptEditing { editing: false, .. }
        )),
        "cancelling releases the daemon hold"
    );
    let delete = vcx.debug_bounds("remote-queue-delete").unwrap().center();
    vcx.simulate_click(delete, Default::default());
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::RemoveQueuedPrompt { edit: false, .. })
    ));
}

#[gpui::test]
fn queue_card_remote_outcomes_and_keyboard(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-remote-keys".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.seed_screenshot_queued(2, 0);
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let overflow = vcx
        .debug_bounds("remote-queue-overflow-0")
        .unwrap()
        .center();
    vcx.simulate_click(overflow, Default::default());
    vcx.simulate_keystrokes("alt-down m");
    assert!(
        matches!(commands.try_recv(), Ok(Command::ReorderQueuedPrompts { order, .. }) if order == ["screenshot-0-1", "screenshot-0-0"])
    );
    assert!(
        matches!(commands.try_recv(), Ok(Command::SetQueuedPromptMode { client_id, mode: jcode_sdk::api::QueuedPromptMode::AfterYield, .. }) if client_id == "screenshot-0-0")
    );
    vcx.simulate_keystrokes("escape");
    panel.update(vcx, |p, cx| {
        let prompt = p.prompt_queue.remote[0].clone();
        p.queue_removal_settled(
            prompt,
            true,
            Ok(jcode_sdk::api::QueuedPromptRemovalOutcome::AlreadyDelivered),
            cx,
        );
    });
    panel.read_with(vcx, |p, cx| {
        assert!(p.input.read(cx).content.is_empty());
        assert_eq!(p.prompt_queue.remote.len(), 2);
    });
    panel.update(vcx, |p, cx| {
        let prompt = p.prompt_queue.remote[0].clone();
        p.queue_removal_settled(prompt, true, Err("connection lost".into()), cx);
    });
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.input.read(cx).content, "also update the changelog before you finish");
        assert!(matches!(p.items.last(), Some(Item::Error(message)) if message.contains("could not be confirmed")));
    });
}

#[gpui::test]
fn queue_card_scrolls_and_expands_long_text(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-many".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        for _ in 0..6 {
            panel.submit_or_queue("A long queued message ".repeat(30), vec![], true, cx);
        }
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let items = vcx.debug_bounds("prompt-queue-items").unwrap();
    assert!(items.size.height <= px(144.));
    let text = vcx.debug_bounds("queue-row-text-0").unwrap().center();
    vcx.simulate_click(text, Default::default());
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.prompt_queue.card_expanded.as_deref(), Some("local-0"))
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-queue-items").unwrap().size.height <= px(144.));
    vcx.simulate_click(text, Default::default());
    panel.read_with(vcx, |p, _| assert!(p.prompt_queue.card_expanded.is_none()));
}
