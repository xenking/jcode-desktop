use super::*;

include!("queue_card_tests.rs");

#[gpui::test]
fn effort_command_during_running_turn_is_immediate_not_a_steer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("active-session".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel.provider = Some("openai".into());
        panel.model = Some("gpt-6.1-sol".into());
        panel
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("/effort high");
    vcx.simulate_keystrokes("enter");
    vcx.simulate_keystrokes("enter");
    assert!(matches!(commands.try_recv(), Ok(Command::SessionOperation {
        operation: SessionOperation::SetEffort(level), ..
    }) if level == "high"));
    assert!(commands.try_recv().is_err());
}
use serde_json::json;

#[gpui::test]
fn queued_skill_is_preserved_per_prompt_in_daemon_queue(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queued-skills".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.prompt_queue.remote_supported = true;
        panel.status = "running".into();
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.active_skill = Some("poteto-mode".into());
        panel.submit_or_queue("first".into(), vec![], true, cx);
        panel.active_skill = Some("selfdev".into());
        panel.submit_or_queue("second".into(), vec![], true, cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. })
        if prompt.content == "first" && prompt.active_skill.as_deref() == Some("poteto-mode"))
    );
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. })
        if prompt.content == "second" && prompt.active_skill.as_deref() == Some("selfdev"))
    );
}

#[gpui::test]
fn restored_unconfirmed_remote_prompt_retries_with_same_id(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let source = cx.new(|cx| {
        let mut panel = Panel::new("queue-restore".into(), None, None, bridge, cx);
        panel
            .prompt_queue
            .pending_remote
            .push(jcode_sdk::api::QueuedPrompt {
                client_id: "restore-id".into(),
                active_skill: Some("poteto-mode".into()),
                content: "keep me".into(),
                images: vec![],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
                editing: false,
            });
        panel
    });
    let bytes = source.read_with(cx, |panel, cx| {
        serde_json::to_vec(&panel.snapshot(cx)).unwrap()
    });
    let snapshot: PanelSnapshot = serde_json::from_slice(&bytes).unwrap();
    source.update(cx, |panel, cx| {
        panel.restore_snapshot(snapshot, cx);
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[]})), cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.client_id == "restore-id" && prompt.content == "keep me" && prompt.active_skill.as_deref() == Some("poteto-mode"))
    );
}

#[gpui::test]
fn pending_remote_steer_does_not_offer_retry_before_failure(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-pending".into(), None, None, bridge, cx);
        panel.prompt_queue.remote_supported = true;
        panel
            .prompt_queue
            .pending_remote
            .push(jcode_sdk::api::QueuedPrompt {
                client_id: "pending-id".into(),
                active_skill: None,
                content: "pending".into(),
                images: vec![],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
                editing: false,
            });
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("queue-section-steer").is_some());
    assert!(
        vcx.debug_bounds("queue-send-control").is_none(),
        "in-flight send is not a failure"
    );
    panel.update(vcx, |panel, cx| {
        panel.queue_prompt_failed("pending-id", "connection lost".into(), cx)
    });
    vcx.run_until_parked();
    let retry = vcx
        .debug_bounds("queue-send-control")
        .expect("real failure offers Retry");
    panel.read_with(vcx, |panel, _| {
        assert_eq!(
            panel.prompt_queue.remote_failures["pending-id"],
            "connection lost"
        )
    });
    vcx.simulate_click(retry.center(), Default::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.client_id == "pending-id")
    );
    assert!(vcx.debug_bounds("queue-send-control").is_none());
}

