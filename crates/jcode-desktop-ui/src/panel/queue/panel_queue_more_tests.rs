use super::*;

#[gpui::test]
fn startup_enter_and_ctrl_enter_queue_until_attached_history_ready(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(Panel::STARTUP_SESSION_ID.into(), None, None, bridge, cx)
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).editor_focus();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("first startup prompt");
    vcx.simulate_keystrokes("enter");
    vcx.simulate_input("second startup prompt");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.run_until_parked();
    assert!(commands.try_recv().is_err());
    assert!(vcx.debug_bounds("prompt-queue").is_some());
    panel.read_with(vcx, |p, cx| {
        assert!(p.input.read(cx).content.is_empty());
        assert_eq!(p.prompt_queue.prompts.len(), 2);
        assert!(p.prompt_queue.waiting_for_connection);
        assert!(p.items.is_empty());
    });
    panel.update(vcx, |p, cx| {
        p.attach_startup_session(attached_session("session-ready"), cx);
        assert!(!p.history_loaded);
    });
    assert!(
        commands.try_recv().is_err(),
        "attachment alone must not drain"
    );
    panel.update(vcx, |p, cx| {
        // The real bridge sends SessionConnected before History, without
        // necessarily producing an idle event for a newly created session.
        p.status = "connected".into();
        p.load_history(vec![], vec![], cx);
        p.load_history(vec![], vec![], cx);
        p.send_queued_prompts(cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { session_id, content, images, .. })
        if session_id == "session-ready" && content == "first startup prompt\n\nsecond startup prompt" && images.is_empty())
    );
    assert!(commands.try_recv().is_err(), "readiness cannot send twice");
}

#[gpui::test]
fn startup_queue_preserves_images_through_snapshot_and_waits_for_identity(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new(Panel::STARTUP_SESSION_ID.into(), None, None, bridge, cx));
    let images = vec![("image/png".into(), "encoded image".into())];
    panel.update(cx, |p, cx| {
        p.submit_or_queue("with attachment".into(), images.clone(), false, cx);
        let saved = serde_json::to_vec(&p.snapshot(cx)).unwrap();
        p.prompt_queue = Default::default();
        p.restore_snapshot(serde_json::from_slice(&saved).unwrap(), cx);
        assert!(p.prompt_queue.waiting_for_connection);
        assert_eq!(p.prompt_queue.prompts[0].images, images);
        p.status = "idle".into();
        p.load_history(vec![], vec![], cx);
    });
    assert!(
        commands.try_recv().is_err(),
        "history cannot send to a draft ID"
    );
    panel.update(cx, |p, cx| {
        p.attach_startup_session(attached_session("local-recovery"), cx);
        p.send_queued_prompts(cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { session_id, content, images: actual, .. })
        if session_id == "local-recovery" && content == "with attachment" && actual == images)
    );
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn startup_slash_command_stays_in_editor_until_explicit_ready_submission(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(Panel::STARTUP_SESSION_ID.into(), None, None, bridge, cx)
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).editor_focus();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/clear");
    vcx.simulate_keystrokes("enter");
    assert!(commands.try_recv().is_err());
    panel.read_with(vcx, |p, cx| {
        assert!(p.prompt_queue.prompts.is_empty());
        assert_eq!(p.input.read(cx).content.as_ref(), "/clear");
    });
    vcx.simulate_keystrokes("ctrl-enter");
    assert!(commands.try_recv().is_err());
    panel.update(vcx, |p, cx| {
        p.attach_startup_session(attached_session("session-ready"), cx);
    });
    vcx.simulate_keystrokes("enter");
    assert!(
        commands.try_recv().is_err(),
        "commands also wait for history"
    );
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.input.read(cx).content.as_ref(), "/clear");
        assert!(p.prompt_queue.prompts.is_empty());
    });
    panel.update(vcx, |p, cx| {
        p.load_history(vec![], vec![], cx);
        p.send_queued_prompts(cx);
    });
    assert!(
        commands.try_recv().is_err(),
        "connecting does not execute editor commands"
    );
    vcx.simulate_keystrokes("enter");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::SessionOperation {
        session_id, operation: SessionOperation::Clear,
    }) if session_id == "session-ready"));
    assert!(
        commands.try_recv().is_err(),
        "slash command is not a chat prompt"
    );
}

