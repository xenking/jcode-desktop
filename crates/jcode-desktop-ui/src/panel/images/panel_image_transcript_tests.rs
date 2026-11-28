use super::*;

#[gpui::test]
fn live_user_image_marker_is_replaced_by_server_session_id(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("live-image-ids".into(), None, None, crate::harness::spawn_inert(), cx));
    let old = "look [#image 1]";
    let id = "img_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User(old.into()), Item::Image(image_preview::fixture_image())];
        let event = serde_json::from_value(serde_json::json!({"ev":"user_image_ids", "session_id":"live-image-ids", "original_content":old, "content":format!("look [#{id}]"), "image_ids":[id], "image_ordinals":[7]})).unwrap();
        panel.apply(&event, cx);
        assert!(matches!(&panel.items[0], Item::User(text) if text == &format!("look [#{id}]")));
        assert_eq!(panel.input.read(cx).session_image_base(), 7);
        assert!(matches!(&panel.items[1], Item::Image(image) if image.image_ordinal() == Some(7) && image.has_image_id(id)));
    });
}

#[gpui::test]
fn wired_composer_echo_uses_the_next_session_image_number(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("composer-image-ordinal".into(), None, None, bridge, cx)
    });
    vcx.update(|_, cx| Panel::connect_input(&panel, cx));
    panel.update(vcx, |panel, cx| {
        panel.load_history(vec![], vec![], cx);
        panel.note_session_image_ordinal(Some(7), cx);
        panel.input.update(cx, |input, cx| {
            let mut draft = input.snapshot();
            draft.content = "look [#image 2]".into();
            draft.attachments.push(crate::input::AttachmentSnapshot {
                media_type: "image/png".into(),
                encoded: image_preview::fixture_image().data,
                label: "[#image 2]".into(),
            });
            input.restore(draft, cx);
        });
    });
    vcx.update(|window, cx| panel.update(cx, |panel, cx| panel.focus_input(window, cx)));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(commands.try_iter().any(|command| matches!(command, Command::Send { content, images, .. }
        if content == "look [#image 1]" && images.len() == 1)));
    panel.read_with(vcx, |panel, _| {
        assert!(matches!(panel.items.last(), Some(Item::Image(image)) if image.image_ordinal() == Some(8)));
    });
}

#[gpui::test]
fn session_gallery_appears_only_after_older_images_are_known(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("gallery-old-image".into(), None, None, bridge, cx));
    panel.update(vcx, |panel, cx| { panel.history_loaded = true; cx.notify(); });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-images").is_none());
    let id = "img_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let images = serde_json::json!([{ "image_id": id, "ordinal": 8, "message_index": 4, "timestamp": "2026-01-01T00:00:00Z", "media_type": "image/png" }]).to_string();
    panel.update(vcx, |panel, cx| panel.load_history_image(crate::harness::SESSION_IMAGE_LIST_KEY.into(), Ok(images), cx));
    vcx.run_until_parked();
    let button = vcx.debug_bounds("panel-images").unwrap();
    vcx.simulate_click(button.center(), Default::default());
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::ListSessionImages { session_id }) if session_id == "gallery-old-image"));
    let image = vcx.debug_bounds("session-gallery-image-0").expect("image outside loaded history appears in gallery");
    vcx.simulate_click(image.center(), Default::default());
    assert!(matches!(commands.try_recv(), Ok(Command::GetHistoryImage { key, .. }) if key == id));
    panel.update(vcx, |panel, cx| panel.load_history_image(id.into(), Ok(image_preview::fixture_image().data), cx));
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, cx| panel.image_preview.as_ref().is_some_and(|image| image.image_ordinal() == Some(8)) && panel.input.read(cx).session_image_base() == 8), "gallery aliases preserve the early image number and update the composer");
}

fn image_marker_history_message() -> jcode_sdk::HistoryMessage {
    jcode_sdk::HistoryMessage {
        attachments: Vec::new(),
        role: "user".into(), content: "text[#image 1]more".into(),
        tool: None, timestamp_ms: None, response_stats: None,
    }
}