#[gpui::test]
fn unconfirmed_steer_survives_serialized_panel_snapshot(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| {
        let mut panel = Panel::new(
            "queue-persist".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    let snapshot = panel.update(cx, |panel, cx| {
        panel.submit_or_queue("preserve this steer".into(), vec![], false, cx);
        serde_json::to_vec(&panel.snapshot(cx)).unwrap()
    });
    let restored: PanelSnapshot = serde_json::from_slice(&snapshot).unwrap();
    let other = cx.new(|cx| {
        Panel::new(
            "queue-persist".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    other.update(cx, |panel, cx| {
        panel.restore_snapshot(restored, cx);
        assert_eq!(
            panel.steered_prompt_count(),
            1,
            "unconfirmed steering must survive quit and relaunch"
        );
    });
}

#[gpui::test]
fn peer_prompts_and_skill_reminder_survive_live_and_history(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| {
        let mut panel = Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.history_loaded = true;
        panel
    });
    panel.update(cx, |panel, cx| {
        for content in ["plain external prompt", "давай делать"] {
            panel.apply(
                &event(json!({"ev":"peer_user_message", "content":content})),
                cx,
            );
        }
        let users: Vec<_> = panel
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, ["plain external prompt", "давай делать"]);
    });
    let restored = cx.new(|cx| {
        Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    restored.update(cx, |panel, cx| {
        let messages = serde_json::from_value(json!([
            {"role":"user", "content":"plain external prompt"},
            {"role":"assistant", "content":"answer"},
            {"role":"system", "content":"skill instructions"},
            {"role":"user", "content":"давай делать"}
        ]))
        .unwrap();
        panel.load_history(messages, vec![], cx);
        let users: Vec<_> = panel
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(users, ["plain external prompt", "давай делать"]);
        assert!(
            !panel.items.iter().any(
                |item| matches!(item, Item::User(text) if text.contains("skill instructions"))
            )
        );
    });
}

#[gpui::test]
fn external_steer_is_visible_live_and_after_history_reload(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| {
        let mut panel = Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.history_loaded = true;
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"text_delta", "text":"before"})), cx);
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"steer from another client", "point":"D", "client_ids":[]})), cx);
        panel.apply(&event(json!({"ev":"text_delta", "text":"after"})), cx);
        panel.flush_streaming();
        assert!(panel.items.iter().any(|i| matches!(i, Item::User(text) if text == "steer from another client")));
        let index = panel.items.iter().position(|i| matches!(i, Item::User(text) if text == "steer from another client")).unwrap();
        assert!(panel.items[..index].iter().any(|i| matches!(i, Item::Assistant(text) if text == "before")));
        assert!(panel.items[index + 1..].iter().any(|i| matches!(i, Item::Assistant(text) if text == "after")));
    });
    let restored = cx.new(|cx| {
        Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    restored.update(cx, |panel, cx| {
        let messages = serde_json::from_value(json!([
            {"role":"assistant","content":"before"}, {"role":"user","content":"steer from another client"},
            {"role":"assistant","content":"after"}
        ])).unwrap();
        panel.load_history(messages, vec![], cx);
        assert!(panel.items.iter().any(|i| matches!(i, Item::User(text) if text == "steer from another client")));
    });
}

#[gpui::test]
fn external_injection_is_idempotent_and_system_injections_are_not_prompts(
    cx: &mut gpui::TestAppContext,
) {
    let panel = cx.new(|cx| {
        Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(cx, |panel, cx| {
        let injected = event(json!({"ev":"soft_interrupt_injected", "content":"external steer", "point":"D", "client_ids":["foreign-1"]}));
        panel.apply(&injected, cx);
        panel.apply(&injected, cx);
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"internal", "point":"D", "display_role":"system", "client_ids":[]})), cx);
        assert_eq!(panel.items.iter().filter(|item| matches!(item, Item::User(text) if text == "external steer")).count(), 1);
        assert!(!panel.items.iter().any(|item| matches!(item, Item::User(text) if text == "internal")));
    });
}

#[gpui::test]
fn mixed_known_and_external_injection_keeps_all_text(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| {
        Panel::new(
            "queue-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(cx, |panel, cx| {
        panel.prompt_queue.pending_remote.push(jcode_sdk::api::QueuedPrompt {
            client_id: "known".into(), active_skill: None, content: "local".into(), images: vec![], attachments: vec![],
            mode: jcode_sdk::api::QueuedPromptMode::Steer, editing: false,
        });
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"local\n\nexternal", "point":"D", "client_ids":["known", "foreign"]})), cx);
        assert!(panel.items.iter().any(|item| matches!(item, Item::User(text) if text.contains("external"))));
    });
}

#[gpui::test]
fn active_empty_queue_has_no_default_mode_label(cx: &mut gpui::TestAppContext) {
    let (_panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "empty-queue".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.status = "running".into();
        panel.prompt_queue.remote_supported = true;
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("queue-default-mode").is_none());
    assert!(vcx.debug_bounds("queue-section-steer").is_none());
}

