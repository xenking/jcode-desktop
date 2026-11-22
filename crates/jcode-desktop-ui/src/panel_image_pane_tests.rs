//! Image-pane contracts. Register this module alongside panel_image_transcript_tests.
use super::*;

const DRAFT: &str = "Keep my unfinished image question";

#[gpui::test]
fn image_pane_preloads_the_selection_and_scrolled_thumbnails(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-preload".into(), None, None, bridge, cx)
    });
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(1000.), px(700.)));
    panel.update(vcx, |panel, cx| {
        let markers: String = (1..=96).map(|number| format!(" [#image {number}]")).collect();
        *panel.items = std::iter::once(Item::User(format!("Inspect these images{markers}")))
            .chain((0..96).map(|number| {
                Item::Image(TranscriptImage::new(
                    "image/png".into(),
                    format!("jcode-lazy:preload-{number}"),
                    None,
                ))
            }))
            .collect();
        panel.set_image_pane_open(true, cx);
    });
    vcx.run_until_parked();

    let read_requests = || {
        commands
            .try_iter()
            .filter_map(|command| match command {
                Command::GetHistoryImage { session_id, key } => {
                    assert_eq!(session_id, "image-preload");
                    Some(key)
                }
                Command::Send { .. } => panic!("gallery must not send a prompt"),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let initial = read_requests();
    assert!(initial.iter().any(|key| key == "preload-95"), "{initial:?}");
    assert!(initial.iter().any(|key| key != "preload-95"), "{initial:?}");
    assert!(initial.len() <= 12, "{initial:?}");
    assert!(!initial.iter().any(|key| key == "preload-45"), "{initial:?}");

    let png = image_preview::fixture_image().data;
    panel.update(vcx, |panel, cx| {
        for key in initial {
            panel.load_history_image(key, Ok(png.clone()), cx);
        }
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-image-preview-ready").is_some());
    assert!(vcx.debug_bounds("image-preview").is_none());

    panel.update(vcx, |panel, cx| {
        panel.image_pane_scroll.scroll_to_item(45);
        cx.notify();
    });
    vcx.run_until_parked();
    let scrolled = read_requests();
    assert!(scrolled.iter().any(|key| key == "preload-45"), "{scrolled:?}");
    assert!(scrolled.len() <= 12, "{scrolled:?}");
    assert!(vcx.debug_bounds("image-preview").is_none());
}

#[gpui::test]
fn image_pane_late_reply_cannot_replace_the_other_sessions_preview(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("gallery-source", cx);
        workspace.push_test_panel("gallery-other", cx);
        workspace
    });
    let (source, other) = workspace.read_with(vcx, |workspace, _| {
        (workspace.test_panel(0).unwrap(), workspace.test_panel(1).unwrap())
    });
    let key = "img_0123456789abcdef0123456789abcdef";
    let lazy = TranscriptImage::new("image/png".into(), format!("jcode-lazy:{key}"), None);
    for panel in [&source, &other] {
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Image(lazy.clone())];
            panel.request_history_thumbnail(&lazy, false);
            panel.set_image_pane_open(true, cx);
        });
    }
    vcx.update(|window, cx| {
        source.update(cx, |panel, cx| {
            panel.open_image_preview(lazy.clone(), window, cx);
            panel.set_image_pane_open(false, cx);
        });
    });

    let deliver = |id: &str, result, vcx: &mut gpui::VisualTestContext| {
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::HistoryImage {
                    session_id: id.into(),
                    key: key.into(),
                    result,
                },
                cx,
            );
        });
        vcx.run_until_parked();
    };
    deliver("gallery-source", Ok(image_preview::fixture_image().data), vcx);
    source.read_with(vcx, |panel, _| {
        assert!(matches!(&panel.items[0], Item::Image(image) if image.preview.is_some()));
        assert!(panel.image_preview.is_none());
    });
    other.read_with(vcx, |panel, _| {
        assert!(matches!(&panel.items[0], Item::Image(image) if image.preview.is_none()));
    });

    deliver("gallery-other", Err("file no longer available".into()), vcx);
    assert!(other.read_with(vcx, |panel, _| panel.gallery_image_unavailable(&lazy)));
    assert!(vcx.debug_bounds("session-image-unavailable").is_some());
    assert!(vcx.debug_bounds("image-preview").is_none());
}

