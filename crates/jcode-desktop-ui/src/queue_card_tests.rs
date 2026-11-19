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
        "send-queued-prompt-0",
        "recall-queued-prompt-0",
        "remove-queued-prompt-0",
    ] {
        assert!(vcx.debug_bounds(selector).is_some(), "missing {selector}");
    }
    let delete = vcx.debug_bounds("remove-queued-prompt-0").unwrap().center();
    vcx.simulate_click(delete, Default::default());
    vcx.run_until_parked();
    panel.update(vcx, |p, cx| {
        p.input.update(cx, |input, cx| input.set_content("my draft".into(), cx));
    });
    let edit = vcx.debug_bounds("recall-queued-prompt-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.prompts.len(), 1);
        assert_eq!(p.input.read(cx).content, "my draft");
    });
    assert!(vcx.debug_bounds("queue-edit-input").is_some());
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
    let row = vcx.debug_bounds("queue-row-text-0").unwrap().center();
    vcx.simulate_click(row, Default::default());
    vcx.run_until_parked();
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
    let row = vcx.debug_bounds("queue-row-text-0").unwrap().center();
    vcx.simulate_click(row, Default::default());
    vcx.run_until_parked();
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
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    // Edit holds the prompt in its original slot, not in the composer.
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
        assert!(p.input.read(cx).content.is_empty());
    });
    vcx.run_until_parked();
    let row = vcx.debug_bounds("remote-queue-row-0").unwrap();
    let editor = vcx.debug_bounds("queue-edit-input").unwrap();
    assert!(row.left() <= editor.left() && row.right() >= editor.right());
    assert!(row.top() <= editor.top() && row.bottom() >= editor.bottom());
    panel.update_in(vcx, |p, window, cx| p.cancel_queued_prompt_edit(window, cx));
    vcx.run_until_parked();
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok()).any(|command| matches!(
            command,
            Command::SetQueuedPromptEditing { editing: false, .. }
        )),
        "cancelling releases the daemon hold"
    );
    let delete = vcx.debug_bounds("remote-queue-delete-0").unwrap().center();
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
    let row = vcx
        .debug_bounds("queue-row-text-remote-0")
        .unwrap()
        .center();
    vcx.simulate_click(row, Default::default());
    vcx.run_until_parked();
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
    assert!(items.size.height <= px(170.));
    let text = vcx.debug_bounds("queue-row-text-0").unwrap().center();
    vcx.simulate_click(text, Default::default());
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.prompt_queue.card_expanded.as_deref(), Some("local-0"))
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-queue-items").unwrap().size.height <= px(170.));
    vcx.simulate_click(text, Default::default());
    panel.read_with(vcx, |p, _| assert!(p.prompt_queue.card_expanded.is_none()));
}

#[gpui::test]
fn queue_card_matches_composer_and_never_blocks_the_transcript(cx: &mut gpui::TestAppContext) {
    let (_panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-surface".into(),
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
    vcx.simulate_window_resize(handle, gpui::size(px(900.), px(620.)));
    vcx.run_until_parked();
    let card = vcx.debug_bounds("queue-card").expect("queue card");
    let composer = vcx.debug_bounds("composer").expect("composer");
    // The card is part of the chat column: same left and right edge as the
    // composer below it, never a wider floating panel.
    assert_eq!(card.left(), composer.left(), "card left edge");
    assert_eq!(card.right(), composer.right(), "card right edge");
    assert_eq!(card.size.width, composer.size.width, "card width");
    assert!(card.bottom() <= composer.top(), "card sits above the composer");
    let surface = super::card::queue_card_surface(Theme::global());
    assert!(
        surface.a <= 0.06,
        "the queue card must stay translucent, got alpha {}",
        surface.a
    );
}

#[gpui::test]
fn queue_row_edit_is_inline_without_an_overflow_menu(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-inline".into(),
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
    assert!(
        vcx.debug_bounds("recall-queued-prompt-0").is_some(),
        "Edit belongs to the row itself"
    );
    assert!(
        vcx.debug_bounds("queue-overflow-0").is_none(),
        "there is no overflow menu to open first"
    );
    assert!(vcx.debug_bounds("send-queued-prompt-0").is_some());
    assert!(vcx.debug_bounds("remove-queued-prompt-0").is_some());
    let edit = vcx.debug_bounds("recall-queued-prompt-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.prompts.len(), 2, "Edit never takes the message out of the queue");
        assert!(p.input.read(cx).content.is_empty(), "the composer is not involved");
    });
    assert!(vcx.debug_bounds("queue-edit-input").is_some());
}