#[gpui::test]
fn queued_image_uses_chip_and_opens_preview(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-image".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.prompt_queue.waiting_for_connection = true;
        let image = super::image_preview::fixture_image();
        panel.prompt_queue.prompts.push(QueuedPrompt {
            content: "Look [#image 1] here".into(),
            active_skill: None,
            images: vec![(image.media_type, image.data)],
            files: vec![],
            after_yield: true,
            send_now: false,
        });
        panel
    });
    vcx.run_until_parked();
    let chip = vcx
        .debug_bounds("queue-image-chip")
        .expect("image is a chip, not marker text");
    assert!(vcx.debug_bounds("queue-image-thumbnail").is_some());
    assert!(vcx.debug_bounds("queue-image-label").is_some());
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
}

#[gpui::test]
fn remote_and_steered_queue_images_use_same_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "queue-image-paths".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.status = "running".into();
        panel.prompt_queue.remote_supported = true;
        let image = super::image_preview::fixture_image();
        panel
            .prompt_queue
            .remote
            .push(jcode_sdk::api::QueuedPrompt {
                client_id: "remote-image".into(),
                active_skill: None,
                content: "Look [#image 1]".into(),
                images: vec![(image.media_type, image.data)],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
                editing: false,
            });
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("queue-image-thumbnail").is_some());
    let chip = vcx.debug_bounds("queue-image-chip").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
    panel.update(vcx, |panel, cx| {
        let remote = panel.prompt_queue.remote.pop().unwrap();
        panel.image_preview = None;
        panel.prompt_queue.steered.push(QueuedPrompt {
            content: remote.content,
            active_skill: None,
            images: remote.images,
            files: vec![],
            after_yield: false,
            send_now: false,
        });
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompt-0").is_some());
    assert!(vcx.debug_bounds("queue-image-thumbnail").is_some());
    let chip = vcx.debug_bounds("queue-image-chip").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
}

#[test]
fn queue_edit_rejects_unrestorable_attachments() {
    let attachment = jcode_sdk::api::Attachment::PastedText { text: "must not disappear".into() };
    assert!(restorable_files(&[attachment]).is_none());
}

#[gpui::test]
fn queued_file_reaches_daemon_without_losing_bytes(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-file".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel.prompt_queue.remote_supported = true;
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.submit_or_queue_with_files(
            "Read notes".into(),
            vec![],
            vec![crate::input::SubmittedFile {
                name: "notes.txt".into(),
                mime: "text/plain".into(),
                size: 3,
                data: "YWJj".into(),
            }],
            true,
            cx,
        );
    });
    let prompt = match commands.try_recv().unwrap() {
        Command::QueuePrompt { prompt, .. } => prompt,
        _ => panic!("expected queued prompt"),
    };
    let prompt = serde_json::to_value(prompt).unwrap();
    assert_eq!(prompt["attachments"][0]["name"], "notes.txt");
    assert_eq!(prompt["attachments"][0]["data"], "YWJj");
}

#[gpui::test]
fn daemon_queue_modes_and_pickup_are_not_duplicated(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[]})), cx);
        panel.submit_or_queue("steer me".into(), vec![], false, cx);
        panel.submit_or_queue("afterwards".into(), vec![], true, cx);
    });
    let first = match commands.try_recv().unwrap() {
        Command::QueuePrompt { prompt, .. } => prompt,
        _ => panic!("expected queued prompt"),
    };
    let second = match commands.try_recv().unwrap() {
        Command::QueuePrompt { prompt, .. } => prompt,
        _ => panic!("expected queued prompt"),
    };
    assert_eq!(first.mode, jcode_sdk::api::QueuedPromptMode::Steer);
    assert_eq!(second.mode, jcode_sdk::api::QueuedPromptMode::AfterYield);
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[first, second]})), cx);
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"steer me", "point":"D", "client_ids":[first.client_id]})), cx);
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"steer me", "point":"D", "client_ids":[first.client_id]})), cx);
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.prompt_queue.remote.len(), 1);
        assert!(panel.prompt_queue.pending_remote.is_empty());
        assert_eq!(
            panel
                .items
                .iter()
                .filter(|item| matches!(item, Item::User(text) if text == "steer me"))
                .count(),
            1
        );
    });
}

