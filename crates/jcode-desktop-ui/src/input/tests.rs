use super::*;
use gpui::{TestAppContext, WindowOptions};

#[test]
fn large_paste_threshold_matches_omp() {
    assert!(!is_large_paste(&"a".repeat(1000)));
    assert!(is_large_paste(&"a".repeat(1001)));
    assert!(!is_large_paste(&"a\n".repeat(9)));
    assert!(is_large_paste(&"a\n".repeat(10)));
    assert!(is_large_paste(&"😀".repeat(501)));
}

#[gpui::test]
fn large_paste_and_match_style_renders_a_chip(cx: &mut TestAppContext) {
    use crate::clipboard_image::TestPasteReader;

    cx.update(|cx| {
        bind_keys(cx);
        cx.set_global(TestPasteReader(|| {
            panic!("plain-text paste must not read media")
        }));
        cx.write_to_clipboard(ClipboardItem::new_string("line\n".repeat(12)));
    });
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::new(cx, "test", |_, _, _, _| {});
        window.focus(&input.focus_handle, cx);
        input
    });
    for shortcut in ["cmd-shift-v", "cmd-alt-shift-v"] {
        vcx.simulate_keystrokes(shortcut);
        assert!(vcx.debug_bounds("composer-image-chip").is_some());
        vcx.simulate_keystrokes("cmd-z");
        assert!(input.read_with(vcx, |input, _| input.content.is_empty()));
    }
    let short = "a".repeat(999);
    vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(short.clone())));
    vcx.simulate_keystrokes("cmd-shift-v");
    assert!(vcx.debug_bounds("composer-image-chip").is_none());
    assert_eq!(
        input.read_with(vcx, |input, _| input.snapshot().content),
        short
    );
}

#[gpui::test]
fn large_native_text_commit_renders_a_chip_at_its_replacement_range(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.set_content("😀replace!".into(), cx);
        window.focus(&input.focus_handle, cx);
        input
    });
    let text = "line\n".repeat(12);
    input.update_in(vcx, |input, window, cx| {
        EntityInputHandler::replace_text_in_range(input, Some(2..9), &text, window, cx);
    });
    vcx.run_until_parked();
    let chip = vcx
        .debug_bounds("composer-image-chip")
        .expect("pasted text chip");
    vcx.simulate_click(chip.center(), Default::default());
    assert_eq!(
        input.read_with(vcx, |input, _| input.snapshot().content),
        format!("😀{text}!")
    );
}

#[gpui::test]
fn long_marked_ime_commit_stays_editable(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    let text = "你好".repeat(501);
    input.update_in(vcx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        EntityInputHandler::replace_text_in_range(input, None, &text, window, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-image-chip").is_none());
    assert_eq!(
        input.read_with(vcx, |input, _| input.snapshot().content),
        text
    );
}

#[gpui::test]
fn inline_only_text_field_keeps_long_paste_literal(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::embedded_editor(cx, "Search").text_only();
        window.focus(&input.focus_handle, cx);
        input
    });
    let text = "a".repeat(1001);
    vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())));
    vcx.simulate_keystrokes("cmd-v");
    assert!(vcx.debug_bounds("composer-image-chip").is_none());
    assert_eq!(
        input.read_with(vcx, |input, _| input.snapshot().content),
        text
    );
}

#[gpui::test]
fn long_paste_falls_back_to_text_and_sends_it_in_full(cx: &mut TestAppContext) {
    use crate::clipboard_image::TestPasteReader;

    let sent = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
    let result = sent.clone();
    let text = "a line\n".repeat(12);
    cx.update(|cx| {
        bind_keys(cx);
        cx.set_global(TestPasteReader(|| Err("image flavor unavailable".into())));
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    });
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::new(cx, "test", move |text, _, _, _| {
            *result.borrow_mut() = Some(text)
        });
        window.focus(&input.focus_handle, cx);
        input
    });
    vcx.simulate_keystrokes("cmd-v");
    assert!(vcx.debug_bounds("composer-image-chip").is_some());
    input.update(vcx, |input, cx| {
        let snapshot = input.snapshot();
        input.restore(snapshot, cx);
        assert_eq!(input.content.as_ref(), "[Pasted text, 13 lines]");
    });
    vcx.simulate_keystrokes("enter");
    assert_eq!(sent.borrow().as_deref(), Some(text.as_str()));
}