fn setup(cx: &mut gpui::TestAppContext) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
    cx.update(crate::input::bind_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("image-pane-session", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![
            Item::User("Compare these images".into()),
            Item::Image(fixture("first.png")),
            Item::Assistant("Another image follows".into()),
            Item::Image(fixture("second.png")),
        ];
        panel
            .input
            .update(cx, |input, cx| input.set_content(DRAFT.into(), cx));
        cx.notify();
    });
    vcx.run_until_parked();
    (panel, vcx)
}

fn fixture(label: &str) -> TranscriptImage {
    let mut image = image_preview::fixture_image();
    image.label = Some(label.into());
    image.source = jcode_sdk::RenderedImageSource::ToolResult {
        tool_name: "read".into(),
    };
    image
}

fn incoming(call: &str) -> jcode_sdk::RenderedImage {
    let image = fixture(call);
    jcode_sdk::RenderedImage {
        media_type: image.media_type,
        data: image.data,
        label: image.label,
        source: jcode_sdk::RenderedImageSource::ToolResult {
            tool_name: "read".into(),
        },
        anchor: Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: call.into() }),
        session_image: None,
        history_message_index: None,
    }
}

fn click(vcx: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = vcx.debug_bounds(selector).expect(selector);
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
}

fn assert_draft(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) {
    assert_eq!(
        panel.read_with(vcx, |panel, cx| panel.input.read(cx).snapshot().content),
        DRAFT
    );
}