#[gpui::test]
fn prompt_before_history_and_queue_handshake_does_not_use_legacy_send(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| Panel::new("queue-test".into(), None, None, bridge, cx));
    panel.update(cx, |panel, cx| {
        assert!(!panel.history_loaded);
        panel.submit_or_queue("early send".into(), vec![], false, cx);
    });
    assert!(
        commands.try_recv().is_err(),
        "must not issue legacy Send before queue support is known"
    );
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[]})), cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel.send_queued_prompts(cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.content == "early send" && prompt.mode == jcode_sdk::api::QueuedPromptMode::Steer)
    );
}

#[gpui::test]
fn pending_connection_prompts_remain_separate_daemon_turns(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "connected".into();
        panel.prompt_queue.remote_supported = true;
        panel.prompt_queue.waiting_for_connection = true;
        panel.prompt_queue.prompts = vec![
            QueuedPrompt {
                content: "one".into(),
                active_skill: None,
                images: vec![],
                files: vec![],
                after_yield: true,
                send_now: false,
            },
            QueuedPrompt {
                content: "two".into(),
                active_skill: None,
                images: vec![],
                files: vec![],
                after_yield: true,
                send_now: false,
            },
        ];
        panel
    });
    panel.update(cx, |panel, cx| panel.send_queued_prompts(cx));
    for expected in ["one", "two"] {
        assert!(
            matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.content == expected && prompt.mode == jcode_sdk::api::QueuedPromptMode::AfterYield)
        );
    }
}

#[gpui::test]
fn configurable_default_applies_to_active_turn_and_survives_restore(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel.set_default_send_after_yield(true, cx);
        let saved = serde_json::to_string(&panel.prompt_queue).unwrap();
        panel.prompt_queue = serde_json::from_str(&saved).unwrap();
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[]})), cx);
        panel.submit_or_queue("configured default".into(), vec![], false, cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.mode == jcode_sdk::api::QueuedPromptMode::AfterYield)
    );
}

#[gpui::test]
fn send_after_turn_done_still_uses_atomic_daemon_queue(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[]})), cx);
        panel.apply(&event(json!({"ev":"turn_done"})), cx);
        panel.apply(&event(json!({"ev":"session_status", "status":"idle"})), cx);
        panel.submit_or_queue("race prompt".into(), vec![], false, cx);
    });
    let prompt = match commands.try_recv() {
        Ok(Command::QueuePrompt { prompt, .. }) => prompt,
        _ => panic!("expected atomic queue submission"),
    };
    assert_eq!(prompt.content, "race prompt");
    assert_eq!(prompt.mode, jcode_sdk::api::QueuedPromptMode::Steer);
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"race prompt", "point":"turn_start", "client_ids":[prompt.client_id]})), cx);
        assert!(panel.activity_active(), "queued turn must show as running before model output");
    });
}

#[gpui::test]
fn retry_after_reload_does_not_duplicate_delivered_history(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.items.push(Item::User("already delivered".into()));
        panel
            .prompt_queue
            .pending_remote
            .push(jcode_sdk::api::QueuedPrompt {
                client_id: "lost-ack".into(),
                active_skill: None,
                content: "already delivered".into(),
                images: vec![],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::AfterYield,
                editing: false,
            });
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"already delivered", "point":"already_delivered", "client_ids":["lost-ack"]})), cx);
        assert!(panel.prompt_queue.pending_remote.is_empty());
        assert_eq!(panel.items.iter().filter(|item| matches!(item, Item::User(text) if text == "already delivered")).count(), 1);
    });
}

#[gpui::test]
fn retry_keeps_a_new_prompt_identical_to_older_history(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.items.push(Item::User("repeat".into()));
        panel
            .prompt_queue
            .pending_remote
            .push(jcode_sdk::api::QueuedPrompt {
                client_id: "new-repeat".into(),
                active_skill: None,
                content: "repeat".into(),
                images: vec![],
                attachments: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::AfterYield,
                editing: false,
            });
        panel
            .prompt_queue
            .pending_remote_history_counts
            .insert("new-repeat".into(), 1);
        panel
    });
    panel.update(cx, |panel, cx| {
        panel.apply(&event(json!({"ev":"soft_interrupt_injected", "content":"repeat", "point":"already_delivered", "client_ids":["new-repeat"]})), cx);
        assert_eq!(panel.items.iter().filter(|item| matches!(item, Item::User(text) if text == "repeat")).count(), 2);
    });
}

fn event(mut value: serde_json::Value) -> ApiEvent {
    if value.get("session_id").is_none() {
        value["session_id"] = json!("queue-test");
    }
    serde_json::from_value(value).unwrap()
}