#[gpui::test]
fn removing_all_startup_prompts_clears_waiting_state_when_ready(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new(Panel::STARTUP_SESSION_ID.into(), None, None, bridge, cx));
    panel.update(cx, |p, cx| {
        p.submit_or_queue("remove this".into(), vec![], false, cx);
        p.prompt_queue.prompts.clear();
        p.prompt_queue.paused = true;
        p.attach_startup_session(attached_session("session-ready"), cx);
        assert!(p.prompt_queue.waiting_for_connection);
        p.load_history(vec![], vec![], cx);
        assert!(!p.prompt_queue.waiting_for_connection);
        p.status = "connected".into();
        p.submit_or_queue("send normally".into(), vec![], false, cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { session_id, content, .. })
        if session_id == "session-ready" && content == "send normally")
    );
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn queue_stays_above_input_in_startup_and_resumed_layouts(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "queue-layout".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    vcx.run_until_parked();
    panel.update(vcx, |p, cx| {
        assert!(p.startup_layout.is_some());
        p.history_loaded = true;
        p.status = "running".into();
        p.items.push(Item::User("First request".into()));
        p.submit_or_queue("Queued follow-up".into(), vec![], true, cx);
    });
    for startup in [true, false] {
        if !startup {
            panel.update(vcx, |p, cx| {
                p.startup_layout = None;
                cx.notify();
            });
        }
        vcx.run_until_parked();
        let queue = vcx.debug_bounds("prompt-queue").unwrap();
        let input = vcx.debug_bounds("prompt-input").unwrap();
        assert!(
            queue.bottom() <= input.top(),
            "queue below input in startup={startup}: {queue:?}, {input:?}"
        );
        assert!(queue.left() >= input.left() && queue.right() <= input.right());
        let footer = vcx.debug_bounds("panel-status").unwrap();
        if startup {
            assert!(
                input.bottom() <= footer.top(),
                "composer must fit above footer"
            );
        }
    }
}

#[gpui::test]
fn multiple_queued_prompts_stack_without_squeezing_input(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut p = Panel::new(
            "queue-stack".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        p.history_loaded = true;
        p.status = "running".into();
        p.items.push(Item::User("First request".into()));
        for index in 0..12 {
            p.submit_or_queue(
                format!("Follow-up {index}: {}", "long prompt ".repeat(20)),
                vec![],
                true,
                cx,
            );
        }
        p
    });
    vcx.run_until_parked();
    let queue = vcx.debug_bounds("queue-card").unwrap();
    let items = vcx.debug_bounds("prompt-queue-items").unwrap();
    let input = vcx.debug_bounds("prompt-input").unwrap();
    assert!(items.size.height <= px(170.));
    assert!(queue.bottom() <= input.top());
    let first = vcx.debug_bounds("queued-prompt-0").unwrap();
    let second = vcx.debug_bounds("queued-prompt-1").unwrap();
    assert!(first.bottom() < second.top(), "rows need breathing room");
    assert!(
        first.size.height >= px(24.),
        "rows must not shrink to fit the stack"
    );
    let recall = vcx.debug_bounds("recall-queued-prompt-0").unwrap();
    let delete = vcx.debug_bounds("remove-queued-prompt-0").unwrap();
    assert!(
        recall.right() <= delete.left() && delete.right() <= items.right(),
        "long text must not push actions out"
    );
    vcx.simulate_click(recall.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.prompts.len(), 12, "Edit keeps the message queued");
        assert!(
            p.prompt_queue.prompts[0]
                .content
                .starts_with("Follow-up 0:")
        );
        assert!(
            p.input.read(cx).content.is_empty(),
            "the message is edited in its row, not in the composer"
        );
    });
    // The open editor may grow the card. Scrolling must not move the composer.
    let input = vcx.debug_bounds("prompt-input").unwrap();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: items.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-1000.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    vcx.run_until_parked();
    let last = vcx.debug_bounds("queued-prompt-10").unwrap();
    assert!(
        last.top() >= items.top() && last.bottom() <= items.bottom() + px(1.),
        "last row must be reachable: {last:?}, {items:?}"
    );
    assert_eq!(
        vcx.debug_bounds("prompt-input"),
        Some(input),
        "queue scrolling must not move the input"
    );
}