#[gpui::test]
fn reconnect_history_keeps_user_images_at_their_original_prompt(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("image-reconnect".into(), None, None, crate::harness::spawn_inert(), cx));
    let ids = (1..=5).map(|n| format!("img_{n:032x}")).collect::<Vec<_>>();
    let content = format!("Inspect {}", ids.iter().map(|id| format!("[#{id}]")).collect::<Vec<_>>().join(" "));
    let messages = serde_json::from_value::<Vec<jcode_sdk::HistoryMessage>>(serde_json::json!([
        {"role":"user", "content":content, "timestamp_ms":1795181596287i64},
        {"role":"system", "content":"[auto] Continue the work below"},
        {"role":"tool", "content":"Filesystem", "tool":{"id":"df","name":"bash","input":{"command":"df -h"}}}
    ])).unwrap();
    let fixture = image_preview::fixture_image();
    let images = ids.iter().enumerate().map(|(i, id)| jcode_sdk::RenderedImage {
        media_type: "image/png".into(),
        data: if i < 2 { fixture.data.clone() } else { format!("jcode-lazy:{id}") },
        label: None,
        source: jcode_sdk::RenderedImageSource::UserInput,
        anchor: Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal: 0 }),
        session_image: Some(jcode_sdk::api::SessionImageRef { image_id: id.clone(), ordinal: i + 10 }),
        history_message_index: None,
    }).collect::<Vec<_>>();
    panel.update(vcx, |panel, cx| {
        panel.load_history(messages.clone(), vec![], cx);
        // Desktop echoed the queued attachments before the history reply
        // reintroduced them with anchors and potentially lazy bytes.
        for _ in &ids {
            panel.items.insert(1, Item::Image(fixture.clone()));
        }
        panel.load_history(messages.clone(), images.clone(), cx);
        panel.load_bounded_history(crate::harness::BoundedHistoryPage {
            messages,
            images: ids.iter().zip(&images).map(|(id, image)| jcode_sdk::RenderedImage {
                data: format!("jcode-lazy:{id}"), ..image.clone()
            }).collect(),
            before: None,
            prompt_offset: Some(0),
        }, cx);
        assert_eq!(panel.session_image_indices(), vec![1, 2, 3, 4, 5]);
        assert_eq!(panel.user_image_markers(0).len(), 5);
        assert_eq!(panel.input.read(cx).session_image_base(), 14);
        for (i, id) in ids.iter().enumerate() {
            assert!(matches!(&panel.items[i + 1], Item::Image(image) if image.image_ordinal() == Some(i + 10) && image.has_image_id(id)));
        }
        assert!(panel.items[1..6].iter().all(|item| matches!(item, Item::Image(image) if image.preview.is_some())));
        assert!(matches!(&panel.items[6], Item::Tool { name, .. } if name == "bash"));
        assert_eq!(panel.transcript_render_rows().len(), 2);
    });
}