#[gpui::test]
fn clicking_pasted_text_chip_expands_the_original_text(cx: &mut TestAppContext) {
    let text = "line\n".repeat(12);
    let (input, vcx) = cx.add_window_view(|_, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.insert_pasted_text(&text, cx);
        input
    });
    vcx.run_until_parked();
    let chip = vcx
        .debug_bounds("composer-image-chip")
        .expect("pasted text chip");
    vcx.simulate_click(chip.center(), Default::default());
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), text);
        assert!(input.attachments.is_empty());
    });
}

#[gpui::test]
fn pasted_file_chip_sends_only_image_and_no_local_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("real picture.png");
    let bytes = include_bytes!("../../../../assets/previews/image-preview.png");
    std::fs::write(&path, bytes).unwrap();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
    let result = sent.clone();
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", move |text, images, _, _| {
            *result.borrow_mut() = Some((text, images))
        });
        input.attach_pasted_files(std::slice::from_ref(&path), window, cx);
        input
    });
    input.update(vcx, |input, cx| {
        assert!(input.content.contains("real picture.png"));
        assert!(input.content.contains(&format!("{} B", bytes.len())));
        let snapshot = input.snapshot();
        input.restore(snapshot, cx);
    });
    input.update_in(vcx, |input, window, cx| {
        input.dispatch_submission(
            SubmissionKind::Message,
            input.content.to_string(),
            false,
            window,
            cx,
        )
    });
    let sent = sent.borrow();
    let (text, images) = sent.as_ref().unwrap();
    assert_eq!(text.trim(), "[#image 1]");
    assert_eq!(images.len(), 1);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(&images[0].1)
            .unwrap(),
        bytes
    );
}

#[gpui::test]
fn recalled_file_keeps_captured_bytes(cx: &mut TestAppContext) {
    let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
    let result = sent.clone();
    let file = SubmittedFile {
        name: "notes.txt".into(),
        mime: "text/plain".into(),
        size: 8,
        data: base64::engine::general_purpose::STANDARD.encode(b"original"),
    };
    let expected = file.clone();
    let (input, vcx) = cx.add_window_view(|_, cx| {
        let mut input =
            PromptInput::new_with_queue_files(cx, "test", move |_, text, _, files, _, _, _| {
                *result.borrow_mut() = Some((text, files))
            });
        input.recall_prompt("/tmp/notes.txt", Vec::new(), cx);
        input.append_queued_files(vec![file], cx);
        input
    });
    input.update_in(vcx, |input, window, cx| {
        input.dispatch_submission(
            SubmissionKind::Message,
            input.content.to_string(),
            false,
            window,
            cx,
        )
    });
    let files = sent.borrow();
    let (text, files) = files.as_ref().unwrap();
    assert_eq!(text.trim(), "/tmp/notes.txt");
    let actual = &files[0];
    assert_eq!(actual.name, expected.name);
    assert_eq!(actual.data, expected.data);
}

/// Pasting a file from the file manager points at it. A zip, a folder or
/// a 3 GB dump is named by path and never read or uploaded.
#[gpui::test]
fn pasted_non_image_files_are_referenced_by_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("build output.zip");
    let file = std::fs::File::create(&archive).unwrap();
    file.set_len(11 * 1024 * 1024).unwrap();
    let folder = dir.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, b"original").unwrap();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
    let result = sent.clone();
    let pasted = [archive.clone(), folder.clone(), notes.clone()];
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new_with_queue_files(
            cx,
            "test",
            move |_, text, images, files, _, _, _| {
                *result.borrow_mut() = Some((text, images, files))
            },
        );
        input.attach_pasted_files(&pasted, window, cx);
        input
    });
    input.read_with(vcx, |input, _| {
        assert!(input.attachments.is_empty(), "no file may be read");
        assert_eq!(
            input.attachment_notice.as_deref(),
            Some("3 file paths added"),
            "a size or directory refusal must not appear"
        );
    });
    input.update_in(vcx, |input, window, cx| {
        input.dispatch_submission(
            SubmissionKind::Message,
            input.content.to_string(),
            false,
            window,
            cx,
        )
    });
    let sent = sent.borrow();
    let (text, images, files) = sent.as_ref().unwrap();
    assert_eq!(
        text.trim(),
        format!(
            "\"{}\" {} {}",
            archive.display(),
            folder.display(),
            notes.display()
        )
    );
    assert!(images.is_empty() && files.is_empty(), "nothing is uploaded");
}