#[gpui::test]
fn queue_sections_group_modes_and_keep_row_actions_in_order(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode::{AfterYield, Steer};
    let (_, vcx, _) = queue_edit_panel(
        cx,
        vec![
            editing_test_prompt("a", "first after yield", AfterYield),
            editing_test_prompt("b", "first steer", Steer),
            editing_test_prompt("c", "second after yield", AfterYield),
            editing_test_prompt("d", "second steer", Steer),
        ],
    );
    let mut bounds = |selector: &'static str| vcx.debug_bounds(selector).unwrap();
    let steer = bounds("queue-section-steer");
    let after_yield = bounds("queue-section-after-yield");
    assert!(steer.bottom() <= after_yield.top());
    for (section, [first, second, send, edit, delete]) in [
        (
            steer,
            [
                "remote-queue-row-1",
                "remote-queue-row-3",
                "queue-row-send-now-1",
                "remote-queue-edit-1",
                "remote-queue-delete-1",
            ],
        ),
        (
            after_yield,
            [
                "remote-queue-row-0",
                "remote-queue-row-2",
                "queue-row-send-now-0",
                "remote-queue-edit-0",
                "remote-queue-delete-0",
            ],
        ),
    ] {
        let first_row = bounds(first);
        let second_row = bounds(second);
        assert!(section.top() < first_row.top());
        assert!(first_row.bottom() <= second_row.top());
        assert!(second_row.bottom() <= section.bottom());
        let (send, edit, delete) = (bounds(send), bounds(edit), bounds(delete));
        assert!(send.right() <= edit.left() && edit.right() <= delete.left());
        assert!(first_row.right() - delete.right() <= px(8.));
    }
}

#[gpui::test]
fn queue_row_send_promotes_after_yield_and_interrupts_only_the_steered_row(
    cx: &mut gpui::TestAppContext,
) {
    use jcode_sdk::api::QueuedPromptMode::{AfterYield, Steer};
    let (_, vcx, commands) = queue_edit_panel(
        cx,
        vec![
            editing_test_prompt("a", "first", AfterYield),
            editing_test_prompt("b", "second", Steer),
            editing_test_prompt("c", "third", Steer),
        ],
    );
    let after_yield = vcx.debug_bounds("queue-row-send-now-0").unwrap();
    vcx.simulate_click(after_yield.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::SetQueuedPromptMode { client_id, mode: Steer, .. }) if client_id == "a")
    );
    let steer = vcx.debug_bounds("queue-row-send-now-1").unwrap();
    vcx.simulate_click(steer.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompt { client_id, .. }) if client_id == "b")
    );
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn queue_send_all_now_only_interrupts_its_own_section(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode::{AfterYield, Steer};
    let (_, vcx, commands) = queue_edit_panel(
        cx,
        vec![
            editing_test_prompt("a", "first", AfterYield),
            editing_test_prompt("b", "second", Steer),
            editing_test_prompt("c", "third", AfterYield),
        ],
    );
    for (selector, expected) in [
        ("queue-send-all-steer", vec!["b"]),
        ("queue-send-all-after-yield", vec!["a", "c"]),
    ] {
        let send = vcx.debug_bounds(selector).unwrap();
        vcx.simulate_click(send.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { ids, .. }) if ids == expected)
        );
    }
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn queue_edit_mode_is_staged_until_save(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::QueuedPromptMode::{AfterYield, Steer};
    let (_, vcx, commands) =
        queue_edit_panel(cx, vec![editing_test_prompt("a", "queued", AfterYield)]);
    let mut click = |selector| {
        let position = vcx.debug_bounds(selector).unwrap().center();
        vcx.simulate_click(position, gpui::Modifiers::default());
        vcx.run_until_parked();
    };
    click("remote-queue-edit-0");
    click("queue-edit-mode-steer");
    click("queue-edit-cancel");
    assert!(matches!(
        drain_commands(&commands).as_slice(),
        [
            Command::SetQueuedPromptEditing { editing: true, .. },
            Command::SetQueuedPromptEditing { editing: false, .. }
        ]
    ));
    click("remote-queue-edit-0");
    click("queue-edit-mode-steer");
    click("queue-edit-save");
    assert!(
        matches!(drain_commands(&commands).as_slice(), [Command::SetQueuedPromptEditing { editing: true, .. }, Command::SetQueuedPromptMode { client_id, mode: Steer, .. }, Command::EditQueuedPrompt { prompt_id, .. }] if client_id == "a" && prompt_id == "a")
    );
}