#[gpui::test]
fn restored_user_images_need_a_visible_owner_and_fill_its_missing_slots(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("history-image-owners".into(), None, None, crate::harness::spawn_inert(), cx));
    let messages = serde_json::from_value::<Vec<jcode_sdk::HistoryMessage>>(serde_json::json!([
        {"role":"user", "content":"You have 1 incomplete todo. Continue the work and update the todo tool."},
        {"role":"user", "content":"Inspect [#image 1]"},
        {"role":"tool", "content":"Filesystem", "tool":{"id":"df","name":"bash","input":{}}},
    ])).unwrap();
    let anchored = |ordinal| jcode_sdk::RenderedImage {
        media_type: "image/png".into(), data: "jcode-lazy:abc".into(), label: None,
        source: jcode_sdk::RenderedImageSource::UserInput,
        anchor: Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal }),
        session_image: None,
        history_message_index: None,
    };
    let rejected = vec![anchored(0), anchored(3), jcode_sdk::RenderedImage {
        anchor: None, history_message_index: Some(2), ..anchored(1)
    }];
    panel.update(vcx, |panel, cx| {
        panel.load_history(messages.clone(), rejected.clone(), cx);
        assert!(panel.session_image_indices().is_empty());
        panel.load_history(messages, [rejected, vec![anchored(1)]].concat(), cx);
        assert_eq!(panel.session_image_indices(), vec![2]);
        assert!(matches!(&panel.items[3], Item::Tool { name, .. } if name == "bash"));
        let rows_before = panel.transcript_render_rows().len();
        // Existing hot-reload caches can still contain the previous loose copy.
        panel.items.push(Item::Image(TranscriptImage::from_rendered(anchored(1))));
        assert_eq!(panel.transcript_render_rows().len(), rows_before);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-chip-placeholder").is_some());
    assert!(vcx.debug_bounds("transcript-user-image-chip").is_none());
}

fn assert_marker_masked_in_rendered_prompt() {
    let rendered = crate::markdown::take_rendered_prompt_text();
    let line = rendered.iter().find(|line| line.starts_with("text") && line.ends_with("more"))
        .expect("the user message was rendered");
    assert!(!line.contains("[#image"), "marker leaked into rendered text: {line}");
}

#[gpui::test]
fn missing_image_marker_in_loaded_history_is_a_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("image-history".into(), None, None, crate::harness::spawn_inert(), cx));
    panel.update(vcx, |panel, cx| panel.load_history(vec![image_marker_history_message()], vec![], cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-image-chip").is_some(), "history marker must render as a chip");
    assert!(vcx.debug_bounds("image-chip-placeholder").is_some());
    let chip = vcx.debug_bounds("transcript-image-chip").unwrap();
    let label = vcx.debug_bounds("transcript-image-label").unwrap();
    assert!(label.right() <= chip.right(), "placeholder label must fit");
    assert_marker_masked_in_rendered_prompt();
}

#[gpui::test]
fn missing_image_marker_in_live_steer_is_a_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("image-live".into(), None, None, crate::harness::spawn_inert(), cx));
    panel.update(vcx, |panel, cx| {
        panel.history_loaded = true;
        let event = serde_json::from_value(serde_json::json!({"ev":"soft_interrupt_injected", "session_id":"image-live", "content":"text[#image 1]more", "point":"D", "client_ids":[]})).unwrap();
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-image-chip").is_some(), "live marker must render as a chip");
    assert_marker_masked_in_rendered_prompt();
}

#[gpui::test]
fn missing_image_marker_in_local_send_echo_is_a_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("image-echo".into(), None, None, crate::harness::spawn_inert(), cx));
    panel.update(vcx, |panel, cx| {
        panel.history_loaded = true;
        panel.submit_or_queue("text[#image 1]more".into(), vec![], false, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-image-chip").is_some());
    assert_marker_masked_in_rendered_prompt();
}

#[gpui::test]
fn missing_image_marker_in_paged_history_is_a_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("image-paged".into(), None, None, crate::harness::spawn_inert(), cx));
    panel.update(vcx, |panel, cx| {
        panel.load_bounded_history(crate::harness::BoundedHistoryPage { messages: vec![], images: vec![], before: Some("older".into()), prompt_offset: None }, cx);
        panel.load_bounded_history_page(Ok(crate::harness::BoundedHistoryPage { messages: vec![image_marker_history_message()], images: vec![], before: None, prompt_offset: None }), cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-image-chip").is_some(), "paged marker must render as a chip");
    assert_marker_masked_in_rendered_prompt();
}

#[gpui::test]
fn raw_image_references_in_paged_history_remain_compact(cx: &mut gpui::TestAppContext) {
    let ids = crate::image_chip::ordinal_fixture_ids();
    let ids = ids.each_ref().map(String::as_str);
    let source = format!("Сначала [#{}] потом [#{}] готово", ids[0], ids[1]);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("raw-paged-images".into(), None, None, bridge, cx));
    let mut message = image_marker_history_message();
    message.content = source.clone();
    let reply = |index| jcode_sdk::HistoryMessage {
        role: "assistant".into(),
        content: format!("Offline reply {index}"),
        ..message.clone()
    };
    let messages = (0..38).map(reply).chain(std::iter::once(message.clone())).chain((38..47).map(reply)).collect();
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(1000.), px(750.)));
    panel.update(vcx, |panel, cx| {
        panel.load_bounded_history(
            crate::harness::BoundedHistoryPage {
                messages,
                images: vec![],
                before: Some("older".into()),
                prompt_offset: Some(117),
            },
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::ListSessionImages { .. })
    ));
    let take_chips = |expected: [&str; 2], vcx: &mut gpui::VisualTestContext| {
        let mut chips = crate::image_chip::take_transcript_chips();
        assert!(chips.len() >= 2, "both inline chips must paint: {chips:?}");
        let chips = chips.split_off(chips.len() - 2);
        for ((label, bounds), expected) in chips.iter().zip(expected) {
            assert_eq!(label, expected);
            assert!(
                bounds.size.width > px(40.) && bounds.size.width < px(100.),
                "unexpected reserved width: {chips:?}"
            );
        }
        let label = vcx
            .debug_bounds("transcript-image-label")
            .expect("visible label");
        let chip = chips
            .iter()
            .find(|(_, bounds)| bounds.contains(&label.center()))
            .unwrap()
            .1;
        assert!(
            (chip.right() - label.right()).abs() <= px(9.),
            "chip leaves blank space or clips label: {chip:?} {label:?}"
        );
        assert!(
            crate::markdown::take_rendered_prompt_text()
                .iter()
                .all(|text| !text.contains("img_"))
        );
        chips
    };
    let open = |chip: gpui::Bounds<gpui::Pixels>, image_id, vcx: &mut gpui::VisualTestContext| {
        vcx.simulate_click(chip.center(), Default::default());
        vcx.run_until_parked();
        let mut received = Vec::new();
        while let Ok(command) = commands.try_recv() {
            if let Command::GetHistoryImage { key, .. } = command {
                received.push(key == image_id);
            }
        }
        assert!(
            received == [true], "expected exactly one request for the clicked chip {chip:?}, matches: {received:?}"
        );
    };
    let chips = take_chips(["[Image]", "[Image]"], vcx);
    eprintln!(
        "unresolved at default width: {:?}",
        chips
            .iter()
            .map(|(_, chip)| chip.size.width)
            .collect::<Vec<_>>()
    );
    open(chips[0].1, ids[0], vcx);
    panel.update(vcx, |panel, cx| {
        panel.load_history_image(ids[0].into(), Err("unavailable test image".into()), cx)
    });
    vcx.run_until_parked();
    let _ = take_chips(["[Image]", "[Image]"], vcx);
    panel.update(vcx, |panel, cx| {
        panel.load_history_image(
            crate::harness::SESSION_IMAGE_LIST_KEY.into(),
            Ok(serde_json::json!([
                {"image_id": ids[1], "ordinal":114, "message_index":38, "media_type":"image/png"},
                {"image_id": ids[0], "ordinal":113, "message_index":38, "media_type":"image/png"}
            ])
            .to_string()),
            cx,
        );
        assert!(matches!(panel.items.get(38), Some(Item::User(content)) if content == &source));
    });
    vcx.run_until_parked();
    let chips = take_chips(["[Image #113]", "[Image #114]"], vcx);
    eprintln!(
        "resolved at default width: {:?}",
        chips
            .iter()
            .map(|(_, chip)| chip.size.width)
            .collect::<Vec<_>>()
    );
    let leaf = vcx
        .debug_bounds("selectable-text-38-0")
        .expect("prompt text must remain selectable");
    let start = point(leaf.left() + px(1.), leaf.center().y);
    let end = point(leaf.right() + px(20.), leaf.center().y);
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: start,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    vcx.simulate_event(gpui::MouseMoveEvent {
        position: end,
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: Default::default(),
    });
    vcx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Left,
        position: end,
        modifiers: Default::default(),
        click_count: 1,
    });
    assert_eq!(
        vcx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .as_deref(),
        Some(source.as_str())
    );
    vcx.simulate_window_resize(window, gpui::size(px(350.), px(600.)));
    vcx.run_until_parked();
    let chips = take_chips(["[Image #113]", "[Image #114]"], vcx);
    eprintln!(
        "resolved at 350 px: {:?}",
        chips
            .iter()
            .map(|(_, chip)| chip.size.width)
            .collect::<Vec<_>>()
    );
    panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(true), cx));
    vcx.run_until_parked();
    let chips = take_chips(["[Image #113]", "[Image #114]"], vcx);
    for ((_, chip), id) in chips.iter().zip(ids) {
        open(*chip, id, vcx);
        panel.update(vcx, |panel, cx| {
            panel.load_history_image(id.into(), Ok(image_preview::fixture_image().data), cx)
        });
        vcx.run_until_parked();
        assert!(panel.read_with(vcx, |panel, _| {
            panel
                .image_preview
                .as_ref()
                .is_some_and(|image| image.has_image_id(id))
        }));
        vcx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.close_image_preview(window, cx))
        });
        vcx.run_until_parked();
    }
}