#[gpui::test]
fn pasted_file_and_clipboard_images_keep_their_submission_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("real picture.png");
    let bytes = include_bytes!("../../../../assets/previews/image-preview.png");
    std::fs::write(&path, bytes).unwrap();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
    let result = sent.clone();
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", move |text, images, _, _| {
            *result.borrow_mut() = Some((text, images))
        });
        input.attach_pasted_files(std::slice::from_ref(&path), window, cx);
        input.attach_image(
            crate::clipboard_image::from_bytes("image/png".into(), bytes.to_vec()).unwrap(),
            cx,
        );
        input
    });
    input.update_in(vcx, |input, window, cx| {
        input.dispatch_submission(
            SubmissionKind::Message,
            input.content.to_string(),
            false,
            window,
            cx,
        )
    });
    let sent = sent.borrow();
    let (text, images) = sent.as_ref().unwrap();
    assert!(text.contains("[#image 1]"));
    assert!(text.contains("[#image 2]"));
    assert_eq!(images.len(), 2);
}

#[test]
fn session_image_marker_is_a_chip() {
    let text = "before [#img_0123456789abcdef0123456789abcdef] after [#image 1]";
    let markers: Vec<_> = image_marker_ranges(text)
        .into_iter()
        .map(|range| &text[range])
        .collect();
    assert_eq!(
        markers,
        vec!["[#img_0123456789abcdef0123456789abcdef]", "[#image 1]"]
    );
}

#[gpui::test]
fn image_paste_is_an_inline_chip_at_the_cursor_and_backspace_removes_it(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.set_content("before after".into(), cx);
        input.sel.range = 7..7;
        input.focus_handle.focus(window, cx);
        input
    });
    input.update(vcx, |input, cx| {
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        assert_eq!(input.content.as_ref(), "before [#image 1]after");
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-attachments").is_none());
    let chip = vcx.debug_bounds("composer-image-chip").expect("chip");
    let label = vcx.debug_bounds("composer-image-label").expect("label");
    let thumb = vcx
        .debug_bounds("composer-image-thumbnail")
        .expect("thumbnail");
    assert!(
        thumb.right() < label.left(),
        "thumbnail and label have a gap"
    );
    assert!(
        label.right() <= chip.right(),
        "label must fit inside chip: {label:?} {chip:?}"
    );
    vcx.simulate_keystrokes("backspace");
    vcx.run_until_parked();
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "before after");
        assert!(input.attachments.is_empty());
    });
}

#[gpui::test]
fn composer_thumbnail_opens_its_image(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.set_content("before ".into(), cx);
        input.sel.range = 7..7;
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 1000,
                height: 600,
            },
            cx,
        );
        input.focus_handle.focus(window, cx);
        input
    });
    vcx.run_until_parked();
    let thumbnail = vcx
        .debug_bounds("composer-image-thumbnail")
        .expect("thumbnail is painted");
    assert_eq!(thumbnail.size, size(px(14.), px(14.)));
    assert!(
        vcx.debug_bounds("prompt-editor")
            .unwrap()
            .contains(&thumbnail.center())
    );
    vcx.simulate_click(thumbnail.center(), Default::default());
    vcx.run_until_parked();
    assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_some()));
    assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
}

#[gpui::test]
fn clicking_end_of_composer_chip_opens_preview(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(vcx, |input, cx| {
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        )
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("composer-image-chip").unwrap();
    vcx.simulate_click(
        point(chip.right() - px(1.), chip.center().y),
        Default::default(),
    );
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
}

#[gpui::test]
fn image_chip_opens_from_tab_and_escape_returns_to_editor(cx: &mut TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        input.focus_handle.focus(window, cx);
        input
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.focus_next(cx));
    vcx.run_until_parked();
    assert!(!vcx.update(|window, cx| input.read(cx).focus_handle.is_focused(window)));
    let keystroke = gpui::Keystroke::parse("space").unwrap();
    vcx.simulate_event(gpui::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    vcx.simulate_event(gpui::KeyUpEvent { keystroke });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
    assert!(vcx.update(|window, cx| input.read(cx).preview_close_focus.is_focused(window)));
    vcx.simulate_keystrokes("tab");
    assert!(vcx.update(|window, cx| input.read(cx).preview_remove_focus.is_focused(window)));
    vcx.simulate_keystrokes("shift-tab");
    assert!(vcx.update(|window, cx| input.read(cx).preview_close_focus.is_focused(window)));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("paste-image-preview-dialog").is_none());
    assert!(vcx.update(|window, cx| input.read(cx).focus_handle.is_focused(window)));
}

#[gpui::test]
fn composer_image_chip_never_disappears_at_wrap_boundary(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(240.), px(400.)));
    for n in 1..45 {
        input.update(vcx, |input, cx| {
            input.attachments.clear();
            input.set_content(format!("{} ", "x".repeat(n)), cx);
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("composer-image-chip").is_some(),
            "prefix length {n}"
        );
    }
}