#[gpui::test]
fn enter_with_text_sends_only_the_new_prompt_and_keeps_after_yield_queued(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        panel.update(cx, |p, cx| p.apply(&event(json!({"ev":"queued_prompts", "prompts":[{"client_id":"a","content":"first","mode":"steer"},{"client_id":"b","content":"second","mode":"after_yield"}]})), cx));
        let focus = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_input("new text");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let new_id = match commands.try_recv() {
        Ok(Command::QueuePrompt { prompt, .. }) => {
            assert_eq!(prompt.content, "new text");
            assert_eq!(prompt.mode, jcode_sdk::api::QueuedPromptMode::AfterYield);
            prompt.client_id
        }
        _ => panic!("expected queue command"),
    };
    match commands.try_recv() {
        Ok(Command::InterruptQueuedPrompts { ids, .. }) => assert_eq!(
            ids,
            [new_id.clone()],
            "plain Enter promotes only the prompt just typed"
        ),
        _ => panic!("expected a targeted interrupt"),
    }
    assert!(
        commands.try_recv().is_err(),
        "the other queued prompts keep their mode"
    );
    panel.read_with(vcx, |p, _| {
        let mode = |id: &str| {
            p.prompt_queue
                .remote
                .iter()
                .find(|prompt| prompt.client_id == id)
                .map(|prompt| prompt.mode)
        };
        assert_eq!(mode("a"), Some(jcode_sdk::api::QueuedPromptMode::Steer));
        assert_eq!(
            mode("b"),
            Some(jcode_sdk::api::QueuedPromptMode::AfterYield),
            "the after-yield prompt still waits for the turn to yield"
        );
    });
}

#[gpui::test]
fn enter_while_pending_promotes_only_the_new_prompt_once_connected(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(Panel::STARTUP_SESSION_ID.into(), None, None, bridge, cx)
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("after yield prompt");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.simulate_input("send this now");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(commands.try_recv().is_err(), "nothing leaves before attach");
    panel.update(vcx, |p, cx| {
        p.attach_startup_session(attached_session("session-ready"), cx);
        p.status = "running".into();
        p.apply(&event(json!({"ev":"queued_prompts", "session_id":"session-ready", "prompts":[]})), cx);
        p.load_history(vec![], vec![], cx);
    });
    vcx.run_until_parked();
    let sent = drain_commands(&commands);
    let queued: Vec<_> = sent
        .iter()
        .filter_map(|command| match command {
            Command::QueuePrompt { prompt, .. } => Some(prompt.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(queued.len(), 2, "both prompts reach the daemon queue");
    let after_yield = queued.iter().find(|p| p.content == "after yield prompt").unwrap();
    let now = queued.iter().find(|p| p.content == "send this now").unwrap();
    assert_eq!(after_yield.mode, jcode_sdk::api::QueuedPromptMode::AfterYield);
    let interrupts: Vec<_> = sent
        .iter()
        .filter_map(|command| match command {
            Command::InterruptQueuedPrompts { ids, .. } => Some(ids.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        interrupts,
        [vec![now.client_id.clone()]],
        "only the Enter prompt is promoted"
    );
}

#[gpui::test]
fn enter_on_legacy_local_queue_steers_only_the_new_prompt(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    panel.update(cx, |p, cx| {
        p.submit_or_queue("after yield prompt".into(), vec![], true, cx);
        p.submit_or_queue("send this now".into(), vec![], false, cx);
    });
    cx.run_until_parked();
    let sent = drain_commands(&commands);
    assert!(
        matches!(sent.as_slice(), [Command::Send { content, .. }] if content == "send this now"),
        "only the new prompt is sent now"
    );
    panel.read_with(cx, |p, _| {
        assert_eq!(p.prompt_queue.prompts.len(), 1);
        assert_eq!(p.prompt_queue.prompts[0].content, "after yield prompt");
        assert!(p.prompt_queue.prompts[0].after_yield);
        assert!(!p.prompt_queue.send_all_on_connect);
    });
}

#[gpui::test]
fn empty_enter_with_queue_interrupts_now(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        panel.update(cx, |p, cx| p.apply(&event(json!({"ev":"queued_prompts", "prompts":[{"client_id":"a","content":"first","mode":"steer"},{"client_id":"b","content":"second","mode":"after_yield"}]})), cx));
        let focus = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { ids, .. }) if ids == ["a", "b"]),
        "empty Enter must trigger batch interrupt"
    );
}

#[gpui::test]
fn unconfirmed_batch_escalates_and_shows_reason(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel.apply(&event(json!({"ev":"queued_prompts", "prompts":[{"client_id":"a","content":"first","mode":"after_yield"}]})), cx);
        panel
    });
    panel.update(cx, |p, cx| p.interrupt_all_queued_prompts(cx));
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::InterruptQueuedPrompts { .. })
    ));
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::Cancel { .. })));
    panel.read_with(cx, |p, _| {
        assert!(p.prompt_queue.remote_failures["a"].contains("blocked"))
    });
}