#[gpui::test]
fn text_only_live_image_ids_supply_session_ordinals(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "text-only-images".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    let id = "img_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let marker = format!("[#{id}]");
    panel.update(vcx, |panel, cx| {
        panel.history_loaded = true;
        *panel.items = vec![Item::User(format!("Сначала {marker} потом"))];
        let event = ApiEvent::UserImageIds {
            session_id: "text-only-images".into(),
            original_content: "Сначала [#image 1] потом".into(),
            content: format!("Сначала {marker} потом"),
            image_ids: vec![id.into()],
            image_ordinals: vec![113],
        };
        panel.apply(&event, cx);
    });
    vcx.run_until_parked();
    let chips = crate::image_chip::take_transcript_chips();
    assert!(chips.iter().any(|(label, bounds)| label == "[Image #113]" && bounds.size.width < px(100.)), "live text-only echo chips: {chips:?}");
}

#[gpui::test]
fn visible_lazy_history_image_fetches_thumbnail_and_opens_on_click(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("lazy-chip".into(), None, None, bridge, cx));
    let image = jcode_sdk::RenderedImage {
        media_type: "image/png".into(), data: "jcode-lazy:abc".into(), label: None,
        source: jcode_sdk::RenderedImageSource::UserInput,
        anchor: Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal: 0 }),
        session_image: None,
        history_message_index: None,
    };
    panel.update(vcx, |panel, cx| panel.load_history(vec![image_marker_history_message()], vec![image], cx));
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::GetHistoryImage { key, .. }) if key == "abc"));
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_none()));
    let chip = vcx.debug_bounds("transcript-image-chip").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    assert!(commands.try_recv().is_err(), "click must reuse the in-flight request");
    panel.update(vcx, |panel, cx| panel.load_history_image("abc".into(), Ok(image_preview::fixture_image().data), cx));
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
    assert!(vcx.debug_bounds("transcript-image-thumbnail").is_some());
}