#[gpui::test]
fn composer_image_modal_is_bounded_and_dismissible(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 1000,
                height: 600,
            },
            cx,
        );
        input.attachment_preview = Some(paste_preview::Preview::new(0));
        input.focus_handle.focus(window, cx);
        input
    });
    let window = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(window, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    let modal = vcx.debug_bounds("paste-image-preview-dialog").unwrap();
    assert!(modal.size.width <= px(640.) && modal.size.height <= px(480.));
    assert!((modal.center().x - px(400.)).abs() < px(1.));
    assert!((modal.center().y - px(300.)).abs() < px(1.));
    vcx.simulate_click(point(px(5.), px(5.)), Default::default());
    vcx.run_until_parked();
    assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_none()));
    input.update(vcx, |input, cx| {
        input.attachment_preview = Some(paste_preview::Preview::new(0));
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_none()));
}

#[gpui::test]
fn image_preview_remove_is_undoable(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        input.focus_handle.focus(window, cx);
        input
    });
    input.update(vcx, |input, cx| {
        input.attachment_preview = Some(paste_preview::Preview::new(0));
        cx.notify();
    });
    vcx.run_until_parked();
    let remove = vcx
        .debug_bounds("paste-image-preview-remove")
        .unwrap()
        .center();
    vcx.simulate_click(remove, Default::default());
    input.read_with(vcx, |input, _| {
        assert!(input.content.is_empty());
        assert!(input.attachments.is_empty());
    });
    vcx.simulate_keystrokes("cmd-z");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "[#image 1]");
        assert_eq!(input.attachments.len(), 1);
    });
}

#[gpui::test]
fn image_history_never_replays_a_chip_without_its_image(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.set_content("describe ".into(), cx);
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        input.focus_handle.focus(window, cx);
        input
    });
    vcx.simulate_keystrokes("enter up");
    input.read_with(vcx, |input, _| {
        assert!(!input.content.contains("[#image"));
        assert!(input.attachments.is_empty());
    });
}

#[gpui::test]
fn image_draft_round_trips_through_history_navigation(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.history.push("old prompt".into());
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: include_bytes!("../../../../assets/previews/image-preview.png").to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        input.focus_handle.focus(window, cx);
        input
    });
    let draft = input.read_with(vcx, |input, _| input.snapshot());
    vcx.simulate_keystrokes("up");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "old prompt");
        assert!(input.attachments.is_empty());
    });
    let persisted = input.read_with(vcx, |input, _| input.snapshot());
    input.update(vcx, |input, cx| input.restore(persisted, cx));
    vcx.simulate_keystrokes("down");
    input.read_with(vcx, |input, _| {
        let restored = input.snapshot();
        assert_eq!(restored.content, draft.content);
        assert_eq!(restored.attachments, draft.attachments);
        assert_eq!(restored.selection_start, draft.selection_start);
        assert_eq!(restored.selection_end, draft.selection_end);
    });
}

#[gpui::test]
fn image_chips_preview_undo_and_submit_in_text_order(cx: &mut TestAppContext) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let output = seen.clone();
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::new(cx, "test", move |text, images, _, _| {
            *output.lock().unwrap() = Some((text, images));
        });
        input.focus_handle.focus(window, cx);
        input
    });
    input.update(vcx, |input, cx| {
        let image = |byte| crate::clipboard_image::ClipboardImage {
            media_type: "image/png".into(),
            bytes: vec![byte],
            width: 1,
            height: 1,
        };
        input.set_session_image_base(9, cx);
        input.set_content("AB".into(), cx);
        input.sel.range = 2..2;
        input.attach_image(image(1), cx);
        input.sel.range = 0..0;
        input.attach_image(image(2), cx);
        assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)]);
        let draft = input.content.clone();
        input.open_model_menu(Vec::new(), cx);
        assert!(input.image_chip_numbers().is_empty());
        input.close_model_menu(cx);
        assert_eq!(input.content, draft);
        assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)]);
    });
    vcx.run_until_parked();
    let position = input.read_with(vcx, |input, _| {
        let bounds = input.last_bounds.unwrap();
        bounds.origin
            + input
                .last_layout
                .as_ref()
                .unwrap()
                .position_for_index(4, input.last_line_height.unwrap())
                .unwrap()
            + point(px(0.), input.last_line_height.unwrap() / 2.)
    });
    vcx.simulate_click(position, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("paste-image-preview-content").is_some());
    let close = vcx
        .debug_bounds("paste-image-preview-close")
        .unwrap()
        .center();
    vcx.simulate_click(close, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("paste-image-preview-content").is_none());
    input.update(vcx, |input, cx| {
        let end = input.chip_ranges()[1].1.end;
        input.sel.range = end..end;
        cx.notify();
    });
    vcx.simulate_keystrokes("backspace");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.image_chip_numbers(), [Some(10)])
    });
    vcx.simulate_keystrokes("cmd-z");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)])
    });
    vcx.simulate_keystrokes("enter");
    let (text, images) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(text, "[#image 1]AB[#image 2]");
    assert_eq!(
        images,
        vec![
            ("image/png".into(), "Ag==".into()),
            ("image/png".into(), "AQ==".into())
        ]
    );
}