#[gpui::test]
fn hidden_retry_output_resumes_queue_without_acceptance_but_cancelled_retry_does_not(
    cx: &mut gpui::TestAppContext,
) {
    for cancelled in [false, true] {
        for output in [
            json!({"ev":"text_delta", "text":"Recovered"}),
            json!({"ev":"reasoning_delta", "text":"Checking again"}),
            json!({"ev":"tool_start", "call_id":"retry-tool", "name":"read"}),
        ] {
            let (bridge, commands) = crate::harness::spawn_recording();
            let panel = cx.new(|cx| {
                let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
                panel.history_loaded = true;
                panel.status = "running".into();
                panel
            });
            panel.update(cx, |panel, cx| {
                panel.submit_or_queue("follow up".into(), vec![], true, cx);
                panel.apply(
                    &event(json!({"ev":"error", "code":"internal", "message":"connection reset"})),
                    cx,
                );
                panel.apply(
                    &event(json!({"ev":"connection_phase", "phase":"retrying in 1s (1/3)"})),
                    cx,
                );
                if cancelled {
                    panel.apply(&event(json!({"ev":"connection_phase", "phase":""})), cx);
                }
                panel.apply(&event(output), cx);
            });
            cx.run_until_parked();
            assert!(
                commands.try_recv().is_err(),
                "retry output is not turn completion"
            );
            panel.update(cx, |panel, cx| {
                panel.apply(
                    &event(json!({"ev":"turn_done", "session_id":"queue-test"})),
                    cx,
                );
                panel.apply(&event(json!({"ev":"session_status", "status":"idle"})), cx);
            });
            cx.run_until_parked();
            if cancelled {
                assert!(commands.try_recv().is_err());
                panel.read_with(cx, |panel, _| assert!(panel.prompt_queue.paused));
            } else {
                assert!(
                    matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "follow up")
                );
                assert!(commands.try_recv().is_err(), "queue sends only once");
            }
        }
    }
}

#[gpui::test]
fn queued_follow_up_resumes_after_retry_but_not_after_user_stop(cx: &mut gpui::TestAppContext) {
    for cancel in [false, true] {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| {
            let mut p = Panel::new("queue-test".into(), None, None, bridge, cx);
            p.history_loaded = true;
            p.status = "running".into();
            p
        });
        panel.update(cx, |p, cx| {
            p.submit_or_queue("follow up".into(), vec![], true, cx);
            p.apply(&event(json!({"ev":"error", "code":"internal", "message":"connection reset"})), cx);
            p.apply(&event(json!({"ev":"turn_done", "session_id":"queue-test"})), cx);
            p.apply(&event(json!({"ev":"connection_phase", "session_id":"queue-test", "phase":"retrying in 1s (1/3)"})), cx);
        });
        cx.run_until_parked();
        assert!(
            commands.try_recv().is_err(),
            "failed attempt must not drain queue"
        );
        panel.update(cx, |p, cx| {
            if cancel {
                p.pause_queue_for_stop();
            }
            p.apply(&event(json!({"ev":"message_accepted", "session_id":"queue-test"})), cx);
            p.apply(&event(json!({"ev":"text_delta", "session_id":"queue-test", "text":"Recovered answer"})), cx);
        });
        cx.run_until_parked();
        assert!(commands.try_recv().is_err(), "retry is still responding");
        panel.update(cx, |p, cx| {
            p.apply(
                &event(json!({"ev":"turn_done", "session_id":"queue-test"})),
                cx,
            );
            p.apply(
                &event(json!({"ev":"session_status", "session_id":"queue-test", "status":"idle"})),
                cx,
            );
        });
        cx.run_until_parked();
        if cancel {
            assert!(
                commands.try_recv().is_err(),
                "user stop must keep queue paused"
            );
            panel.read_with(cx, |p, _| assert_eq!(p.prompt_queue.prompts.len(), 1));
        } else {
            assert!(
                matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "follow up")
            );
            assert!(
                commands.try_recv().is_err(),
                "completion must send only once"
            );
        }
    }
}