#[gpui::test]
fn remote_queue_row_edit_is_inline(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-remote-inline".into(), None, None, bridge, cx);
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
    assert!(
        vcx.debug_bounds("remote-queue-edit-0").is_some(),
        "daemon rows edit in place without an overflow menu"
    );
    assert!(vcx.debug_bounds("remote-queue-overflow-0").is_none());
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    assert!(std::iter::from_fn(|| commands.try_recv().ok()).any(|command| matches!(
        command,
        Command::SetQueuedPromptEditing { editing: true, .. }
    )));
    panel.read_with(vcx, |p, _| assert_eq!(p.prompt_queue.remote.len(), 2));
}

#[gpui::test]
fn escape_dismisses_row_actions_without_pausing_the_queue(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-escape".into(), None, None, bridge, cx);
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
    let row = vcx.debug_bounds("queue-row-text-0").unwrap().center();
    vcx.simulate_click(row, Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| {
        assert!(p.prompt_queue.card_menu.is_some(), "click selects the row");
        assert!(p.prompt_queue.card_expanded.is_some());
    });
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| {
        assert!(p.prompt_queue.card_menu.is_none(), "Esc closes row actions");
        assert!(p.prompt_queue.card_expanded.is_none(), "Esc collapses the row");
        assert!(!p.prompt_queue.paused, "Esc must not pause the queue");
        assert_eq!(p.prompt_queue.prompts.len(), 2, "Esc keeps every queued prompt");
    });
    assert!(
        commands.try_recv().is_err(),
        "Esc must not cancel the turn or send anything"
    );
    assert!(vcx.debug_bounds("queue-resume").is_none());
}

#[gpui::test]
fn paused_queue_is_labelled_in_the_header_and_resumes(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-paused".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "idle".into();
        panel.items.push(Item::User("initial".into()));
        for content in ["first queued message", "second queued message"] {
            panel.prompt_queue.prompts.push(QueuedPrompt {
                files: Vec::new(),
                content: content.into(),
                images: vec![],
                active_skill: None,
                after_yield: true,
                send_now: false,
            });
        }
        panel.prompt_queue.paused = true;
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let resume = vcx
        .debug_bounds("queue-resume")
        .expect("the paused state is labelled in the card header");
    let first_row = vcx.debug_bounds("queued-prompt-0").unwrap();
    assert!(
        resume.bottom() <= first_row.top(),
        "the label lives in the header, not as a mystery button below the rows"
    );
    assert!(vcx.debug_bounds("resume-prompt-queue").is_none());
    vcx.simulate_click(resume.center(), Default::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| {
        assert!(!p.prompt_queue.paused);
        assert!(p.prompt_queue.prompts.is_empty());
    });
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::Send { content, .. })
            if content == "first queued message\n\nsecond queued message"
    ));
}

#[gpui::test]
fn reconnect_waits_neutrally_then_flushes_the_queue(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-reconnect".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "lost: connection reset".into();
        panel.prompt_queue.remote_supported = true;
        panel.prompt_queue.waiting_for_connection = true;
        panel.items.push(Item::User("initial".into()));
        panel.prompt_queue.prompts.push(QueuedPrompt {
            files: Vec::new(),
            content: "typed during the outage".into(),
            images: vec![],
            active_skill: None,
            after_yield: true,
            send_now: false,
        });
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("queue-waiting").is_some(),
        "a reconnect is a neutral waiting state, not a failure"
    );
    assert!(vcx.debug_bounds("queue-send-all-steer").is_none());
    assert!(vcx.debug_bounds("queue-send-all-after-yield").is_none());
    // Asking to send everything while offline records the intent instead of
    // leaving a permanent error behind.
    panel.update(vcx, |p, cx| p.interrupt_all_queued_prompts(cx));
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| {
        assert!(p.prompt_queue.send_all_on_connect);
        assert_eq!(p.prompt_queue.prompts.len(), 1, "the queue is preserved");
    });
    assert!(commands.try_recv().is_err(), "nothing is sent while offline");
    // The session comes back: the queue flushes itself and honours Send all now.
    panel.update(vcx, |p, cx| {
        p.apply(
            &event(json!({"ev":"session_status", "status":"connected"})),
            cx,
        )
    });
    vcx.run_until_parked();
    let queued = std::iter::from_fn(|| commands.try_recv().ok()).find_map(|command| match command {
        Command::QueuePrompt { prompt, .. } => Some(prompt),
        _ => None,
    });
    let queued = queued.expect("the outage prompt reaches the daemon queue on reconnect");
    assert_eq!(queued.content, "typed during the outage");
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok()).any(|command| matches!(
            command,
            Command::InterruptQueuedPrompts { ids, .. } if ids == [queued.client_id.clone()]
        )),
        "Send all now runs as soon as the session is back"
    );
    panel.read_with(vcx, |p, _| {
        assert!(!p.prompt_queue.waiting_for_connection);
        assert!(!p.prompt_queue.send_all_on_connect);
        assert!(p.prompt_queue.prompts.is_empty());
    });
    assert!(vcx.debug_bounds("queue-waiting").is_none());
}