#[test]
fn slash_palette_filters_commands_and_models() {
    let models = vec!["gpt-5.6-sol".into(), "claude-fable-5".into()];
    let commands = command_suggestions("/m", &models);
    assert_eq!(commands[0].value, "/model");
    assert!(commands.iter().any(|entry| entry.value == "/models"));

    let picker = command_suggestions("/model claude", &models);
    assert_eq!(picker.len(), 1);
    assert_eq!(picker[0].value, "/model claude-fable-5");
    assert!(command_suggestions("hello /model", &models).is_empty());

    let effort = command_suggestions("/effort x", &models);
    assert_eq!(effort[0].value, "/effort xhigh");

    let tui_commands = command_suggestions("/session", &models);
    assert!(tui_commands.iter().any(|entry| entry.value == "/sessions"));
    assert!(
        command_suggestions("/logical commits", &models)
            .iter()
            .any(|entry| entry.value == "/commit")
    );
}

#[test]
fn slash_palette_keeps_every_command_reachable_by_scrolling() {
    let suggestions = command_suggestions("/", &[]);
    let registered = registered_command_entries().collect::<Vec<_>>();
    assert!(registered.len() > 12);
    assert_eq!(suggestions.len(), registered.len());
    for (suggestion, (command, _)) in suggestions.iter().zip(registered) {
        assert_eq!(suggestion.value, command);
    }
}

#[test]
fn enter_accepts_highlighted_description_match() {
    let suggestions = command_suggestions("/logical commits", &[]);
    assert_eq!(
        accepted_command_submission(&suggestions, 0).as_deref(),
        Some("/commit")
    );
    let aliases = command_suggestions("/models", &[]);
    assert_eq!(
        accepted_command_submission(&aliases, 0).as_deref(),
        Some("/models")
    );
}

#[test]
fn filtered_model_search_accepts_the_full_selected_model_id() {
    let models = vec!["gpt-5.6-sol".into(), "gpt-5.6-luna".into()];
    let suggestions = command_suggestions("/model luna", &models);

    assert_eq!(suggestions.len(), 1);
    assert_eq!(
        accepted_command_submission(&suggestions, 0).as_deref(),
        Some("/model gpt-5.6-luna")
    );
}

fn input_window(cx: &mut TestAppContext) -> gpui::WindowHandle<PromptInput> {
    cx.update(bind_keys);
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}))
        })
        .unwrap()
    });
    window
        .update(cx, |input, window, cx| {
            window.focus(&input.focus_handle, cx)
        })
        .unwrap();
    window
}

#[gpui::test]
fn arrow_history_waits_for_the_edge_of_a_soft_wrapped_prompt(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let draft = "界 👩‍💻 Привет Jcode ".repeat(80);
    window
        .update(cx, |input, _, cx| {
            input.history.push("older prompt".into());
            input.set_content(draft.clone(), cx);
        })
        .unwrap();
    cx.run_until_parked();

    cx.simulate_keystrokes(*window, "up");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), draft.as_str());
            assert!(input.sel.cursor() < draft.len());
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "down");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), draft.as_str());
            assert_eq!(input.sel.cursor(), draft.len());
        })
        .unwrap();

    cx.simulate_keystrokes(*window, "home up");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "older prompt")
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(*window, "down");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), draft.as_str())
        })
        .unwrap();
}

#[gpui::test]
fn arrow_navigation_keeps_its_column_across_short_and_empty_lines(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let draft = "abcdef\n\nx\nabcdef";
    window
        .update(cx, |input, _, cx| input.set_content(draft.into(), cx))
        .unwrap();
    cx.run_until_parked();

    for (key, expected) in [
        ("up", 9),
        ("up", 7),
        ("up", 6),
        ("down", 7),
        ("down", 9),
        ("down", draft.len()),
    ] {
        cx.simulate_keystrokes(*window, key);
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.sel.cursor(), expected, "{key}")
            })
            .unwrap();
    }
}