#[gpui::test]
fn lazy_thumbnail_finishes_without_opening_preview(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("lazy-background".into(), None, None, bridge, cx));
    let image = jcode_sdk::RenderedImage {
        media_type: "image/png".into(), data: "jcode-lazy:background".into(), label: None,
        source: jcode_sdk::RenderedImageSource::UserInput,
        anchor: Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal: 0 }),
        session_image: None,
        history_message_index: None,
    };
    panel.update(vcx, |panel, cx| panel.load_history(vec![image_marker_history_message()], vec![image], cx));
    vcx.run_until_parked();
    assert!(matches!(commands.try_recv(), Ok(Command::GetHistoryImage { key, .. }) if key == "background"));
    panel.update(vcx, |panel, cx| panel.load_history_image("background".into(), Ok(image_preview::fixture_image().data), cx));
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_none()));
    assert!(vcx.debug_bounds("transcript-image-thumbnail").is_some());
}

#[gpui::test]
fn adjacent_missing_markers_are_masked(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("adjacent-images".into(), None, None, crate::harness::spawn_inert(), cx));
    let _ = crate::markdown::take_rendered_prompt_text();
    panel.update(vcx, |panel, cx| {
        let mut message = image_marker_history_message();
        message.content = "text[#image 1][#image 2]more".into();
        panel.load_history(vec![message], vec![], cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-image-chip").is_some());
    assert_marker_masked_in_rendered_prompt();
}

#[gpui::test]
fn transcript_chip_survives_wrap_boundaries(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("wrapped-image".into(), None, None, crate::harness::spawn_inert(), cx));
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(350.), px(600.)));
    for n in 1..50 {
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::User(format!("{}[#image 1]more", "x".repeat(n)))];
            panel.transcript_measurements.invalidate();
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("transcript-image-chip").is_some(), "prefix length {n}");
    }
}

#[gpui::test]
fn wrapped_transcript_chip_opens_image(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("wrapped-image-click".into(), None, None, crate::harness::spawn_inert(), cx));
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(350.), px(600.)));
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User(format!("{}[#image 1]more", "x".repeat(26))), Item::Image(image_preview::fixture_image())];
        cx.notify();
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("transcript-image-chip").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_some()));
}