#[gpui::test]
fn send_now_arrow_sends_one_queued_prompt_immediately(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut p = Panel::new("send-now".into(), None, None, bridge, cx);
        p.history_loaded = true;
        p.status = "running".into();
        p.items.push(Item::User("First".into()));
        p.pending_users.push_back(0);
        p.submit_or_queue("first queued".into(), vec![], true, cx);
        p.submit_or_queue("second queued".into(), vec![], true, cx);
        p
    });
    assert!(commands.try_recv().is_err(), "queued while busy");
    panel.update(cx, |p, cx| p.send_queued_prompt_now(1, cx));
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "second queued")
    );
    panel.read_with(cx, |p, _| {
        assert_eq!(p.prompt_queue.prompts.len(), 1);
        assert_eq!(p.prompt_queue.prompts[0].content, "first queued");
    });
}

pub(super) fn steered_panel<'a>(
    cx: &'a mut gpui::TestAppContext,
    id: &str,
) -> (
    gpui::Entity<Panel>,
    &'a mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Command>,
) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let id = id.to_string();
    let (panel, vcx) = cx.add_window_view(move |_, cx| {
        let mut p = Panel::new(id, None, None, bridge, cx);
        p.history_loaded = true;
        p.status = "running".into();
        p.items.push(Item::User("First request".into()));
        p.pending_users.push_back(0);
        p.apply(&event(json!({"ev":"message_accepted"})), cx);
        p.apply(
            &event(json!({"ev":"text_delta", "text":"Working on it"})),
            cx,
        );
        p
    });
    (panel, vcx, commands)
}