#[gpui::test]
fn arrow_navigation_uses_reflowed_rows_after_resize(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let draft = "word word ".repeat(12);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.history.push("old prompt".into());
        input.set_content(draft.clone(), cx);
        window.focus(&input.focus_handle, cx);
        input
    });
    vcx.simulate_resize(size(px(180.), px(500.)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("home down");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), draft.as_str());
        assert!((1..draft.len()).contains(&input.sel.cursor()));
    });
    vcx.simulate_keystrokes("up");
    input.read_with(vcx, |input, _| assert_eq!(input.sel.cursor(), 0));

    vcx.simulate_resize(size(px(1600.), px(500.)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("end up");
    input.read_with(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "old prompt")
    });
}

#[gpui::test]
fn arrow_navigation_does_not_split_image_or_file_chips(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test-file.png");
    let png = include_bytes!("../../../../assets/previews/image-preview.png");
    std::fs::write(&path, png).unwrap();
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
        input.attach_image(
            crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: png.to_vec(),
                width: 640,
                height: 480,
            },
            cx,
        );
        input.replace_text_in_range(None, "\n", window, cx);
        input.attach_pasted_files(&[path], window, cx);
        input.replace_text_in_range(None, "\nabc", window, cx);
        window.focus(&input.focus_handle, cx);
        input
    });
    vcx.run_until_parked();
    let before = input.read_with(vcx, |input, _| input.snapshot());
    for label in ["test-file.png", "[#image"] {
        vcx.simulate_keystrokes("up");
        input.read_with(vcx, |input, _| {
            let (_, chip) = input
                .chip_ranges()
                .into_iter()
                .find(|(_, range)| input.content[range.clone()].contains(label))
                .unwrap();
            assert!([chip.start, chip.end].contains(&input.sel.cursor()));
        });
    }
    vcx.simulate_keystrokes("down down");
    assert_eq!(input.read_with(vcx, |input, _| input.snapshot()), before);
}

#[gpui::test]
fn composer_double_click_selects_a_word_and_preserves_shift_extension(cx: &mut TestAppContext) {
    let handle = input_window(cx);
    handle
        .update(cx, |input, _, cx| {
            input.set_content("hello world".into(), cx)
        })
        .unwrap();
    cx.run_until_parked();
    handle
        .update(cx, |input, window, cx| {
            let origin = input.last_bounds.unwrap().origin;
            let position = origin
                + input
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_index(2, window.line_height())
                    .unwrap();
            let event = MouseDownEvent {
                button: MouseButton::Left,
                position: position + point(px(0.), window.line_height() / 2.),
                modifiers: Default::default(),
                click_count: 2,
                first_mouse: false,
            };
            input.on_mouse_down(&event, window, cx);
            assert_eq!(input.sel.range, 0..5);
            assert!(input.focus_handle.is_focused(window));
            let mut shift_event = event;
            shift_event.modifiers.shift = true;
            input.on_mouse_down(&shift_event, window, cx);
            assert_eq!(input.sel.range, 0..2);
        })
        .unwrap();
}

/// Mouse handlers run outside paint, where `window.line_height()` reflects
/// the window default rather than the composer's 14px text style. The hit
/// test must use the row height the layout was actually painted with.
#[gpui::test]
fn composer_click_on_each_line_places_caret_on_that_line(cx: &mut TestAppContext) {
    let handle = input_window(cx);
    handle
        .update(cx, |input, _, cx| {
            input.set_content("first\nsecond\nthird".into(), cx)
        })
        .unwrap();
    cx.run_until_parked();
    handle
        .update(cx, |input, window, cx| {
            let bounds = input.last_bounds.unwrap();
            assert_eq!(input.visual_line_count, 3);
            let row_height = bounds.size.height / 3.;
            for (row, line_start) in [(0, 0), (1, 6), (2, 13)] {
                // Just inside the top edge: a taller handler-time line
                // height would map this to the row above.
                let position = bounds.origin + point(px(1.), row_height * row as f32 + px(1.));
                let event = MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                input.on_mouse_down(&event, window, cx);
                assert_eq!(
                    input.sel.range,
                    line_start..line_start,
                    "click at the top of row {row}"
                );
                input.on_mouse_up(
                    &MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                    },
                    window,
                    cx,
                );
            }
        })
        .unwrap();
}

