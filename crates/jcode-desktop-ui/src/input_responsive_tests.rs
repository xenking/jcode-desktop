use super::*;

#[gpui::test]
fn editor_keeps_wheel_scroll_at_its_edge(cx: &mut gpui::TestAppContext) {
    struct Outer {
        input: Entity<PromptInput>,
        scroll: gpui::ScrollHandle,
    }
    impl Render for Outer {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("outer")
                .w(px(500.))
                .h(px(350.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(div().h(px(100.)))
                .child(self.input.clone())
                .child(div().h(px(600.)))
        }
    }
    cx.update(bind_keys);
    let (outer, vcx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, cx| {
            input.set_content("long line wrapping here ".repeat(200), cx);
            input.focus_handle.focus(window, cx);
        });
        Outer {
            input,
            scroll: gpui::ScrollHandle::new(),
        }
    });
    vcx.run_until_parked();
    let input = outer.read_with(vcx, |outer, _| outer.input.clone());
    vcx.simulate_keystrokes("ctrl-home");
    vcx.run_until_parked();
    let before_outer = outer.read_with(vcx, |outer, _| outer.scroll.offset().y);
    let before_editor = input.read_with(vcx, |input, _| input.editor_scroll.offset().y);
    let position = vcx.debug_bounds("prompt-editor").unwrap().center();
    let wheel = |vcx: &mut gpui::VisualTestContext, y| {
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(y))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
    };
    wheel(vcx, -40.);
    let after_editor = input.read_with(vcx, |input, _| input.editor_scroll.offset().y);
    assert!(after_editor < before_editor, "editor must scroll");
    assert_eq!(
        outer.read_with(vcx, |outer, _| outer.scroll.offset().y),
        before_outer,
        "outer transcript must not scroll with the editor"
    );
    input.update(vcx, |input, cx| {
        let max = input.editor_scroll.max_offset().y;
        input.editor_scroll.set_offset(point(px(0.), -max));
        cx.notify();
    });
    vcx.run_until_parked();
    wheel(vcx, -40.);
    assert_eq!(
        outer.read_with(vcx, |outer, _| outer.scroll.offset().y),
        before_outer,
        "the wheel must not escape at the editor edge"
    );
}

#[gpui::test]
fn long_draft_can_be_resized_and_reset_with_the_keyboard(cx: &mut gpui::TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::new(cx, "test", |_, _, _, _| {});
        window.focus(&input.focus_handle, cx);
        input
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-resize-handle").is_none());
    input.update(vcx, |input, cx| input.set_content("line\n".repeat(80), cx));
    vcx.run_until_parked();
    let auto = vcx.debug_bounds("prompt-editor").unwrap().size.height;
    assert!(vcx.debug_bounds("prompt-resize-handle").is_some());
    vcx.simulate_keystrokes("alt-shift-up");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-editor").unwrap().size.height > auto);
    vcx.simulate_keystrokes("alt-shift-down alt-shift-down");
    vcx.run_until_parked();
    assert_eq!(vcx.debug_bounds("prompt-editor").unwrap().size.height, auto);
}

#[gpui::test]
fn long_drafts_scroll_and_reveal_keyboard_caret_in_short_windows(cx: &mut gpui::TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|window, cx| {
        let input = PromptInput::new(cx, "test", |_, _, _, _| {});
        window.focus(&input.focus_handle, cx);
        input
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for (width, height) in [(240., 240.), (320., 300.), (600., 600.)] {
        vcx.simulate_window_resize(handle, size(px(width), px(height)));
        input.update(vcx, |input, cx| {
            input.set_content("a long prompt with many wrapped lines ".repeat(100), cx)
        });
        vcx.run_until_parked();
        let viewport = vcx.debug_bounds("prompt-editor").unwrap();
        assert!(viewport.size.height <= px((height * 0.25).min(160.)));
        assert!(viewport.right() <= px(width));
        for key in ["ctrl-home", "ctrl-end"] {
            vcx.simulate_keystrokes(key);
            vcx.run_until_parked();
            input.read_with(vcx, |input, _| {
                let bounds = input.last_bounds.unwrap();
                let line = input.last_layout.as_ref().unwrap();
                // Use the actual shaped line height, including theme scaling.
                let caret = line
                    .position_for_index(
                        input.cursor_offset(),
                        bounds.size.height / input.visual_line_count as f32,
                    )
                    .unwrap();
                let y = bounds.top() + caret.y;
                assert!(
                    y >= viewport.top() - px(1.) && y < viewport.bottom(),
                    "{key} caret {y:?} outside {viewport:?}"
                );
            });
        }
        let before = input.read_with(vcx, |input, _| input.editor_scroll.offset().y);
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(40.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        let after = input.read_with(vcx, |input, _| input.editor_scroll.offset().y);
        assert!(
            after > before,
            "wheel scroll should not snap back to the caret"
        );
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        input.read_with(vcx, |input, _| {
            assert!(input.content.is_empty());
            assert_eq!(input.visual_line_count, 1);
            assert_eq!(input.editor_scroll.offset().y, px(0.));
        });
    }
}