#[gpui::test]
fn message_sent_while_busy_shows_queued_row_until_delivered(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = steered_panel(cx, "steer-row");
    panel.update(vcx, |p, cx| {
        p.submit_or_queue("please look at this now".into(), vec![], false, cx)
    });
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));

    // Real pending-interrupt state: sent to the runtime, not yet delivered.
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 1);
        assert_eq!(
            p.items
                .iter()
                .filter(|item| matches!(item, Item::User(_)))
                .count(),
            1,
            "an undelivered message is not a transcript message yet"
        );
    });
    let row = vcx
        .debug_bounds("steered-prompt-0")
        .expect("queued row must render immediately");
    let input = vcx.debug_bounds("prompt-input").unwrap();
    assert!(row.bottom() <= input.top(), "row sits above the composer");
    assert!(
        vcx.debug_bounds("steered-count").is_none(),
        "one message needs no count"
    );

    panel.update(vcx, |p, cx| {
        p.send_prompt("and this too".into(), vec![], cx);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("steered-prompt-1").is_some(),
        "both messages have their own rows"
    );

    // Delivered: both become one transcript pass, after the streamed text.
    panel.update(vcx, |p, cx| {
        p.apply(
            &event(json!({"ev":"soft_interrupt_injected", "client_ids":[], "point":"D"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompt-0").is_none());
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 0);
        let texts: Vec<&str> = p
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                Item::Assistant(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            [
                "First request",
                "Working on it",
                "please look at this now",
                "and this too"
            ]
        );
    });
}

#[gpui::test]
fn two_steered_messages_are_queued_or_in_the_transcript_never_both(cx: &mut gpui::TestAppContext) {
    fn user_texts(panel: &Panel) -> Vec<&str> {
        panel
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    let (panel, vcx, commands) = steered_panel(cx, "steer-both");
    panel.update(vcx, |p, cx| {
        p.send_prompt("first steer".into(), vec![], cx);
        p.send_prompt("second steer".into(), vec![], cx);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
    assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));

    // Before the runtime injects, the text lives only in the queued rows.
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 2);
        assert_eq!(
            user_texts(p),
            ["First request"],
            "queued text is not a message yet"
        );
    });
    assert!(vcx.debug_bounds("steered-prompt-0").is_some());
    assert!(vcx.debug_bounds("steered-prompt-1").is_some());

    // Delivered: the rows go away and each message enters the transcript once.
    panel.update(vcx, |p, cx| {
        p.apply(
            &event(json!({"ev":"soft_interrupt_injected", "client_ids":[], "point":"D"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompt-0").is_none());
    assert!(vcx.debug_bounds("steered-count").is_none());
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 0);
        assert_eq!(
            user_texts(p),
            ["First request", "first steer", "second steer"]
        );
    });

    // Nothing that follows can add a second copy: a repeated injection finds an
    // empty queue, and the turn ending has nothing left to release.
    panel.update(vcx, |p, cx| {
        p.apply(
            &event(json!({"ev":"soft_interrupt_injected", "client_ids":[], "point":"B"})),
            cx,
        );
        p.apply(&event(json!({"ev":"turn_done"})), cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompt-0").is_none());
    panel.read_with(vcx, |p, _| {
        assert_eq!(
            user_texts(p),
            ["First request", "first steer", "second steer"],
            "a delivered message must never reappear as queued or duplicated"
        );
    });
}

#[gpui::test]
fn acceptance_ack_does_not_drop_a_steered_row(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, _commands) = steered_panel(cx, "steer-ack");
    panel.update(vcx, |p, cx| {
        p.submit_or_queue("look at the logs".into(), vec![], false, cx)
    });
    vcx.run_until_parked();
    // The ack proves the daemon took a `message` command. This text left as a
    // steering interrupt, so the ack says nothing about it: dropping the row
    // here would show the message as delivered before the model saw it.
    panel.update(vcx, |p, cx| {
        p.apply(&event(json!({"ev":"message_accepted"})), cx)
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |p, _| assert_eq!(p.steered_prompt_count(), 1));
    assert!(vcx.debug_bounds("steered-prompt-0").is_some());
}

#[gpui::test]
fn steered_row_does_not_appear_when_idle_and_survives_no_turn_end(cx: &mut gpui::TestAppContext) {
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(move |_, cx| {
        let mut p = Panel::new("steer-idle".into(), None, None, bridge, cx);
        p.history_loaded = true;
        p.status = "idle".into();
        p
    });
    panel.update(vcx, |p, cx| {
        p.submit_or_queue("hello".into(), vec![], false, cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompt-0").is_none());
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 0);
        assert!(
            p.items
                .iter()
                .any(|item| matches!(item, Item::User(t) if t == "hello"))
        );
    });
}

#[gpui::test]
fn undelivered_steered_message_is_kept_when_the_turn_ends(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = steered_panel(cx, "steer-end");
    panel.update(vcx, |p, cx| {
        p.submit_or_queue("late note".into(), vec![], false, cx)
    });
    panel.update(vcx, |p, cx| {
        p.apply(&event(json!({"ev":"turn_done"})), cx);
    });
    panel.read_with(vcx, |p, _| {
        assert_eq!(p.steered_prompt_count(), 1);
        assert!(
            !p.items
                .iter()
                .any(|item| matches!(item, Item::User(t) if t == "late note")),
            "an ended turn is not a delivery confirmation"
        );
    });
    let _ = commands.try_recv();
    panel.update(vcx, |p, cx| {
        p.apply(&event(json!({"ev":"session_status", "status":"idle"})), cx);
    });
    vcx.run_until_parked();
    let button = vcx
        .debug_bounds("resend-steered-0")
        .expect("unconfirmed steer can be resent explicitly");
    vcx.simulate_click(button.center(), Default::default());
    vcx.run_until_parked();
    let sent: Vec<_> = commands.try_iter().collect();
    assert!(
        sent.iter().any(
            |command| matches!(command, Command::Send { content, .. } if content == "late note")
        ),
        "received {} commands",
        sent.len()
    );
}

pub(super) fn editing_test_prompt(
    id: &str,
    content: &str,
    mode: jcode_sdk::api::QueuedPromptMode,
) -> jcode_sdk::api::QueuedPrompt {
    jcode_sdk::api::QueuedPrompt {
        client_id: id.into(),
        active_skill: None,
        content: content.into(),
        images: vec![],
        attachments: vec![],
        mode,
        editing: false,
    }
}

pub(super) fn drain_commands(commands: &std::sync::mpsc::Receiver<Command>) -> Vec<Command> {
    let mut drained = Vec::new();
    while let Ok(command) = commands.try_recv() {
        drained.push(command);
    }
    drained
}

/// A panel with daemon queue editing, its composer wired and focused.
pub(super) fn queue_edit_panel(
    cx: &mut gpui::TestAppContext,
    prompts: Vec<jcode_sdk::api::QueuedPrompt>,
) -> (
    Entity<Panel>,
    &mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Command>,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-edit".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.prompt_queue.remote_supported = true;
        panel.prompt_queue.remote_mutations_supported = true;
        panel.prompt_queue.remote_editing_supported = true;
        panel.prompt_queue.remote = prompts;
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(900.), px(700.)));
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).editor_focus();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();
    (panel, vcx, commands)
}

pub(super) fn edit_input(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> Entity<PromptInput> {
    panel.read_with(vcx, |panel, _| {
        panel.prompt_queue.queued_edit.as_ref().expect("an open queue editor").input.clone()
    })
}

#[gpui::test]
fn daemon_row_edit_opens_in_place_and_leaves_the_composer_alone(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![
            editing_test_prompt("p1", "first queued", QueuedPromptMode::Steer),
            editing_test_prompt("p2", "second queued", QueuedPromptMode::AfterYield),
        ],
    );
    panel.update(vcx, |p, cx| p.input.update(cx, |input, cx| input.set_content("my draft".into(), cx)));
    let edit = vcx.debug_bounds("remote-queue-edit-1").expect("Edit on the second row").center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    let row = vcx.debug_bounds("remote-queue-row-1").expect("the edited row");
    let editor = vcx.debug_bounds("queue-edit-input").expect("the editor is in the row");
    assert!(row.left() <= editor.left() && row.right() >= editor.right(), "{row:?} {editor:?}");
    assert!(row.top() <= editor.top() && row.bottom() >= editor.bottom(), "{row:?} {editor:?}");
    let composer = vcx.debug_bounds("composer").unwrap();
    assert!(editor.bottom() <= composer.top(), "the editor is in the queue, above the composer");
    let input = edit_input(&panel, vcx);
    assert_eq!(input.read_with(vcx, |input, _| input.content.to_string()), "second queued");
    assert!(vcx.update(|window, cx| input.read(cx).is_focused(window)));
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.input.read(cx).content, "my draft", "the composer draft is untouched");
        assert_eq!(p.prompt_queue.remote.len(), 2);
    });
    let sent = drain_commands(&commands);
    assert!(sent.iter().any(|command| matches!(
        command,
        Command::SetQueuedPromptEditing { prompt_id, editing: true, .. } if prompt_id == "p2"
    )), "the daemon holds the message while it is edited");
    assert!(!sent.iter().any(|command| matches!(command, Command::RemoveQueuedPrompt { .. })));
    vcx.simulate_input(" now");
    vcx.simulate_keystrokes("enter");
    let saved = drain_commands(&commands);
    assert!(saved.iter().any(|command| matches!(
        command,
        Command::EditQueuedPrompt { prompt_id, content, .. }
            if prompt_id == "p2" && content == "second queued now"
    )), "Enter saves in place");
    assert!(!saved.iter().any(|command| matches!(command, Command::QueuePrompt { .. } | Command::Send { .. })));
    panel.update(vcx, |p, cx| {
        p.queued_prompt_edited("p2", Ok(jcode_sdk::api::QueuedPromptEditOutcome::Edited), cx)
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert!(p.prompt_queue.queued_edit.is_none(), "a confirmed save closes the editor");
        assert_eq!(p.input.read(cx).content, "my draft");
    });
    assert!(vcx.debug_bounds("queue-edit-input").is_none());
}

#[gpui::test]
fn after_yield_edit_types_space_without_sending(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode::AfterYield;
    cx.update(crate::bind_workspace_keys);
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", "Проверь", AfterYield)],
    );
    let click_edit = |vcx: &mut gpui::VisualTestContext| {
        let edit = vcx.debug_bounds("remote-queue-edit-0").expect("Edit");
        vcx.simulate_click(edit.center(), Default::default());
        vcx.run_until_parked();
    };

    click_edit(vcx);
    vcx.simulate_keystrokes("space");
    vcx.simulate_input("очередь");
    let input = edit_input(&panel, vcx);
    assert_eq!(
        input.read_with(vcx, |input, _| input.content.to_string()),
        "Проверь очередь",
    );
    assert!(matches!(commands.try_recv(), Ok(Command::SetQueuedPromptEditing {
        prompt_id, editing: true, ..
    }) if prompt_id == "p1"));
    assert!(commands.try_recv().is_err(), "typing must not send or save");

    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::SetQueuedPromptEditing {
        prompt_id, editing: false, ..
    }) if prompt_id == "p1"));
    vcx.simulate_input("Несохранённый черновик");

    click_edit(vcx);
    vcx.simulate_keystrokes("space");
    vcx.simulate_input("текст");
    vcx.simulate_keystrokes("shift-space");
    vcx.simulate_input("ещё");
    assert!(matches!(commands.try_recv(), Ok(Command::SetQueuedPromptEditing {
        prompt_id, editing: true, ..
    }) if prompt_id == "p1"));
    assert!(commands.try_recv().is_err(), "typing must not send or save");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::EditQueuedPrompt {
        prompt_id, content, ..
    }) if prompt_id == "p1" && content == "Проверь текст ещё"));
    assert!(commands.try_recv().is_err(), "Enter must only save in place");
    panel.read_with(vcx, |panel, cx| {
        assert_eq!(panel.input.read(cx).content, "Несохранённый черновик");
        assert_eq!(panel.prompt_queue.remote[0].mode, AfterYield);
    });
}