#[gpui::test]
fn completed_ask_replaces_inline_marker_with_image_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("ask-inline-image".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        let mut image = image_preview::fixture_image();
        image.source = jcode_sdk::RenderedImageSource::ToolResult { tool_name: "ask".into() };
        image.anchor = Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: "ask-inline".into() });
        let output = "User answers:\n1. Which? -> Look [#image 1] here";
        *panel.items = vec![Item::Tool {
            call_id: "ask-inline".into(), name: "ask".into(), input: "{}".into(),
            output: output.into(), done: true, error: None, ask: ask::resolve_record("ask", None, output),
        }, Item::Image(image)];
        cx.notify();
    });
    vcx.run_until_parked();
    let row = vcx.debug_bounds("decision-header").expect("compact decision row");
    vcx.simulate_click(row.center(), Default::default());
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("ask-result-inline-image").expect("marker must render inline as a chip");
    assert!(vcx.debug_bounds("ask-result-inline-thumbnail").is_some());
    assert!(vcx.debug_bounds("ask-result-inline-label").is_some());
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn ask_chip_fetches_matching_session_id_even_when_image_order_differs(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("ask-session-id".into(), None, None, bridge, cx));
    let other = "img_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let comment = "img_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    panel.update(vcx, |panel, cx| {
        let make_image = |id: &str| {
            let mut image = TranscriptImage::new("image/png".into(), format!("jcode-lazy:{id}"), None);
            image.source = jcode_sdk::RenderedImageSource::ToolResult { tool_name: "ask".into() };
            image.anchor = Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: "ask-inline".into() });
            image
        };
        let output = format!("User answers:\n1. Which? -> B (comment: reason [#{comment}])");
        *panel.items = vec![
            Item::Tool { call_id: "ask-inline".into(), name: "ask".into(), input: "{}".into(),
                ask: ask::resolve_record("ask", None, &output), output, done: true, error: None },
            Item::Image(make_image(other)), Item::Image(make_image(comment)),
        ];
        cx.notify();
    });
    vcx.run_until_parked();
    let decision = vcx.debug_bounds("decision-header").unwrap();
    vcx.simulate_click(decision.center(), Default::default());
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("ask-result-inline-image").unwrap();
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert_eq!(panel.read_with(vcx, |panel, _| panel.pending_history_image.clone()).as_deref(), Some(comment));
    assert!(matches!(commands.try_recv(), Ok(crate::harness::Command::GetHistoryImage { key, .. }) if key == comment));
}

#[gpui::test]
fn user_image_marker_is_the_only_preview_control(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("inline-image-marker".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![
            Item::User("[#image 1]".into()),
            Item::Image(image_preview::fixture_image()),
        ];
        cx.notify();
    });
    vcx.run_until_parked();
    let thumbnail = vcx.debug_bounds("transcript-image-thumbnail").expect("thumbnail");
    let chip = vcx.debug_bounds("transcript-image-chip").expect("chip");
    let label = vcx.debug_bounds("transcript-image-label").expect("label");
    assert!(thumbnail.right() < label.left());
    assert!(label.right() <= chip.right(), "label must fit: {label:?} {chip:?}");
    let marker = vcx.debug_bounds("prompt-content-0").expect("inline marker");
    vcx.simulate_click(marker.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn clicking_end_of_transcript_image_chip_opens_preview(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-click-edge".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("Look [#image 1] after".into()), Item::Image(image_preview::fixture_image())];
        cx.notify();
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("transcript-image-chip").unwrap();
    vcx.simulate_click(gpui::point(chip.right() - px(1.), chip.center().y), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
async fn transcript_thumbnail_paints_decoded_image_in_the_marker(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("inline-image-painted".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("Look at [#image 1] here".into()), Item::Image(image_preview::fixture_image())];
        cx.notify();
    });
    crate::markdown::take_painted_prompt_thumbnails();
    let mut painted = Vec::new();
    for _ in 0..128 {
        panel.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        painted.extend(crate::markdown::take_painted_prompt_thumbnails());
        if !painted.is_empty() { break; }
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded { std::task::Poll::Ready(()) } else {
                yielded = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }).await;
    }
    let thumbnail = painted.first().expect("decoded thumbnail must paint");
    assert_eq!(thumbnail.size, gpui::size(px(14.), px(14.)));
    let prompt = vcx.debug_bounds("prompt-content-0").unwrap();
    assert!(prompt.contains(&thumbnail.center()), "thumbnail is inside the actual prompt");
}

#[gpui::test]
fn user_image_markers_follow_submission_order_and_turn_boundaries(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| Panel::new("image-order".into(), None, None, crate::harness::spawn_inert(), cx));
    panel.update(cx, |panel, _| {
        *panel.items = vec![
            Item::User("[#image 2] then [#image 1]".into()),
            Item::Image(image_preview::fixture_image()),
            Item::Image(image_preview::fixture_image()),
            Item::User("[#image 1]".into()),
            Item::Image(image_preview::fixture_image()),
        ];
        assert_eq!(panel.user_image_markers(0), vec![(1, "[#image 2]".into()), (2, "[#image 1]".into())]);
        assert_eq!(panel.user_image_markers(3), vec![(4, "[#image 1]".into())]);
        let rows = panel.transcript_render_rows();
        assert_eq!(rows.iter().map(|row| row.index).collect::<Vec<_>>(), vec![0, 3]);
        assert_eq!(panel.session_image_indices(), vec![1, 2, 4]);
    });
}