#[gpui::test]
fn session_image_marker_labels_the_user_chip(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    let marker = "[#img_0123456789abcdef0123456789abcdef]";
    panel.update(vcx, |panel, cx| {
        let mut image = image_preview::fixture_image();
        image.source = jcode_sdk::RenderedImageSource::UserInput;
        *panel.items = vec![Item::User(format!("Inspect {marker}(then continue)")), Item::Image(image)];
        assert_eq!(panel.user_image_markers(0), vec![(1, marker.to_string())]);
        cx.notify();
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("transcript-image-chip").expect("session ID chip");
    assert!(chip.size.width < px(100.), "storage ID reserved excess inline space: {chip:?}");
    let label = vcx.debug_bounds("transcript-image-label").expect("label");
    assert!(chip.right() - label.right() <= px(16.), "chip leaves a gap after its label: {chip:?} {label:?}");
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn image_pane_button_hides_when_the_last_image_is_removed(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    assert!(vcx.debug_bounds("panel-images").is_some());
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::User("No pictures here".into())];
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-images").is_none());
    assert!(vcx.debug_bounds("panel-status-badge").is_none());
}

#[gpui::test]
fn image_pane_button_toggles_links_and_close_restores_inline(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        assert!(!panel.image_pane_open);
        assert_eq!(panel.image_pane_selected, None);
        assert_eq!(panel.session_image_indices(), vec![1, 3]);
        assert!(panel.render_image_pane(cx).is_none());
    });
    assert!(vcx.debug_bounds("transcript-image").is_some());
    assert!(vcx.debug_bounds("session-image-pane").is_none());

    click(vcx, "panel-images");
    assert!(panel.read_with(vcx, |panel, _| panel.image_pane_open));
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    assert!(vcx.debug_bounds("session-image-main").is_some());
    assert!(vcx.debug_bounds("transcript-image").is_none());
    assert!(vcx.debug_bounds("transcript-image-link-1").is_some());
    assert!(vcx.debug_bounds("transcript-image-link-3").is_some());
    assert_draft(&panel, vcx);

    click(vcx, "panel-images");
    assert!(!panel.read_with(vcx, |panel, _| panel.image_pane_open));
    assert!(vcx.debug_bounds("session-image-pane").is_none());
    assert!(vcx.debug_bounds("transcript-image").is_some());

    click(vcx, "panel-images");
    click(vcx, "session-image-close");
    assert!(!panel.read_with(vcx, |panel, _| panel.image_pane_open));
    assert!(vcx.debug_bounds("session-image-pane").is_none());
    assert!(vcx.debug_bounds("transcript-image-link-1").is_none());
    assert!(vcx.debug_bounds("transcript-image").is_some());
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_thumbnails_select_item_indices_and_main_opens_lightbox(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    click(vcx, "panel-images");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.selected_session_image_index()),
        Some(3)
    );
    click(vcx, "session-image-thumb-1");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.image_pane_selected),
        Some(1)
    );
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.selected_session_image_index()),
        Some(1)
    );
    click(vcx, "session-image-main");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel
            .image_preview
            .as_ref()
            .and_then(|image| image.label.clone())),
        Some("first.png".into())
    );
    assert!(vcx.debug_bounds("image-preview-full").is_some());
    vcx.simulate_keystrokes("x");
    assert_draft(&panel, vcx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_none()));
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    click(vcx, "session-image-thumb-3");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.image_pane_selected),
        Some(3)
    );
    click(vcx, "session-image-main");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel
            .image_preview
            .as_ref()
            .and_then(|image| image.label.clone())),
        Some("second.png".into())
    );
    click(vcx, "image-preview-close");
    click(vcx, "session-image-close");
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_letterbox_does_not_open_the_preview(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    click(vcx, "panel-images");
    let bounds = vcx.debug_bounds("session-image-main").unwrap();
    let at = gpui::point(bounds.left() + px(1.), bounds.top() + px(1.));
    vcx.simulate_click(at, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("image-preview-full").is_none());
    click(vcx, "session-image-main");
    assert!(vcx.debug_bounds("image-preview-full").is_some());
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_arrows_and_escape_only_capture_when_the_filmstrip_has_focus(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    vcx.update(|window, cx| {
        let handle = panel.read(cx).input_focus_handle(cx);
        window.focus(&handle, cx);
    });
    click(vcx, "panel-images");
    vcx.simulate_keystrokes("left");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).snapshot().selection_start),
        DRAFT.len() - 1
    );
    assert_eq!(
        panel.read_with(vcx, |p, _| p.selected_session_image_index()),
        Some(3)
    );
    vcx.simulate_keystrokes("escape");
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    assert!(panel.read_with(vcx, |p, cx| p.input.read(cx).snapshot().content.is_empty()));
    let input = panel.read_with(vcx, |p, _| p.input.clone());
    input.update(vcx, |input, cx| input.set_content(DRAFT.into(), cx));

    click(vcx, "session-image-thumb-1");
    assert!(vcx.update(|window, cx| panel.read(cx).image_pane_focus.is_focused(window)));
    vcx.simulate_keystrokes("right");
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, _| p.selected_session_image_index()),
        Some(3)
    );
    vcx.simulate_keystrokes("left left");
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, _| p.selected_session_image_index()),
        Some(1)
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-image-pane").is_none());
    assert!(vcx.update(|window, cx| panel.read(cx).input_focus_handle(cx).is_focused(window)));
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_captures_wheel_and_precise_swipes_at_both_ends(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("many-images", cx);
        workspace.push_test_panel("another-session", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |w, _| w.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        let image = fixture("frame.png");
        *panel.items = (0..96).map(|_| Item::Image(image.clone())).collect();
        cx.notify();
    });
    vcx.run_until_parked();
    click(vcx, "panel-images");
    let at = vcx.debug_bounds("session-image-thumbnails").unwrap().center();
    let (first, max) = panel.read_with(vcx, |p, _| {
        (p.image_pane_scroll.offset().x, p.image_pane_scroll.max_offset().x)
    });
    let focus = workspace.read_with(vcx, |w, _| w.test_focus_position());
    let transcript = panel.read_with(vcx, |p, _| p.test_scroll_offset_y());
    let panel_origin = vcx.debug_bounds("panel-0").unwrap().origin;
    assert!(max > px(1_000.));

    let mut wheel = |delta, touch_phase| {
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: at,
            delta,
            touch_phase,
            modifiers: gpui::Modifiers::default(),
        });
        vcx.run_until_parked();
        panel.read_with(vcx, |p, _| p.image_pane_scroll.offset().x)
    };
    use gpui::{ScrollDelta, TouchPhase};
    let lines = if first == px(0.) { -4. } else { 4. };
    assert_ne!(
        wheel(ScrollDelta::Lines(gpui::point(0., lines)), TouchPhase::Moved),
        first
    );
    for (delta, edge) in [(-10_000., -max), (10_000., px(0.))] {
        let payload = ScrollDelta::Pixels(gpui::point(px(delta), px(delta / 2.)));
        assert_eq!(wheel(payload, TouchPhase::Started), edge);
        assert_eq!(wheel(payload, TouchPhase::Moved), edge);
        assert_eq!(wheel(payload, TouchPhase::Ended), edge);
    }
    assert_eq!(workspace.read_with(vcx, |w, _| w.test_focus_position()), focus);
    assert_eq!(panel.read_with(vcx, |p, _| p.test_scroll_offset_y()), transcript);
    assert_eq!(vcx.debug_bounds("panel-0").unwrap().origin, panel_origin);
}