#[gpui::test]
fn up_in_an_empty_composer_edits_the_newest_row_in_place(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![
            editing_test_prompt("p1", "first queued", QueuedPromptMode::Steer),
            editing_test_prompt("p2", "second queued", QueuedPromptMode::AfterYield),
        ],
    );
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    let input = edit_input(&panel, vcx);
    assert_eq!(input.read_with(vcx, |input, _| input.content.to_string()), "second queued");
    assert!(vcx.debug_bounds("remote-queue-row-1").is_some());
    panel.read_with(vcx, |p, cx| assert!(p.input.read(cx).content.is_empty()));
    assert!(drain_commands(&commands).iter().any(|command| matches!(
        command,
        Command::SetQueuedPromptEditing { prompt_id, editing: true, .. } if prompt_id == "p2"
    )));
}

#[gpui::test]
fn local_row_edit_keeps_queue_order_and_never_merges_into_the_draft(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-local-edit".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.startup_layout = None;
        panel.status = "running".into();
        panel.items.push(Item::User("initial".into()));
        panel.submit_or_queue("first queued message".into(), vec![], true, cx);
        panel.submit_or_queue("second queued message".into(), vec![], true, cx);
        panel.submit_or_queue("third queued message".into(), vec![], true, cx);
        panel
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(900.), px(700.)));
    vcx.update(|_, cx| Panel::connect_input(&panel, cx));
    vcx.run_until_parked();
    panel.update(vcx, |p, cx| p.input.update(cx, |input, cx| input.set_content("my draft".into(), cx)));
    let edit = vcx.debug_bounds("recall-queued-prompt-1").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    let row = vcx.debug_bounds("queued-prompt-1").expect("the edited row");
    let editor = vcx.debug_bounds("queue-edit-input").expect("editor in the row");
    assert!(row.top() <= editor.top() && row.bottom() >= editor.bottom());
    vcx.simulate_input(" edited");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        let contents: Vec<_> = p.prompt_queue.prompts.iter().map(|prompt| prompt.content.as_str()).collect();
        assert_eq!(contents, ["first queued message", "second queued message edited", "third queued message"]);
        assert_eq!(p.input.read(cx).content, "my draft", "nothing merged into the draft");
        assert!(p.prompt_queue.queued_edit.is_none());
    });
    assert!(
        !drain_commands(&commands).iter().any(|command| matches!(command, Command::Send { .. } | Command::QueuePrompt { .. })),
        "saving a local edit sends nothing by itself"
    );
}