#[gpui::test]
fn each_inline_marker_opens_its_own_image(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-click-order".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        let mut first = image_preview::fixture_image();
        first.label = Some("first".into());
        let mut second = image_preview::fixture_image();
        second.label = Some("second".into());
        *panel.items = vec![Item::User("[#image 2]\n\n[#image 1]".into()), Item::Image(first), Item::Image(second)];
        cx.notify();
    });
    vcx.run_until_parked();
    let bounds = vcx.debug_bounds("prompt-content-0").unwrap();
    for (y, expected) in [(bounds.top() + px(8.), "first"), (bounds.bottom() - px(8.), "second")] {
        vcx.simulate_click(gpui::point(bounds.center().x, y), Default::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.image_preview.as_ref().and_then(|image| image.label.as_deref()), Some(expected));
        });
        vcx.update(|window, cx| panel.update(cx, |panel, cx| panel.close_image_preview(window, cx)));
        vcx.run_until_parked();
    }
}

#[gpui::test]
fn image_marker_before_markdown_link_opens_image_not_url(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-and-link".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("[#image 1] and [docs](https://example.com)".into()), Item::Image(image_preview::fixture_image())];
        cx.notify();
    });
    vcx.run_until_parked();
    let bounds = vcx.debug_bounds("prompt-content-0").unwrap();
    vcx.simulate_click(gpui::point(bounds.left() + px(15.), bounds.top() + px(8.)), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
    assert_eq!(vcx.opened_url(), None);
}

/// A parenthetical typed right after a pasted image is prose, not a link URL.
#[gpui::test]
fn image_marker_followed_by_parenthesis_is_a_chip_not_a_link(cx: &mut gpui::TestAppContext) {
    for marker in ["[#image 1]", "[#img_64ab0e4a8c467a9f70900b407050b140]"] {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new("image-paren".into(), None, None, crate::harness::spawn_inert(), cx)
        });
        panel.update(vcx, |panel, cx| {
            let mut image = image_preview::fixture_image();
            image.label = Some(marker.into());
            *panel.items = vec![
                Item::User(format!("Look {marker}(сессия и TLDR видео) after")),
                Item::Image(image),
            ];
            cx.notify();
        });
        vcx.run_until_parked();
        let chip = vcx.debug_bounds("transcript-image-chip").unwrap_or_else(|| panic!("{marker} must render as a chip"));
        vcx.simulate_click(chip.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("image-preview-full").is_some(), "{marker} click must preview the image");
        assert_eq!(vcx.opened_url(), None, "{marker} click must not ask the OS to open a URL");
    }
}

#[gpui::test]
fn image_marker_in_code_block_opens_image(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-in-code".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("```\n[#image 1]\n```".into()), Item::Image(image_preview::fixture_image())];
        cx.notify();
    });
    vcx.run_until_parked();
    let code = vcx.debug_bounds("prompt-image-code").expect("code text");
    assert!(vcx.debug_bounds("transcript-user-image-chip").is_none());
    vcx.simulate_click(gpui::point(code.left() + px(20.), code.top() + px(8.)), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn user_image_without_marker_has_compact_fallback_preview(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "user-image-chip".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![
            Item::User("Inspect attachment".into()),
            Item::Image(image_preview::fixture_image()),
        ];
        cx.notify();
    });
    vcx.run_until_parked();
    let chip = vcx
        .debug_bounds("transcript-user-image-chip")
        .expect("compact chip");
    let thumbnail = vcx.debug_bounds("transcript-fallback-thumbnail").expect("fallback thumbnail");
    assert_eq!(thumbnail.size, gpui::size(px(14.), px(14.)));
    assert!(chip.size.height < px(40.));
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

