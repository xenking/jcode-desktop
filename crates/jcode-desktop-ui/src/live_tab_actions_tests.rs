use super::*;

#[gpui::test]
fn title_actions_fit_and_rename_does_not_select_or_close(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session_fox_original", cx);
        w.push_test_panel("session_owl_neighbor", cx);
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [1440., 800., 640.] {
        vcx.simulate_mouse_move(gpui::point(px(350.), px(400.)), None, Default::default());
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        let tab = vcx.debug_bounds("live-session-tab-0").unwrap();
        let title = vcx.debug_bounds("live-session-tab-0-title").unwrap();
        assert!(vcx.debug_bounds("close-session-button-0").is_none());
        assert!(vcx.debug_bounds("close-session-button-1").is_none());
        assert!(vcx.debug_bounds("rename-session-button").is_none());
        vcx.simulate_mouse_move(tab.center(), None, Default::default());
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        let hovered_tab = vcx.debug_bounds("live-session-tab-0").unwrap();
        let hovered_close = vcx.debug_bounds("close-session-button-0").unwrap();
        let rename = vcx.debug_bounds("rename-session-button").unwrap();
        let hovered_title = vcx.debug_bounds("live-session-tab-0-title").unwrap();
        assert!(hovered_tab.size.height > tab.size.height);
        assert_eq!(hovered_tab.size.width, tab.size.width);
        assert_close_fits_cap(hovered_tab, hovered_close);
        assert_eq!(title.left(), hovered_title.left());
        assert!(title.right() > hovered_close.left());
        assert!(hovered_title.right() <= rename.left());
        assert!(rename.right() <= hovered_close.left());
        vcx.simulate_click(rename.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, _| {
            assert_eq!(w.active, 0);
            assert_eq!(w.slots.len(), 2);
            assert!(w.slots.iter().all(|slot| !slot.closing));
            assert!(w.rename_editor.is_some());
        });
        vcx.update(|_, cx| workspace.update(cx, |w, cx| w.close_rename_editor(cx)));
        vcx.run_until_parked();
    }
}

pub(super) fn assert_close_fits_cap(
    tab: gpui::Bounds<gpui::Pixels>,
    close: gpui::Bounds<gpui::Pixels>,
) {
    assert_eq!(close.size.width, tab.size.height);
    assert_eq!(close.size.height, tab.size.height);
    assert!((close.top() - tab.top()).abs() < px(0.01));
    assert!((close.right() - tab.right()).abs() < px(0.01));
}

#[gpui::test]
fn close_fill_and_glyph_follow_the_panel_activity(cx: &mut gpui::TestAppContext) {
    cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        for i in 0..6 {
            w.push_test_panel(&format!("close-colors-{i}"), cx);
        }
        w.seed_tab_status_fixture(cx);
        w.slots[4].panel.update(cx, |panel, cx| {
            panel.apply(
                &jcode_sdk::ApiEvent::Error {
                    code: jcode_sdk::api::ErrorCode::Internal,
                    message: "fixture error".into(),
                },
                cx,
            );
        });
        let theme = Theme::global();
        let idle = theme.workspace_accent(0);
        let expected = [
            theme.WARN,
            theme.LINK,
            theme.CODE_CONTROL,
            theme.OK,
            theme.ERROR,
            idle,
        ];
        for (slot, accent) in w.slots.iter().zip(expected) {
            let colors = TabCloseColors::for_tab(slot.panel.read(cx).tab_ring(), idle);
            assert_eq!(colors.background, accent.opacity(0.16));
            assert_eq!(colors.glyph, theme.TEXT.blend(accent.opacity(0.75)));
            assert_eq!(colors.hover_background, accent.opacity(0.32));
            assert_eq!(colors.hover_glyph, theme.TEXT.blend(accent.opacity(0.45)));
        }
        w
    });
}

#[gpui::test]
fn hovered_inactive_tab_close_targets_that_tab_only(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session_fox_original", cx);
        w.push_test_panel("session_owl_neighbor", cx);
        w
    });
    vcx.run_until_parked();
    // Hover the tab first: it pops out and its X appears at the popped edge.
    let tab = vcx.debug_bounds("live-session-tab-1-slot").unwrap();
    vcx.update(|window, cx| window.simulate_mouse_move(tab.center(), cx));
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.live_tabs.settle();
        cx.notify();
    });
    vcx.run_until_parked();
    let close = vcx.debug_bounds("close-session-button-1").unwrap();
    vcx.update(|window, cx| window.simulate_mouse_move(close.center(), cx));
    vcx.run_until_parked();
    vcx.simulate_click(close.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        let surviving: Vec<_> = w
            .slots
            .iter()
            .filter(|slot| !slot.closing)
            .map(|slot| slot.panel.read(cx).session_id.as_str())
            .collect();
        assert_eq!(surviving, ["session_fox_original"]);
        assert!(w.rename_editor.is_none());
    });
}

#[gpui::test]
fn tab_close_reveals_on_keyboard_focus_and_activates(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("session_fox_keyboard", cx);
        w
    });
    vcx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("close-session-button-0").is_none());

    let reveal = |vcx: &mut gpui::VisualTestContext| {
        for _ in 0..40 {
            vcx.simulate_keystrokes("tab");
            vcx.run_until_parked();
            if let Some(close) = vcx.debug_bounds("close-session-button-0") {
                assert_close_fits_cap(vcx.debug_bounds("live-session-tab-0").unwrap(), close);
                return;
            }
        }
        panic!("Tab did not reach the hidden close control");
    };
    reveal(vcx);
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("close-session-button-0").is_none());

    reveal(vcx);
    vcx.simulate_keystrokes("enter");
    vcx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("enter").unwrap(),
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| assert!(w.slots.iter().all(|slot| slot.closing)));
}