#[gpui::test]
fn queued_images_and_files_survive_a_save_byte_for_byte(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::{Attachment, QueuedPromptMode};
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let mut prompt = editing_test_prompt("p1", "look at [#image 1] then the logs", QueuedPromptMode::AfterYield);
    prompt.images = vec![
        ("image/png".into(), PNG.into()),
        ("image/png".into(), format!("{PNG}AA")),
    ];
    prompt.attachments = vec![
        Attachment::File {
            name: "notes.txt".into(),
            mime: "text/plain".into(),
            size: 3,
            data: Some("YWJj".into()),
            path: None,
        },
        Attachment::File {
            name: "big.log".into(),
            mime: "text/plain".into(),
            size: 7_000_000,
            data: None,
            path: Some("/var/log/big.log".into()),
        },
        Attachment::PastedText { text: "pasted\nblock".into() },
    ];
    let original = prompt.clone();
    let (panel, vcx, commands) = queue_edit_panel(cx, vec![prompt]);
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-image-thumbnail").is_some(), "queued screenshots show as thumbnails");
    let input = edit_input(&panel, vcx);
    let shown = input.read_with(vcx, |input, _| input.content.to_string());
    assert!(shown.contains("[#notes.txt, 3 B]") && shown.contains("[#big.log, 7000000 B]"), "{shown}");
    // Untouched: the save carries exactly what was queued.
    let (content, images, attachments) = input.read_with(vcx, |input, _| input.queued_edit_payload());
    assert_eq!(content, original.content);
    assert_eq!(images, original.images);
    assert_eq!(attachments, original.attachments);
    // Edit only the text: every image and attachment goes back on the wire.
    vcx.simulate_keystrokes("end");
    vcx.simulate_input("!");
    vcx.simulate_keystrokes("enter");
    let saved = drain_commands(&commands).into_iter().find_map(|command| match command {
        Command::EditQueuedPrompt { content, images, attachments, .. } => Some((content, images, attachments)),
        _ => None,
    }).expect("Enter saves");
    assert_eq!(saved.0, format!("{}!", original.content));
    assert_eq!(saved.1, original.images, "images unchanged on the wire");
    assert_eq!(saved.2, original.attachments, "path-based and inline files unchanged on the wire");
}