#[gpui::test]
fn image_pane_leaves_room_for_the_picture_with_a_full_metadata_list(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(480.), px(540.)));
    panel.update(vcx, |panel, cx| {
        let rows: Vec<_> = (0..96)
            .map(|index| {
                serde_json::json!({
                    "image_id": format!("img_{index:032x}"),
                    "message_index": index,
                    "media_type": "image/png",
                })
            })
            .collect();
        panel.session_image_gallery.receive(Ok(serde_json::to_string(&rows).unwrap()));
        cx.notify();
    });
    vcx.run_until_parked();
    click(vcx, "panel-images");
    let picture = vcx.debug_bounds("session-image-picture").unwrap();
    let strip = vcx.debug_bounds("session-image-thumbnails").unwrap();
    let pane = vcx.debug_bounds("session-image-pane").unwrap();
    assert!(picture.size.height > px(24.), "{picture:?} within {pane:?}");
    assert!(strip.bottom() <= pane.bottom(), "{strip:?} within {pane:?}");
    click(vcx, "session-image-picture");
    assert!(vcx.debug_bounds("image-preview-full").is_some());
}

#[gpui::test]
fn image_pane_follows_incoming_images_only_without_explicit_selection(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    click(vcx, "panel-images");
    panel.update(vcx, |panel, cx| {
        panel.insert_rendered_image(incoming("third.png"));
        assert_eq!(panel.image_pane_selected, None);
        assert_eq!(panel.session_image_indices(), vec![1, 3, 4]);
        assert_eq!(panel.selected_session_image_index(), Some(4));
        cx.notify();
    });
    vcx.run_until_parked();
    click(vcx, "session-image-main");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel
            .image_preview
            .as_ref()
            .and_then(|image| image.label.clone())),
        Some("third.png".into())
    );
    click(vcx, "image-preview-close");
    click(vcx, "session-image-thumb-1");
    panel.update(vcx, |panel, cx| {
        panel.insert_rendered_image(incoming("fourth.png"));
        assert_eq!(panel.image_pane_selected, Some(1));
        assert_eq!(panel.selected_session_image_index(), Some(1));
        cx.notify();
    });
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_selection_tracks_image_when_earlier_tool_result_arrives(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        panel.items[0] = Item::Tool {
            call_id: "early-read".into(),
            name: "read".into(),
            input: String::new(),
            output: "Image loaded".into(),
            done: true,
            error: None,
        };
        panel.set_image_pane_open(true, cx);
        panel.image_pane_selected = Some(3);
        panel.insert_rendered_image(incoming("early-read"));
        assert_eq!(panel.session_image_indices(), vec![1, 2, 4]);
        assert_eq!(panel.image_pane_selected, Some(4));
        assert_eq!(panel.selected_session_image_index(), Some(4));
        assert!(matches!(&panel.items[4], Item::Image(image) if image.label.as_deref() == Some("second.png")));
        // Replayed results do not shift the selection a second time.
        panel.insert_rendered_image(incoming("early-read"));
        assert_eq!(panel.image_pane_selected, Some(4));
        assert_eq!(panel.session_image_indices(), vec![1, 2, 4]);
    });
}