#[gpui::test]
fn composer_ime_bounds_follow_preedit_without_paint(cx: &mut TestAppContext) {
    let handle = input_window(cx);
    handle
        .update(cx, |input, window, cx| {
            input.replace_text_in_range(None, "测😀", window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    handle
        .update(cx, |input, window, cx| {
            let bounds = input.last_bounds.unwrap();
            let anchor = input.bounds_for_range(3..3, bounds, window, cx).unwrap();
            let mut widths = Vec::new();
            // macOS queries the rect synchronously within the same frame.
            for marked in ["q", "qishi", "q"] {
                let end = marked.len();
                input.replace_and_mark_text_in_range(None, marked, Some(end..end), window, cx);
                let rect = input
                    .bounds_for_range(3..3 + end, bounds, window, cx)
                    .expect("preedit must have bounds before the next paint");
                assert_eq!(rect.origin, anchor.origin);
                assert_eq!(rect.size.height, anchor.size.height);
                widths.push(rect.size.width);
            }
            assert!(widths[0] > px(0.));
            assert!(widths[1] > widths[0]);
            assert_eq!(widths[2], widths[0]);

            let marked = "界\n中文";
            input.replace_and_mark_text_in_range(None, marked, None, window, cx);
            let first_line = input.bounds_for_range(3..6, bounds, window, cx).unwrap();
            assert_eq!(
                first_line,
                input.bounds_for_range(3..4, bounds, window, cx).unwrap()
            );
            let second_line = input.bounds_for_range(5..6, bounds, window, cx).unwrap();
            assert_eq!(second_line.top(), first_line.bottom());
            input.replace_text_in_range(None, "其实", window, cx);
            assert_eq!(input.content.as_ref(), "测😀其实");
        })
        .unwrap();
}

#[gpui::test]
fn pinyin_composition_after_cjk_text_does_not_panic(cx: &mut TestAppContext) {
    // Issue #6: macOS Pinyin marks "ni", "ni h", "ni hao" with a caret at
    // the end of the composition, then commits the candidate. Earlier CJK
    // text made the selection math slice inside a character and abort.
    let window = input_window(cx);
    window
        .update(cx, |input, window, cx| {
            input.replace_text_in_range(None, "你好，", window, cx);
            for marked in ["n", "ni", "ni h", "ni hao"] {
                let caret = marked.encode_utf16().count();
                input.replace_and_mark_text_in_range(None, marked, Some(caret..caret), window, cx);
                assert_eq!(input.content.as_ref(), format!("你好，{marked}"));
                let start = "你好，".len();
                assert_eq!(input.sel.marked, Some(start..start + marked.len()));
                assert_eq!(input.sel.range, start + marked.len()..start + marked.len());
                assert_eq!(
                    input.marked_text_range(window, cx),
                    Some(3..3 + marked.encode_utf16().count())
                );
            }
            // A CJK composition with an out-of-range selection is clamped.
            input.replace_and_mark_text_in_range(None, "你好", Some(1..9), window, cx);
            assert_eq!(input.content.as_ref(), "你好，你好");
            let start = "你好，".len();
            assert_eq!(input.sel.range, start + 3..start + 6);
            input.replace_text_in_range(None, "你好🚀", window, cx);
            assert_eq!(input.content.as_ref(), "你好，你好🚀");
            assert!(input.sel.marked.is_none());
            assert_eq!(input.sel.range, input.content.len()..input.content.len());

            // Stale marked text and ranges that split characters or run
            // past the end are clamped, never sliced blindly.
            input.sel.marked = Some(1..400);
            input.replace_and_mark_text_in_range(None, "zh", Some(2..2), window, cx);
            assert_eq!(input.content.as_ref(), "zh");
            input.replace_text_in_range(Some(1..1000), "中", window, cx);
            assert_eq!(input.content.as_ref(), "z中");
            // Deliberately inverted: the handler must clamp the range
            // instead of slicing blindly, which is the case under test.
            #[allow(clippy::reversed_empty_ranges)]
            let inverted = 5..2;
            input.replace_text_in_range(Some(inverted), "文", window, cx);
            assert_eq!(input.content.as_ref(), "z中文");
            let mut actual = None;
            assert_eq!(
                input.text_for_range(1..99, &mut actual, window, cx),
                Some("中文".into())
            );
            assert_eq!(actual, Some(1..3));
        })
        .unwrap();
    cx.run_until_parked();
}

/// `[dictation] mode = "insert"`: the words arrive at the caret, not after
/// a newline, and one undo removes the whole insertion.
#[gpui::test]
fn dictation_inserts_at_the_caret_as_one_undo_step(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    for (draft, caret, spoken, expected, caret_after) in [
        ("", 0, "run the suite", "run the suite", 13),
        ("fix the test", 4, "flaky ", "fix flaky the test", 10),
        ("fix the test", 12, "now", "fix the test now", 16),
        ("keep", 0, "please ", "please keep", 7),
        ("один два", 9, "три", "один три два", 16),
    ] {
        input.update(cx, |input, cx| {
            input.set_content(draft.into(), cx);
            input.sel.range = caret..caret;
            input.undo.clear();
            input.redo = vec![EditSnapshot {
                content: "old redo".into(),
                attachments: vec![],
            }];
            input.insert_dictation(spoken, cx);
            assert_eq!(input.content.as_ref(), expected);
            assert_eq!(input.sel.range, caret_after..caret_after);
            assert_eq!(
                input
                    .undo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec![draft.to_owned()],
                "one undo step"
            );
            assert!(input.redo.is_empty());
            assert!(input.sel.marked.is_none());
            assert!(input.history_index.is_none());
        });
    }
}

/// A selection is replaced the way typing over it would, so dictation into
/// a highlighted word does not duplicate it.
#[gpui::test]
fn dictation_insert_replaces_the_selection(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        input.set_content("keep drop tail".into(), cx);
        input.sel.range = 5..9;
        input.insert_dictation("spare", cx);
        assert_eq!(input.content.as_ref(), "keep spare tail");
        assert_eq!(input.sel.range, 10..10);
    });
}

#[gpui::test]
fn dictation_appends_newline_without_replacing_existing_draft(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    for (before, expected) in [
        ("", "spoken words"),
        ("typed", "typed\nspoken words"),
        ("typed ", "typed \nspoken words"),
        ("typed\t", "typed\t\nspoken words"),
        ("typed\n", "typed\nspoken words"),
        ("typed\n\n", "typed\n\nspoken words"),
        ("你好 🚀", "你好 🚀\nspoken words"),
    ] {
        input.update(cx, |input, cx| {
            input.set_content(before.into(), cx);
            input.undo.clear();
            input.redo = vec![EditSnapshot {
                content: "old redo".into(),
                attachments: vec![],
            }];
            input.sel.range = 0..input.content.len();
            input.sel.marked = Some(0..input.content.len());
            input.history_index = Some(0);
            input.append_dictation("  spoken words\n", cx);
            assert_eq!(input.content.as_ref(), expected);
            assert_eq!(
                input
                    .undo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec![before.to_owned()]
            );
            assert!(input.redo.is_empty());
            assert_eq!(input.sel.range, expected.len()..expected.len());
            assert!(input.sel.marked.is_none());
            assert!(input.history_index.is_none());
        });
    }
}

/// `[dictation] mode = "replace"` throws a draft away, so it must be one
/// undoable edit: the discarded draft comes back with a single undo, and a
/// stale redo from before the replacement is cleared.
#[gpui::test]
fn dictation_replace_is_one_undoable_edit(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    for draft in ["", "half typed", "多行\n草稿", "  keep spacing  "] {
        input.update(cx, |input, cx| {
            input.set_content(draft.into(), cx);
            input.sel.range = 0..input.content.len();
            input.sel.marked = Some(0..2);
            input.history_index = Some(0);
            input.undo.clear();
            input.redo = vec![EditSnapshot {
                content: "stale redo".into(),
                attachments: vec![],
            }];
            input.replace_dictation("  spoken words  ", cx);
            assert_eq!(input.content.as_ref(), "spoken words");
            assert_eq!(
                input
                    .undo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec![draft.to_owned()],
                "one undo step"
            );
            assert!(input.redo.is_empty(), "a replacement clears redo");
            assert_eq!(input.sel.range, 12..12);
            assert!(input.sel.marked.is_none());
            assert!(input.history_index.is_none());
        });
    }
}

/// An empty utterance never wipes the draft and never touches undo history.
#[gpui::test]
fn empty_replace_dictation_keeps_the_draft(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        input.set_content("keep me".into(), cx);
        input.undo = vec![EditSnapshot {
            content: "previous".into(),
            attachments: vec![],
        }];
        input.redo = vec![EditSnapshot {
            content: "next".into(),
            attachments: vec![],
        }];
        input.sel.range = 0..3;
        input.replace_dictation("   ", cx);
        assert_eq!(input.content.as_ref(), "keep me");
        assert_eq!(input.sel.range, 0..3);
        assert_eq!(
            input
                .undo
                .iter()
                .map(|s| s.content.as_str())
                .collect::<Vec<_>>(),
            vec!["previous"]
        );
        assert_eq!(
            input
                .redo
                .iter()
                .map(|s| s.content.as_str())
                .collect::<Vec<_>>(),
            vec!["next"]
        );
    });
}

mod more;