#[gpui::test]
fn ctrl_enter_queues_until_completion_and_enter_still_sends(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.simulate_input("first queued prompt");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.simulate_input("second queued prompt");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "queue must not steer the active turn"
    );
    let queue = vcx.debug_bounds("prompt-queue").unwrap();
    let input = vcx.debug_bounds("prompt-input").unwrap();
    assert!(
        queue.bottom() <= input.top(),
        "queue must be above the input"
    );
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.prompts.len(), 2);
        assert!(p.input.read(cx).content.is_empty());
        assert!(
            p.items.is_empty(),
            "waiting prompts are not transcript messages yet"
        );
    });
    panel.update(vcx, |p, cx| {
        p.apply(
            &event(json!({"ev":"turn_done", "session_id":"queue-test"})),
            cx,
        );
        p.apply(
            &event(json!({"ev":"session_status", "session_id":"queue-test", "status":"idle"})),
            cx,
        );
    });
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, images, .. })
        if content == "first queued prompt\n\nsecond queued prompt" && images.is_empty())
    );
    assert!(
        commands.try_recv().is_err(),
        "duplicate completion must not replay the queue"
    );
    assert!(vcx.debug_bounds("prompt-queue").is_none());

    vcx.simulate_input("send immediately");
    vcx.simulate_keystrokes("enter");
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "send immediately")
    );
}

#[gpui::test]
fn queue_survives_reload_with_images_and_pauses_on_cancel(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut p = Panel::new("queue-test".into(), None, None, bridge, cx);
        p.history_loaded = true;
        p.status = "running".into();
        p
    });
    let images = vec![("image/png".into(), "image bytes".into())];
    panel.update(cx, |p, cx| {
        p.submit_or_queue("look at this".into(), images.clone(), true, cx);
        let snapshot = serde_json::to_vec(&p.snapshot(cx)).unwrap();
        p.prompt_queue = Default::default();
        p.restore_snapshot(serde_json::from_slice(&snapshot).unwrap(), cx);
        assert_eq!(p.prompt_queue.prompts[0].images, images);
        p.apply(
            &event(json!({"ev":"session_status", "session_id":"queue-test", "status":"cancelled"})),
            cx,
        );
        p.apply(
            &event(json!({"ev":"turn_done", "session_id":"queue-test"})),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(commands.try_recv().is_err());
    panel.update(cx, |p, cx| {
        assert!(p.prompt_queue.paused);
        p.prompt_queue.paused = false;
        p.send_queued_prompts(cx);
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, images: actual, .. })
        if content == "look at this" && actual == images)
    );
}

#[gpui::test]
fn idle_queue_shortcut_sends_and_unaccepted_prompt_gates_next_queue(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let panel = cx.new(|cx| {
        let mut p = Panel::new("queue-test".into(), None, None, bridge, cx);
        p.history_loaded = true;
        p.status = "idle".into();
        p
    });
    panel.update(cx, |p, cx| {
        p.submit_or_queue("idle prompt".into(), vec![], true, cx);
        p.submit_or_queue("waiting for acceptance".into(), vec![], true, cx);
        p.apply(
            &event(json!({"ev":"session_status", "session_id":"queue-test", "status":"idle"})),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "idle prompt")
    );
    assert!(commands.try_recv().is_err());
    panel.update(cx, |p, cx| {
        p.apply(
            &event(json!({"ev":"message_accepted", "session_id":"queue-test"})),
            cx,
        );
        p.apply(
            &event(json!({"ev":"turn_done", "session_id":"queue-test"})),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if content == "waiting for acceptance")
    );
}

fn attached_session(id: &str) -> jcode_sdk::SessionInfo {
    serde_json::from_value(json!({
        "session_id": id, "title": "Connected session", "status": "idle"
    }))
    .unwrap()
}

#[path = "panel_queue_more_tests.rs"]
mod more;
use more::*;