#[gpui::test]
fn removing_an_attachment_chip_drops_only_that_attachment(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::{Attachment, QueuedPromptMode};
    let mut prompt = editing_test_prompt("p1", "read these", QueuedPromptMode::Steer);
    prompt.attachments = vec![
        Attachment::File { name: "a.txt".into(), mime: "text/plain".into(), size: 1, data: Some("YQ==".into()), path: None },
        Attachment::File { name: "b.txt".into(), mime: "text/plain".into(), size: 1, data: None, path: Some("/tmp/b.txt".into()) },
    ];
    let (panel, vcx, commands) = queue_edit_panel(cx, vec![prompt]);
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    // The caret sits after the last chip; Backspace removes that chip whole.
    vcx.simulate_keystrokes("end backspace enter");
    let _ = panel;
    let attachments = drain_commands(&commands).into_iter().find_map(|command| match command {
        Command::EditQueuedPrompt { content, attachments, .. } => {
            assert_eq!(content.trim_end(), "read these");
            Some(attachments)
        }
        _ => None,
    }).expect("saved");
    assert_eq!(attachments.len(), 1);
    assert!(matches!(&attachments[0], Attachment::File { name, .. } if name == "a.txt"));
}

#[gpui::test]
fn escape_cancels_without_changes_and_releases_the_hold(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", "original queued text", QueuedPromptMode::Steer)],
    );
    panel.update(vcx, |p, cx| p.input.update(cx, |input, cx| input.set_content("my draft".into(), cx)));
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    vcx.simulate_input(" changed");
    drain_commands(&commands);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    let released = drain_commands(&commands);
    assert!(released.iter().any(|command| matches!(
        command,
        Command::SetQueuedPromptEditing { prompt_id, editing: false, .. } if prompt_id == "p1"
    )), "Esc releases the daemon hold");
    assert!(!released.iter().any(|command| matches!(command, Command::EditQueuedPrompt { .. })), "Esc saves nothing");
    panel.read_with(vcx, |p, cx| {
        assert!(p.prompt_queue.queued_edit.is_none());
        assert_eq!(p.prompt_queue.remote[0].content, "original queued text");
        assert_eq!(p.input.read(cx).content, "my draft");
    });
    assert!(vcx.debug_bounds("queue-edit-input").is_none());
    assert!(vcx.debug_bounds("remote-queue-row-0").is_some());
}