#[gpui::test]
fn image_pane_state_is_independent_between_session_panels(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("images-a", cx);
        workspace.push_test_panel("images-b", cx);
        workspace
    });
    let first = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    let second = workspace.update(vcx, |workspace, _| workspace.test_panel(1).unwrap());
    for panel in [&first, &second] {
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Image(fixture("one")), Item::Image(fixture("two"))];
            cx.notify();
        });
    }
    first.update(vcx, |panel, cx| {
        panel.set_image_pane_open(true, cx);
        panel.image_pane_selected = Some(0);
    });
    second.update(vcx, |panel, cx| {
        assert!(!panel.image_pane_open);
        assert_eq!(panel.image_pane_selected, None);
        assert!(panel.render_image_pane(cx).is_none());
        panel.set_image_pane_open(true, cx);
        assert_eq!(panel.selected_session_image_index(), Some(1));
    });
    first.update(vcx, |panel, cx| {
        assert_eq!(panel.image_pane_selected, Some(0));
        panel.set_image_pane_open(false, cx);
    });
    second.update(vcx, |panel, cx| {
        assert!(panel.image_pane_open);
        assert!(panel.render_image_pane(cx).is_some());
        assert_eq!(panel.image_pane_selected, None);
    });
}

#[gpui::test]
fn image_pane_snapshot_roundtrip_preserves_open_and_draft_but_follows_latest(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        panel.set_image_pane_open(true, cx);
        panel.image_pane_selected = Some(1);
        let json = serde_json::to_value(panel.snapshot(cx)).unwrap();
        assert_eq!(json["image_pane_open"], true);
        // Item indices are not stable across history reconstruction.
        assert!(json.get("image_pane_selected").is_none());
        let snapshot: PanelSnapshot = serde_json::from_value(json).unwrap();
        assert!(snapshot.image_pane_open);
        panel.set_image_pane_open(false, cx);
        panel
            .input
            .update(cx, |input, cx| input.set_content("changed".into(), cx));
        panel.restore_snapshot(snapshot, cx);
        assert!(panel.image_pane_open);
        assert_eq!(panel.image_pane_selected, None);
        assert_eq!(panel.selected_session_image_index(), Some(3));
    });
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_legacy_snapshot_defaults_closed(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        let mut json = serde_json::to_value(panel.snapshot(cx)).unwrap();
        json.as_object_mut().unwrap().remove("image_pane_open");
        json.as_object_mut().unwrap().remove("image_pane_selected");
        let legacy: PanelSnapshot = serde_json::from_value(json).unwrap();
        assert!(!legacy.image_pane_open);
        panel.set_image_pane_open(true, cx);
        panel.image_pane_selected = Some(1);
        panel.restore_snapshot(legacy, cx);
        assert!(!panel.image_pane_open);
        assert_eq!(panel.image_pane_selected, None);
        assert!(panel.render_image_pane(cx).is_none());
    });
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_follow_latest_button_resumes_after_manual_selection(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    click(vcx, "panel-images");
    click(vcx, "session-image-thumb-1");
    click(vcx, "session-image-latest");
    panel.update(vcx, |panel, cx| {
        assert_eq!(panel.image_pane_selected, None);
        assert_eq!(panel.selected_session_image_index(), Some(3));
        panel.insert_rendered_image(incoming("resumed.png"));
        assert_eq!(panel.selected_session_image_index(), Some(4));
        cx.notify();
    });
    vcx.run_until_parked();
    click(vcx, "session-image-main");
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel
            .image_preview
            .as_ref()
            .and_then(|image| image.label.clone())),
        Some("resumed.png".into())
    );
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_empty_session_accepts_first_image(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        assert!(panel.session_image_indices().is_empty());
        assert_eq!(panel.selected_session_image_index(), None);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-images").is_none());
    panel.update(vcx, |panel, cx| panel.set_image_pane_open(true, cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-images").is_none());
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    assert!(vcx.debug_bounds("session-image-main").is_none());
    panel.update(vcx, |panel, cx| {
        panel.insert_rendered_image(incoming("first-in-empty.png"));
        assert_eq!(panel.selected_session_image_index(), Some(0));
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("session-image-main").is_some());
    assert!(vcx.debug_bounds("session-image-thumb-0").is_some());
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_missing_preview_does_not_open_lightbox(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = setup(cx);
    panel.update(vcx, |panel, cx| {
        let mut image = fixture("unavailable.png");
        image.preview = None;
        *panel.items = vec![Item::Image(image)];
        cx.notify();
    });
    vcx.run_until_parked();
    click(vcx, "panel-images");
    click(vcx, "session-image-thumb-0");
    click(vcx, "session-image-main");
    assert!(panel.read_with(vcx, |panel, _| panel.image_preview.is_none()));
    assert!(vcx.debug_bounds("image-preview").is_none());
    assert!(vcx.debug_bounds("session-image-pane").is_some());
    assert_draft(&panel, vcx);
}

#[gpui::test]
fn image_pane_narrow_and_wide_geometry_keeps_toggle_and_composer_visible(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new("image-pane-responsive".into(), None, None, bridge, cx)
    });
    panel.update(vcx, |panel, cx| {
        *panel.items = vec![Item::Image(fixture("responsive.png"))];
        panel
            .input
            .update(cx, |input, cx| input.set_content(DRAFT.into(), cx));
        cx.notify();
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [480., 1000., 480.] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(800.)));
        panel.update(vcx, |panel, cx| panel.set_image_pane_open(false, cx));
        vcx.run_until_parked();
        click(vcx, "panel-images");
        // The layout observer schedules the responsive orientation update.
        panel.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        for selector in [
            "panel-images",
            "prompt-input",
            "session-image-pane",
            "session-image-main",
            "session-image-close",
        ] {
            let bounds = vcx.debug_bounds(selector).expect(selector);
            assert!(
                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                "{selector} at width {width}: {bounds:?}"
            );
            assert!(
                bounds.left() >= px(0.) && bounds.right() <= px(width + 1.),
                "{selector} at width {width}: {bounds:?}"
            );
            assert!(
                bounds.top() >= px(0.) && bounds.bottom() <= px(801.),
                "{selector} at width {width}: {bounds:?}"
            );
        }
        let transcript = vcx.debug_bounds("transcript").unwrap();
        let gallery = vcx.debug_bounds("session-image-pane").unwrap();
        if width < 600. {
            assert!(
                gallery.top() >= transcript.bottom(),
                "narrow pane should stack below chat: {gallery:?} / {transcript:?}"
            );
        } else {
            assert!(
                gallery.left() >= transcript.right(),
                "wide pane should sit beside chat: {gallery:?} / {transcript:?}"
            );
        }
        assert_draft(&panel, vcx);
        click(vcx, "session-image-close");
        assert!(vcx.debug_bounds("transcript-image").is_some());
    }
}
