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
    assert!(matches!(commands.try_recv(), Ok(Command::SessionOperation {
        operation: SessionOperation::SetEffort(level), ..
    }) if level == "high"));
    assert!(commands.try_recv().is_err());
}
use serde_json::json;

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
                content: "keep me".into(),
                images: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
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
        matches!(commands.try_recv(), Ok(Command::QueuePrompt { prompt, .. }) if prompt.client_id == "restore-id" && prompt.content == "keep me")
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
                content: "pending".into(),
                images: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
            });
        panel
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("daemon-prompt-queue").is_some());
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
        let mut panel = Panel::new("queue-test".into(), None, None, crate::harness::spawn_inert(), cx);
        panel.history_loaded = true;
        panel
    });
    panel.update(cx, |panel, cx| {
        for content in ["plain external prompt", "/poteto-mode давай делать"] {
            panel.apply(&event(json!({"ev":"peer_user_message", "content":content})), cx);
        }
        let users: Vec<_> = panel.items.iter().filter_map(|item| match item { Item::User(text) => Some(text.as_str()), _ => None }).collect();
        assert_eq!(users, ["plain external prompt", "/poteto-mode давай делать"]);
    });
    let restored = cx.new(|cx| Panel::new("queue-test".into(), None, None, crate::harness::spawn_inert(), cx));
    restored.update(cx, |panel, cx| {
        let messages = serde_json::from_value(json!([
            {"role":"user", "content":"plain external prompt"},
            {"role":"assistant", "content":"answer"},
            {"role":"system", "content":"skill instructions"},
            {"role":"user", "content":"/poteto-mode давай делать"}
        ])).unwrap();
        panel.load_history(messages, vec![], cx);
        let users: Vec<_> = panel.items.iter().filter_map(|item| match item { Item::User(text) => Some(text.as_str()), _ => None }).collect();
        assert_eq!(users, ["plain external prompt", "/poteto-mode давай делать"]);
        assert!(!panel.items.iter().any(|item| matches!(item, Item::User(text) if text.contains("skill instructions"))));
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
            client_id: "known".into(), content: "local".into(), images: vec![],
            mode: jcode_sdk::api::QueuedPromptMode::Steer,
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
    assert!(vcx.debug_bounds("daemon-prompt-queue").is_none());
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
            images: vec![(image.media_type, image.data)],
            after_yield: true,
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
                content: "Look [#image 1]".into(),
                images: vec![(image.media_type, image.data)],
                mode: jcode_sdk::api::QueuedPromptMode::Steer,
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
            images: remote.images,
            after_yield: false,
        });
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("steered-prompts").is_some());
    assert!(vcx.debug_bounds("queue-image-thumbnail").is_some());
    let chip = vcx.debug_bounds("queue-image-chip").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
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
                images: vec![],
                after_yield: true,
            },
            QueuedPrompt {
                content: "two".into(),
                images: vec![],
                after_yield: true,
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
                content: "already delivered".into(),
                images: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::AfterYield,
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
                content: "repeat".into(),
                images: vec![],
                mode: jcode_sdk::api::QueuedPromptMode::AfterYield,
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
fn queued_row_has_visible_send_now_action(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new("queue-test".into(), None, None, bridge, cx);
        panel.history_loaded = true;
        panel.status = "running".into();
        panel
    });
    panel.update(vcx, |p, cx| p.apply(&event(json!({"ev":"queued_prompts", "prompts":[{"client_id":"a","content":"first","mode":"steer"},{"client_id":"b","content":"second","mode":"after_yield"}]})), cx));
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("queue-row-send-now-0").is_some(),
        "row Send now must be visible without opening a menu"
    );
    assert!(
        vcx.debug_bounds("queue-send-all-now").is_some(),
        "header must offer Send all now"
    );
    let row = vcx.debug_bounds("queue-row-send-now-0").unwrap();
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { ids, .. }) if ids == ["a", "b"])
    );
    let header = vcx.debug_bounds("queue-send-all-now").unwrap();
    vcx.simulate_click(header.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { ids, .. }) if ids == ["a", "b"])
    );
}

#[gpui::test]
fn enter_with_text_adds_it_to_the_queued_batch(cx: &mut gpui::TestAppContext) {
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
    assert!(
        matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { ids, .. }) if ids == ["a", "b", &new_id])
    );
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
    assert!(matches!(commands.try_recv(), Ok(Command::InterruptQueuedPrompts { .. })));
    cx.executor().advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::Cancel { .. })));
    panel.read_with(cx, |p, _| assert!(p.prompt_queue.remote_failures["a"].contains("blocked")));
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

#[gpui::test]
fn startup_enter_and_ctrl_enter_queue_until_attached_history_ready(cx: &mut gpui::TestAppContext) {
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
        matches!(commands.try_recv(), Ok(Command::Send { session_id, content, images })
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
        matches!(commands.try_recv(), Ok(Command::Send { session_id, content, images: actual })
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
        let handle = panel.read(cx).input.read(cx).focus_handle.clone();
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
    assert!(items.size.height <= px(144.));
    assert!(queue.bottom() <= input.top());
    let first = vcx.debug_bounds("queued-prompt-0").unwrap();
    let second = vcx.debug_bounds("queued-prompt-1").unwrap();
    assert!(first.bottom() < second.top(), "rows need breathing room");
    assert!(
        first.size.height >= px(24.),
        "rows must not shrink to fit the stack"
    );
    let overflow = vcx.debug_bounds("queue-overflow-0").unwrap();
    let delete = vcx.debug_bounds("remove-queued-prompt-0").unwrap();
    assert!(
        delete.right() <= overflow.left() && overflow.right() <= items.right(),
        "long text must not push actions out"
    );
    vcx.simulate_click(overflow.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let recall = vcx.debug_bounds("recall-queued-prompt-0").unwrap();
    vcx.simulate_click(recall.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.read_with(vcx, |p, cx| {
        assert_eq!(p.prompt_queue.prompts.len(), 11);
        assert!(
            p.prompt_queue.prompts[0]
                .content
                .starts_with("Follow-up 1:")
        );
        assert!(
            p.input.read(cx).content.starts_with("Follow-up 0:"),
            "recall puts the prompt back in the composer"
        );
    });
    // The recalled prompt may grow the composer. Scrolling must not move it.
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

fn steered_panel<'a>(
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
