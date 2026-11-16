use super::*;

#[gpui::test]
fn completed_ask_replaces_inline_marker_with_image_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("ask-inline-image".into(), None, None, crate::harness::spawn_inert(), cx)
    });
    panel.update(vcx, |panel, cx| {
        let mut image = image_preview::fixture_image();
        image.source = jcode_sdk::RenderedImageSource::ToolResult { tool_name: "ask".into() };
        image.anchor = Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: "ask-inline".into() });
        *panel.items = vec![Item::Tool {
            call_id: "ask-inline".into(), name: "ask".into(), input: "{}".into(),
            output: "User answers:\n1. Which? -> Look [#image 1] here".into(), done: true, error: None,
        }, Item::Image(image)];
        cx.notify();
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("ask-result-inline-image").expect("marker must render inline as a chip");
    assert!(vcx.debug_bounds("ask-result-inline-thumbnail").is_some());
    assert!(vcx.debug_bounds("ask-result-inline-label").is_some());
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
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
    }
}

fn message(role: &str, content: &str) -> jcode_sdk::HistoryMessage {
    jcode_sdk::HistoryMessage {
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