#[gpui::test]
fn cancel_pill_and_panel_close_release_the_hold(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", "queued", QueuedPromptMode::Steer)],
    );
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    let cancel = vcx.debug_bounds("queue-edit-cancel").expect("Cancel pill").center();
    vcx.simulate_click(cancel, Default::default());
    vcx.run_until_parked();
    assert!(drain_commands(&commands).iter().any(|command| matches!(command, Command::SetQueuedPromptEditing { editing: false, .. })));
    panel.read_with(vcx, |p, _| assert!(p.prompt_queue.queued_edit.is_none()));
    let edit = vcx.debug_bounds("remote-queue-edit-0").unwrap().center();
    vcx.simulate_click(edit, Default::default());
    vcx.run_until_parked();
    drain_commands(&commands);
    panel.update(vcx, |p, _| p.release_queue_edit_hold());
    assert!(drain_commands(&commands).iter().any(|command| matches!(command, Command::SetQueuedPromptEditing { editing: false, .. })));
}

#[gpui::test]
fn long_multiline_message_is_fully_visible_in_the_editor(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let long = (1..=6).map(|n| format!("line {n} of a queued message")).collect::<Vec<_>>().join("\n");
    let (panel, vcx, _commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", &long, QueuedPromptMode::Steer)],
    );
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    vcx.run_until_parked();
    let input = edit_input(&panel, vcx);
    let content = input.read_with(vcx, |input, _| input.content.to_string());
    assert_eq!(content, long, "the editor holds the whole message");
    let (lines, overflow) = input.read_with(vcx, |input, _| input.editor_layout());
    assert!(lines >= 6, "every line is laid out, got {lines}");
    assert_eq!(overflow, px(0.), "six lines fit without scrolling the editor");
    let field = vcx.debug_bounds("queue-edit-input").unwrap();
    let card = vcx.debug_bounds("prompt-queue-items").unwrap();
    assert!(card.top() <= field.top() && card.bottom() >= field.bottom(), "the whole editor is in view: {card:?} {field:?}");
}

#[gpui::test]
fn rejected_save_keeps_the_edited_text_with_a_note(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::{QueuedPromptEditOutcome, QueuedPromptMode};
    let (panel, vcx, commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", "queued", QueuedPromptMode::Steer)],
    );
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    vcx.simulate_input(" with my edits");
    vcx.simulate_keystrokes("enter");
    assert!(drain_commands(&commands).iter().any(|command| matches!(command, Command::EditQueuedPrompt { .. })));
    // The daemon delivered the message before the save reached it.
    panel.update(vcx, |p, cx| {
        p.prompt_queue.remote.clear();
        p.queued_prompt_edited("p1", Ok(QueuedPromptEditOutcome::AlreadyDelivered), cx);
    });
    vcx.run_until_parked();
    let input = edit_input(&panel, vcx);
    assert_eq!(input.read_with(vcx, |input, _| input.content.to_string()), "queued with my edits");
    assert!(vcx.debug_bounds("queue-edit-input").is_some(), "the editor stays visible");
    assert!(vcx.debug_bounds("queue-edit-note").is_some(), "with a neutral note");
    panel.read_with(vcx, |p, _| {
        assert!(!p.items.iter().any(|item| matches!(item, Item::Error(_))), "no red error for a race");
    });
    // Esc dismisses it; nothing is sent again by itself.
    vcx.update(|window, cx| window.focus(&input.read(cx).editor_focus(), cx));
    vcx.simulate_keystrokes("escape");
    assert!(!drain_commands(&commands).iter().any(|command| matches!(command, Command::QueuePrompt { .. })));
}

#[gpui::test]
fn a_message_delivered_mid_edit_keeps_the_edited_text(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode;
    let (panel, vcx, _commands) = queue_edit_panel(
        cx,
        vec![editing_test_prompt("p1", "queued", QueuedPromptMode::Steer)],
    );
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    vcx.simulate_input(" typed");
    panel.update(vcx, |p, cx| {
        p.apply(&event(json!({"ev":"queued_prompts", "prompts": []})), cx);
    });
    vcx.run_until_parked();
    let input = edit_input(&panel, vcx);
    assert_eq!(input.read_with(vcx, |input, _| input.content.to_string()), "queued typed");
    assert!(vcx.debug_bounds("queue-edit-note").is_some());
}