fn image(call: &str, data: &str, boundary: Option<usize>) -> jcode_sdk::RenderedImage {
    jcode_sdk::RenderedImage {
        media_type: "image/png".into(),
        data: data.into(),
        label: Some("chart.png".into()),
        source: jcode_sdk::RenderedImageSource::ToolResult {
            tool_name: "read".into(),
        },
        anchor: Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: call.into() }),
        session_image: None,
        history_message_index: boundary,
    }
}

fn tool(call: &str) -> Item {
    Item::Tool {
        call_id: call.into(),
        name: "read".into(),
        input: String::new(),
        output: "Image loaded".into(),
        done: true,
        error: None,
        ask: None,
    }
}

fn message(role: &str, content: &str) -> jcode_sdk::HistoryMessage {
    jcode_sdk::HistoryMessage {
        attachments: Vec::new(),
        response_stats: None,
        timestamp_ms: None,
        tool: None,
        role: role.into(),
        content: content.into(),
    }
}

#[gpui::test]
fn history_images_stay_between_read_and_response_across_turns(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        w.push_test_panel("image-history", cx);
        w
    });
    let panel = workspace.update(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.load_history(vec![message("user", "Read chart"), message("assistant", "Reading now"),
            message("tool", "read result"), message("assistant", "Chart response"),
            message("user", "Read again"), message("tool", "read result"),
            message("assistant", "Second response")],
            vec![image("first", "same", Some(3)), image("second", "same", Some(6))], cx);
        assert!(matches!(panel.items.as_slice(), [Item::User(_), Item::Assistant(before),
            Item::Image(first), Item::Assistant(after), Item::User(_), Item::Image(second), Item::Assistant(last)]
            if before == "Reading now" && after == "Chart response" && last == "Second response"
                && first.anchor != second.anchor));
        for item in &panel.items {
            if let Item::Image(image) = item {
                assert_eq!(image.model_input_caption(), Some("Image provided to model"));
            }
        }
    });
}

#[gpui::test]
fn live_images_keep_batch_order_and_repeated_reads_but_deduplicate_replay(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        w.push_test_panel("images-live", cx);
        w
    });
    let panel = workspace.update(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |panel, _| {
        *panel.items = vec![tool("first"), Item::Assistant("After first read".into()), tool("second"), Item::User("pending".into())];
        panel.pending_users = [3].into();
        panel.accepted_users.insert(3, Instant::now());
        for _ in 0..2 {
            panel.insert_rendered_image(image("first", "one", None));
            panel.insert_rendered_image(image("first", "two", None));
            panel.insert_rendered_image(image("second", "one", None));
        }
        assert!(matches!(panel.items.as_slice(), [Item::Tool {..}, Item::Image(first), Item::Image(second),
            Item::Assistant(_), Item::Tool {..}, Item::Image(repeated), Item::User(_)]
            if first.data == "one" && second.data == "two" && repeated.data == "one"));
        assert_eq!(panel.pending_users.front(), Some(&6));
        assert!(panel.accepted_users.contains_key(&6));
    });
}

#[gpui::test]
fn history_image_boundaries_include_start_end_and_hidden_rows(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        w.push_test_panel("images-boundaries", cx);
        w
    });
    let panel = workspace.update(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.load_history(vec![message("assistant", "First"), message("tool", "hidden"), message("assistant", "Last")],
            vec![image("start", "start", Some(0)), image("middle", "middle", Some(2)), image("end", "end", Some(3))], cx);
        assert!(matches!(panel.items.as_slice(), [Item::Image(a), Item::Assistant(_), Item::Image(b),
            Item::Assistant(_), Item::Image(c)] if a.data == "start" && b.data == "middle" && c.data == "end"));
    });
}

#[test]
fn only_tool_image_payloads_claim_model_input() {
    let pasted = TranscriptImage::new("image/png".into(), "bytes".into(), None);
    assert_eq!(pasted.model_input_caption(), None);
    let from_tool = TranscriptImage::from_rendered(image("read", "bytes", None));
    assert_eq!(
        from_tool.model_input_caption(),
        Some("Image provided to model")
    );
    assert_eq!(from_tool.label.as_deref(), Some("chart.png"));
}
