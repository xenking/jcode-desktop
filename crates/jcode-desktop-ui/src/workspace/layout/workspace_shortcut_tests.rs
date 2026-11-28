//! Workspace tests: shortcut.
use super::*;

#[test]
fn help_session_prompt_carries_docs_instruction_and_visible_hints() {
    assert!(HELP_SESSION_PROMPT.contains("bundled Jcode documentation"));
    assert!(HELP_SESSION_PROMPT.contains("Super+H/J/K/L"));
    assert!(HELP_SESSION_PROMPT.contains("Super+/ or F1"));
    assert!(HELP_SESSION_PROMPT.contains("Super+Shift+/"));
}

/// Drive real keystrokes through the real keymap and confirm the coach
/// draws the right conclusions. This is the acceptance path: it exercises
/// the bindings the user actually presses, not the handlers directly.
#[gpui::test]
fn real_keystrokes_teach_the_jump_shortcut_when_the_user_grinds(cx: &mut gpui::TestAppContext) {
    // A user who knows super-h/l well but has never used super-home/end.
    let now = learning::now();
    let mut coach = learning::Coach::new();
    for step in 0..8 {
        coach.used_shortcut("focus_left_right", now - (8 - step) * 3 * 86_400);
    }
    assert!(coach.mastery("focus_left_right", now) >= 0.7);

    let window = cx.update(|cx| {
        crate::bind_workspace_keys(cx);
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(coach, cx);
                for name in ["one", "two", "three", "four", "five"] {
                    workspace.push_test_panel(name, cx);
                }
                let _ = window;
                workspace
            })
        })
        .unwrap()
    });
    window
        .update(cx, |workspace, window, cx| {
            window.focus(&workspace.focus_handle, cx);
        })
        .unwrap();

    // Start at the right-hand end of the strip.
    cx.simulate_keystrokes(*window, "super-end");
    window
        .update(cx, |workspace, _, _| {
            assert_eq!(workspace.test_focus_position(), Some(4));
        })
        .unwrap();

    // Now grind back one panel at a time, which is what someone who does
    // not know super-home does.
    cx.simulate_keystrokes(*window, "super-h super-h super-h");
    window
        .update(cx, |workspace, _, _| {
            assert_eq!(workspace.test_focus_position(), Some(1));
            let coach = workspace.test_coach();
            assert_eq!(
                coach.active_hint_id(),
                Some("focus_first_last"),
                "grinding should teach the jump shortcut"
            );
            assert!(
                coach.effort_wasted > 0,
                "grinding should be counted as wasted effort"
            );
        })
        .unwrap();
}

/// Every instrumented skill must actually be reachable by the keys the
/// catalog advertises. This is the check the earlier work never had: it
/// drives each shortcut through the real keymap and asserts the coach
/// recognised it, so an instrumented skill that no keystroke can trigger
/// (a wrong binding, a guard that always returns early) cannot pass.
#[gpui::test]
fn every_taught_shortcut_is_reachable_by_its_advertised_keys(cx: &mut gpui::TestAppContext) {
    // Each skill, with keys that should exercise it from a fresh workspace.
    // Ordering matters only in that each sequence must leave enough panels
    // to work with; the assertions are per-skill.
    let cases: &[(&str, &str)] = &[
        ("new_panel", "super-enter"),
        ("focus_left_right", "super-h"),
        ("focus_first_last", "super-end"),
        ("focus_previous", "super-tab"),
        ("overview", "super-o"),
        ("move_panel", "super-shift-h"),
        ("move_panel_end", "super-shift-end"),
        ("cycle_width", "super-r"),
        ("maximize", "super-f"),
        ("width_presets", "super-2"),
        ("move_panel_strip", "super-shift-j"),
        ("focus_up_down", "super-j"),
        ("close_panel", CLOSE_PANEL_CHORD),
    ];

    for (skill_id, keys) in cases {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|window, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            // Enough panels that no shortcut is a no-op, and a second strip
            // populated so cross-strip navigation is meaningful.
            for name in ["one", "two", "three", "four"] {
                workspace.push_test_panel(name, cx);
            }
            workspace.active_row = 1;
            workspace.push_test_panel("below", cx);
            workspace.active_row = 0;
            workspace.active = 1;
            let _ = window;
            workspace
        });
        vcx.update(|window, cx| {
            let handle = workspace.read(cx).focus_handle.clone();
            window.focus(&handle, cx);
        });
        vcx.run_until_parked();

        // focus_previous needs somewhere to return to.
        if *skill_id == "focus_previous" {
            vcx.simulate_keystrokes("super-l");
            vcx.run_until_parked();
        }

        vcx.simulate_keystrokes(keys);
        vcx.run_until_parked();

        workspace.update(vcx, |workspace, _| {
            let coach = workspace.test_coach();
            let trace = coach.trace(skill_id);
            assert!(
                trace.recalled > 0,
                "{keys} should have registered {skill_id} as used, \
                 but the coach recorded no unaided use"
            );
            assert!(
                coach.mastery(skill_id, learning::now()) > 0.0,
                "{skill_id} should have nonzero mastery after {keys}"
            );
        });
    }
}

/// The catalog's advertised keys must be the keys that are actually bound.
/// Earlier work only checked that catalog entries existed by name; this
/// compares each advertised chord against the real keymap, so the coach can
/// never teach a keystroke the app does not listen for.
#[gpui::test]
fn advertised_keys_match_the_real_keymap(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let workspace = Workspace::for_test(learning::Coach::new(), cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();

    // Each advertised chord paired with the action it should invoke. A skill
    // that shows two chords (left/right) maps each to its own action.
    let expected: Vec<(&str, &str, Box<dyn gpui::Action>)> = vec![
        ("focus_left_right", "super-h", Box::new(FocusLeft)),
        ("focus_left_right", "super-l", Box::new(FocusRight)),
        ("focus_up_down", "super-j", Box::new(FocusDown)),
        ("focus_up_down", "super-k", Box::new(FocusUp)),
        ("focus_first_last", "super-u", Box::new(FocusFirst)),
        ("focus_first_last", "super-p", Box::new(FocusLast)),
        ("focus_previous", "super-tab", Box::new(FocusPrevious)),
        ("overview", "super-o", Box::new(ToggleOverview)),
        ("move_panel", "super-shift-h", Box::new(MovePanelLeft)),
        ("move_panel", "super-shift-l", Box::new(MovePanelRight)),
        ("move_panel_strip", "super-shift-j", Box::new(MovePanelDown)),
        ("move_panel_strip", "super-shift-k", Box::new(MovePanelUp)),
        (
            "move_panel_end",
            "super-shift-home",
            Box::new(MovePanelToFirst),
        ),
        (
            "move_panel_end",
            "super-shift-end",
            Box::new(MovePanelToLast),
        ),
        ("cycle_width", "super-r", Box::new(CycleWidth)),
        ("maximize", "super-f", Box::new(MaximizeWidth)),
        ("width_presets", "super-1", Box::new(WidthPreset1)),
        ("width_presets", "super-2", Box::new(WidthPreset2)),
        ("width_presets", "super-3", Box::new(WidthPreset3)),
        ("width_presets", "super-4", Box::new(WidthPreset4)),
        (
            "new_panel",
            "super-enter",
            Box::new(NewPanelInPinnedDirectory),
        ),
        ("close_panel", CLOSE_PANEL_CHORD, Box::new(ClosePanel)),
    ];

    // Every catalog skill must appear, so a new skill cannot skip this check.
    for skill in learning::SKILLS {
        assert!(
            expected.iter().any(|(id, _, _)| *id == skill.id),
            "{} is in the catalog but unchecked against the keymap",
            skill.id
        );
    }

    for (skill_id, chord, action) in &expected {
        let skill = learning::skill(skill_id).expect("catalog entry");
        // The chord must be one the catalog actually shows the user.
        assert!(
            advertises(skill.keys, chord),
            "{skill_id} shows {:?}, which does not include {chord:?}",
            skill.keys
        );
        let bound: Vec<String> = vcx.update(|window, _| {
            window
                .bindings_for_action(action.as_ref())
                .iter()
                .map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|keystroke| keystroke.unparse())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect()
        });
        // GPUI unparses the platform modifier per platform: "super-" on
        // Linux, "cmd-" on macOS. The catalog writes one chord for humans,
        // so compare on a normalized form rather than the literal string.
        let expected_chord = platform_chord(chord);
        assert!(
            bound
                .iter()
                .any(|actual| platform_chord(actual) == expected_chord),
            "{skill_id} advertises {chord:?} but that action binds {bound:?}"
        );
    }
}

/// Clicking a panel that a keypress would have focused is the pointer slow
/// path. This clicks the real panel element in a real rendered frame, rather
/// than testing the classification helper in isolation.
#[gpui::test]
fn clicking_a_neighbouring_panel_is_recorded_as_a_missed_shortcut(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for name in ["one", "two", "three"] {
            workspace.push_test_panel(name, cx);
        }
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();

    // Focus starts on the first panel; the second is its neighbour.
    workspace.update(cx, |workspace, _| {
        assert_eq!(workspace.test_focus_position(), Some(0));
        assert_eq!(workspace.test_coach().effort_wasted, 0);
    });

    let neighbour = cx
        .debug_bounds("panel-1")
        .expect("the second panel should have painted");
    cx.simulate_click(neighbour.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.test_focus_position(),
            Some(1),
            "the click should have moved focus"
        );
        let coach = workspace.test_coach();
        assert!(
            coach.effort_wasted > 0,
            "clicking a neighbour should count as work done the long way"
        );
        assert_eq!(
            coach.active_hint_id(),
            Some("focus_left_right"),
            "and should teach the navigation keys"
        );
    });
}

/// Hot reload reconstructs each panel and its selectable-text focus model.
/// Selectable text consumes mouse-down to preserve copy focus, so panel
/// activation must still happen when the corresponding mouse-up bubbles.
#[gpui::test]
fn clicking_selectable_text_in_a_restored_panel_activates_it(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace.push_test_panel("restored", cx);
        workspace
            .test_panel(1)
            .expect("second test panel")
            .update(cx, |panel, cx| panel.append_test_error("restored text", cx));
        workspace
    });
    cx.run_until_parked();

    let text = cx
        .debug_bounds("selectable-text-0-error")
        .expect("the restored panel's selectable text should paint");
    cx.simulate_click(text.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.test_focus_position(),
            Some(1),
            "mouse-up should activate the panel even when text consumed mouse-down"
        );
    });
}

/// Clicking the sidebar's "+" opens a session, but must never be mistaken
/// for knowing super-enter. Without this check, routing the button through the
/// keyboard handler would silently teach the coach that the user is fluent
/// in a shortcut they have never pressed.
#[gpui::test]
fn clicking_the_new_session_button_is_not_keyboard_credit(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();

    click_sidebar_navigation(&workspace, vcx, "sidebar-new-session");

    workspace.update(vcx, |workspace, _| {
        let coach = workspace.test_coach();
        let trace = coach.trace("new_panel");
        assert_eq!(
            trace.recalled, 0,
            "clicking the button must not count as recalling super-enter"
        );
        assert!(
            trace.slow_paths > 0,
            "clicking the button should be recorded as the long way round"
        );
        assert!(coach.effort_wasted > 0, "and should count as wasted effort");
    });
}

#[gpui::test]
fn sidebar_navigation_scrolls_with_wheel_and_track(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.layout_mode = crate::config::LayoutMode::Normal;
        w
    });
    vcx.run_until_parked();
    let tabs = vcx.debug_bounds("sidebar-navigation-tabs").unwrap();
    let track = vcx.debug_bounds("sidebar-navigation-scrollbar").unwrap();
    assert!(
        track.bottom() <= tabs.top() + px(10.0),
        "outline scrollbar stays above tab labels and content"
    );
    let max = workspace.read_with(vcx, |w, _| w.sidebar_navigation_scroll.max_offset().x);
    assert!(max > px(0.0), "tabs must retain their widths and overflow");
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: tabs.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -2.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    assert!(workspace.read_with(vcx, |w, _| w.sidebar_navigation_scroll.offset().x) < px(0.0));
    vcx.simulate_click(
        gpui::point(track.right() - px(1.0), track.center().y),
        gpui::Modifiers::default(),
    );
    vcx.run_until_parked();
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.sidebar_navigation_scroll.offset().x),
        -max
    );
    let last = vcx.debug_bounds("sidebar-new-session").unwrap();
    assert!(
        last.right() <= tabs.right() + px(1.0),
        "last tab must be reachable"
    );
    vcx.simulate_event(gpui::MouseMoveEvent {
        position: gpui::point(track.left() + px(1.0), track.center().y),
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: gpui::Modifiers::default(),
    });
    vcx.run_until_parked();
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.sidebar_navigation_scroll.offset().x),
        px(0.0)
    );
}


#[gpui::test]
fn inbox_button_lives_in_the_sidebar_and_opens_gmail(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace
    });
    vcx.run_until_parked();

    assert!(vcx.debug_bounds("sidebar-section-trigger").unwrap().right() <= px(SIDEBAR_WIDTH));
    click_sidebar_navigation(&workspace, vcx, "open-gmail");
    fake_gmail_provider(vcx);
    sync_applets(&workspace, vcx);

    workspace.update(vcx, |workspace, cx| {
        assert_eq!(
            workspace.slots[workspace.active].panel.session_id(cx),
            "applet://gmail#launch0"
        );
    });
}

#[gpui::test]
fn tab_plus_replaces_the_right_edge_new_session_target(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace
    });
    vcx.run_until_parked();

    assert!(vcx.debug_bounds("edge-new-session").is_none());
    let plus = vcx.debug_bounds("tab-new-session").unwrap();
    assert_eq!(plus.size, gpui::size(px(28.0), px(28.0)));
    vcx.simulate_click(plus.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    workspace.update(vcx, |workspace, _| {
        assert!(
            workspace.test_coach().trace("new_panel").slow_paths > 0,
            "clicking the tab plus should invoke the pointer spawn path"
        );
    });
}

/// Clicking the panel that is already focused is not a missed shortcut: no
/// keypress would have done anything, so it must not be held against them.
#[gpui::test]
fn clicking_the_focused_panel_is_not_a_missed_shortcut(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for name in ["one", "two"] {
            workspace.push_test_panel(name, cx);
        }
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();

    let focused = cx
        .debug_bounds("panel-0")
        .expect("the focused panel should have painted");
    cx.simulate_click(focused.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        let coach = workspace.test_coach();
        assert_eq!(
            coach.effort_wasted, 0,
            "clicking into the panel you are already in is not a slow path"
        );
        assert_eq!(coach.active_hint_id(), None, "and should not be lectured");
    });
}

/// The hint lives in spare header space without changing panel geometry.
#[gpui::test]
fn the_hint_chip_uses_only_spare_tab_space(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace
    });
    let handle = cx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
        window.window_handle()
    });
    cx.simulate_window_resize(handle, gpui::size(px(1600.), px(800.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("coach-toast").is_none());
    let canvas_before = cx.debug_bounds("workspace-canvas").unwrap();
    let tab_before = cx.debug_bounds("live-session-tab-0").unwrap();
    workspace.update(cx, |workspace, cx| workspace.missed("new_panel", cx));
    cx.run_until_parked();
    let tip = cx
        .debug_bounds("coach-toast")
        .expect("hint chip should paint");
    let row = cx.debug_bounds("workspace-tab-row").unwrap();
    let new = cx.debug_bounds("tab-new-session").unwrap();
    assert_eq!(cx.debug_bounds("workspace-canvas").unwrap(), canvas_before);
    assert_eq!(cx.debug_bounds("live-session-tab-0").unwrap(), tab_before);
    assert!(tip.left() >= tab_before.right() + px(16.0));
    assert!(tip.right() < new.left());
    // Entry slides from six pixels above its settled position, inside chrome.
    assert!(tip.top() >= row.top() - px(4.0) && tip.bottom() <= row.bottom());
    for selector in ["coach-title", "coach-keys", "coach-dismiss"] {
        let child = cx.debug_bounds(selector).unwrap();
        assert!(child.left() >= tip.left() && child.right() <= tip.right());
        assert!(child.top() >= tip.top() && child.bottom() <= tip.bottom());
    }
    let title = cx.debug_bounds("coach-title").unwrap();
    cx.simulate_click(title.center(), gpui::Modifiers::default());
    cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.coach.active_hint_id().is_some());
        assert!(workspace.focus_handle.is_focused(window));
    });
    for width in [640.0, 480.0] {
        cx.simulate_window_resize(handle, gpui::size(px(width), px(800.0)));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("coach-toast").is_none(),
            "hide when crowded at {width}"
        );
        assert_eq!(
            cx.debug_bounds("workspace-canvas").unwrap().bottom(),
            canvas_before.bottom()
        );
    }
    cx.simulate_window_resize(handle, gpui::size(px(1600.), px(800.)));
    cx.run_until_parked();
    let dismiss = cx.debug_bounds("coach-dismiss").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::default());
    workspace.read_with(cx, |workspace, _| {
        assert!(workspace.coach.active_hint_id().is_none());
        assert!(
            workspace.coach_display_hint.is_some(),
            "retain content for exit fade"
        );
    });
}

/// The update chip must actually paint, and only while the updater has
/// something to say. This is the surface that would have told Bruno a fix
/// for the blank-text build was already downloading, so it is asserted
/// through the real render path rather than on state alone.
#[gpui::test]
fn the_update_chip_paints_only_while_the_updater_has_something_to_say(
    cx: &mut gpui::TestAppContext,
) {
    let _update_guard = updates::test_lock();
    updates::set_release_status(updates::ReleaseStatus::Unknown);
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace.single_panel = true;
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(1200.), px(800.)),
            |_, _| gpui::div(),
        );
        cx.run_until_parked();
    };

    // A current app says nothing at all.
    updates::set(updates::UpdateState::Idle);
    draw(cx);
    assert_ne!(
        updates::release_status(),
        updates::ReleaseStatus::Unknown,
        "single-panel windows must initialize update checks without a version footer"
    );
    assert!(
        cx.debug_bounds("update-chip").is_none(),
        "a current app should not paint an update chip"
    );

    // A download in flight is visible while it happens.
    updates::set(updates::UpdateState::Available {
        version: "0.1.0-beta.15".to_owned(),
    });
    draw(cx);
    let downloading = cx
        .debug_bounds("update-chip")
        .expect("a download in flight should paint the chip");
    assert!(
        downloading.size.width > px(0.) && downloading.size.height > px(0.),
        "the chip must occupy real space, got {downloading:?}"
    );
    assert!(
        downloading.origin.x >= px(0.0),
        "the chip should stay inside the workspace instead of spilling outside the window"
    );

    // And the staged build keeps offering the restart that applies it.
    updates::set(updates::UpdateState::ReadyToRestart {
        version: "0.1.0-beta.15".to_owned(),
    });
    draw(cx);
    let ready = cx
        .debug_bounds("update-chip")
        .expect("a staged update should keep the chip on screen");
    assert_eq!(
        ready.origin.x + ready.size.width,
        downloading.origin.x + downloading.size.width,
        "the chip should hold its right edge as its label changes"
    );

    // Clicking it must reach the installer. No Sparkle framework is loaded
    // in a test, so the click is a no-op rather than a relaunch, and the
    // chip must survive it.
    cx.simulate_click(ready.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("update-chip").is_some(),
        "clicking the chip must not tear it down"
    );

    updates::set(updates::UpdateState::Idle);
    draw(cx);
    assert!(
        cx.debug_bounds("update-chip").is_none(),
        "the chip should disappear once the updater goes quiet again"
    );
}

/// The coach view must paint too, and only while it is open.
#[gpui::test]
fn the_coach_view_paints_only_when_opened(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("only", cx);
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("coach-card").is_none(),
        "the coach view starts closed"
    );

    cx.simulate_keystrokes("super-/");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("coach-card").is_some(),
        "super-/ should open the coach view"
    );

    // Closing animates out over MODAL_DURATION, so the card is still painted
    // mid-fade; it must be gone once the transition has finished. The
    // animations are driven by the wall clock rather than the test clock, so
    // this waits out the real duration.
    cx.simulate_keystrokes("super-/");
    cx.run_until_parked();
    std::thread::sleep(transition::policy(Transition::Hints).duration * 2);
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("coach-card").is_none(),
        "super-/ should close it again"
    );
}

/// Moving onto a populated strip is real navigation and should be credited,
/// which is the counterpart to the no-op case below.
#[gpui::test]
fn moving_to_a_populated_strip_is_credited(cx: &mut gpui::TestAppContext) {
    let window = cx.update(|cx| {
        crate::bind_workspace_keys(cx);
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
                workspace.push_test_panel("top", cx);
                // A panel on the strip below.
                workspace.active_row = 1;
                workspace.push_test_panel("below", cx);
                workspace.active_row = 0;
                workspace.active = 0;
                let _ = window;
                workspace
            })
        })
        .unwrap()
    });
    window
        .update(cx, |workspace, window, cx| {
            window.focus(&workspace.focus_handle, cx);
        })
        .unwrap();

    cx.simulate_keystrokes(*window, "super-j");
    window
        .update(cx, |workspace, _, _| {
            assert_eq!(workspace.active_row, 1);
            assert!(
                workspace
                    .test_coach()
                    .mastery("focus_up_down", learning::now())
                    > 0.0,
                "landing on a populated strip should count"
            );
        })
        .unwrap();
}

/// The tutorial bug this pins down: on a fresh workspace with one panel,
/// every other strip is empty, so super-j/super-k never counted as
/// practiced and the ⌘J/⌘K lesson chips never went away. Pressing the
/// chord must clear the lesson even when the strip switch is a no-op,
/// while still granting no mastery and showing no hint. Asserted at the
/// rendered surface: the chip's completion checkmark must appear.
#[gpui::test]
fn pressing_into_empty_strips_clears_the_lesson_without_teaching(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("only", cx);
        workspace.sidebar_view = SidebarView::Learn;
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("tutorial-learned-focus_up_down").is_none(),
        "the strip lesson should start incomplete"
    );

    // Down onto an empty strip, and up against the top edge: both are
    // no-ops as navigation, both are the user producing the chord.
    vcx.simulate_keystrokes("super-j super-k super-k");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        let coach = workspace.test_coach();
        assert!(
            coach.trace("focus_up_down").practiced(),
            "the lesson should clear once the chord has been pressed"
        );
        assert_eq!(
            coach.mastery("focus_up_down", learning::now()),
            0.0,
            "a no-op press proves the chord, not the navigation"
        );
        assert_eq!(coach.effort_saved, 0);
        assert_eq!(coach.effort_wasted, 0);
        assert_eq!(coach.active_hint_id(), None);
    });
    assert!(
        vcx.debug_bounds("tutorial-learned-focus_up_down").is_some(),
        "the J/K chips should render their green completion state"
    );
}

#[gpui::test]
fn overview_navigation_keeps_root_focus_and_restores_composer(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for name in ["left", "middle", "right"] {
            workspace.push_test_panel(name, cx);
        }
        workspace.set_active(1, cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_active(window, cx));
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        assert_eq!(
            workspace.read(cx).navigation_state(window, cx)["keyboard_panel"],
            1
        );
    });

    vcx.simulate_keystrokes("super-o");
    std::thread::sleep(transition::policy(Transition::Overview).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    for (chord, expected) in [("super-h", 0), ("super-l", 1), ("super-l", 2)] {
        vcx.simulate_keystrokes(chord);
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.active, expected, "{chord} must navigate overview");
            assert!(workspace.overview, "navigation must keep overview visible");
            assert!(
                workspace.focus_handle.is_focused(window),
                "overview must own keyboard focus"
            );
        });
    }
    vcx.simulate_keystrokes("super-o");
    std::thread::sleep(transition::policy(Transition::Overview).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(!workspace.overview);
        assert_eq!(workspace.navigation_state(window, cx)["keyboard_panel"], 2);
    });
    vcx.simulate_keystrokes("super-h");
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _| assert_eq!(workspace.active, 1));
}

#[gpui::test]
fn overview_restore_and_new_panel_keep_mounted_focus(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("left", cx);
        workspace.push_test_panel("right", cx);
        workspace.set_active(1, cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let mut snapshot = workspace.snapshot(window, cx).unwrap();
            snapshot.overview = true;
            // Older generations could persist a detached panel handle.
            snapshot.focus = FocusSnapshot::Panel(1);
            workspace.apply_snapshot(snapshot, cx);
            workspace.restore_focus(window, cx);
        });
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        assert!(workspace.read(cx).focus_handle.is_focused(window));
    });
    vcx.simulate_keystrokes("super-h");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        assert_eq!(workspace.active, 0);
        workspace.apply(
            Update::SessionCreated {
                session: session_info("new-in-overview", None),
                request_id: None,
            },
            cx,
        );
        assert!(workspace.focus_pending);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.overview);
        assert!(workspace.focus_handle.is_focused(window));
        assert!(!workspace.focus_pending);
        assert_eq!(workspace.active, 1);
    });
    vcx.simulate_keystrokes("super-o");
    std::thread::sleep(transition::policy(Transition::Overview).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(!workspace.overview);
        assert_eq!(workspace.navigation_state(window, cx)["keyboard_panel"], 1);
    });
    vcx.simulate_keystrokes("super-h");
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _| assert_eq!(workspace.active, 0));
}

#[gpui::test]
fn navigation_keys_move_once_after_rebinding(cx: &mut gpui::TestAppContext) {
    // Plugin activation installs the keymap again on every hot reload.
    // GPUI tries older matching bindings if a handler leaves propagation on.
    cx.update(|cx| {
        for _ in 0..4 {
            crate::bind_workspace_keys(cx);
        }
    });
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for name in ["one", "two", "three", "four", "five"] {
            workspace.push_test_panel(name, cx);
        }
        workspace.set_active(2, cx);
        workspace
    });
    let mut pairs = vec![
        ("super-h", "super-l"),
        ("super-left", "super-right"),
        ("ctrl-shift-tab", "ctrl-tab"),
        ("ctrl-pageup", "ctrl-pagedown"),
    ];
    #[cfg(not(target_os = "macos"))]
    pairs.extend([("alt-h", "alt-l"), ("ctrl-alt-left", "ctrl-alt-right")]);
    #[cfg(target_os = "macos")]
    pairs.push(("cmd-left", "cmd-right"));
    for (left, right) in pairs {
        vcx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.set_active(2, cx);
                workspace.focus_active(window, cx);
            })
        });
        vcx.run_until_parked();
        // Test every destination, both boundaries, and rapid reversals from
        // the real focused composer rather than invoking handlers directly.
        for (chord, expected) in [
            (right, 3),
            (right, 4),
            (right, 4),
            (left, 3),
            (left, 2),
            (left, 1),
            (left, 0),
            (left, 0),
            (right, 1),
            (left, 0),
        ] {
            vcx.simulate_keystrokes(chord);
            vcx.run_until_parked();
            vcx.update(|window, cx| {
                let workspace = workspace.read(cx);
                assert_eq!(
                    workspace.active, expected,
                    "{chord} must move exactly one panel"
                );
                let state = workspace.navigation_state(window, cx);
                assert_eq!(state["focused_slot"], expected);
                assert_eq!(state["keyboard_panel"], expected);
            });
        }
    }
    // The same capture bug affected vertical navigation and could toggle
    // FocusPrevious an even number of times, appearing to do nothing.
    for (chord, row, position) in [
        ("super-end", 0, Some(4)),
        ("super-home", 0, Some(0)),
        ("super-tab", 0, Some(4)),
        ("super-tab", 0, Some(0)),
        ("super-j", 1, None),
        ("super-l", 1, None),
        ("super-k", 0, Some(0)),
        ("super-k", 0, Some(0)),
    ] {
        vcx.simulate_keystrokes(chord);
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, _| {
            assert_eq!(workspace.active_row, row, "{chord}");
            assert_eq!(workspace.test_focus_position(), position, "{chord}");
        });
    }
}

/// This is the public acceptance path for vertical navigation: real bound
/// keys must reveal a directional transition and returning to either strip
/// must restore that strip's own last-focused panel.
#[gpui::test]
fn vertical_keys_animate_and_restore_each_strips_focus(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("top-left", cx);
        workspace.push_test_panel("top-right", cx);
        workspace.active_row = 1;
        workspace.push_test_panel("bottom-left", cx);
        workspace.push_test_panel("bottom-right", cx);
        workspace.set_active(1, cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    vcx.run_until_parked();

    // Moving down initially preserves column position. The incoming strip
    // starts below the outgoing strip, proving the visible direction.
    vcx.simulate_keystrokes("super-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active_row, 1);
        assert_eq!(workspace.test_focus_position(), Some(1));
        assert!(workspace.row_progress.is_animating());
    });
    let outgoing = vcx
        .debug_bounds("row-transition-outgoing")
        .expect("the old strip should remain visible during the transition");
    let incoming = vcx
        .debug_bounds("row-transition-incoming")
        .expect("the new strip should be visible during the transition");
    assert!(
        incoming.origin.y > outgoing.origin.y,
        "moving down should bring the new strip in from below"
    );

    // Give the lower strip a distinct remembered position, then round-trip.
    vcx.simulate_keystrokes("super-h super-k");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active_row, 0);
        assert_eq!(workspace.test_focus_position(), Some(1));
    });
    vcx.simulate_keystrokes("super-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active_row, 1);
        assert_eq!(
            workspace.test_focus_position(),
            Some(0),
            "returning should restore the lower strip's last-focused panel"
        );
    });

    // The temporary outgoing layer must be retired after the policy duration.
    std::thread::sleep(transition::policy(Transition::Row).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.outgoing_row, None);
        assert!(!workspace.row_progress.is_animating());
    });
    assert!(
        vcx.debug_bounds("panel-2").is_some(),
        "the remembered lower panel should remain painted after settling"
    );
}

#[gpui::test]
fn vertical_keys_cover_both_animation_directions(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("top-left", cx);
        workspace.push_test_panel("top-right", cx);
        workspace.active_row = 1;
        workspace.push_test_panel("bottom-left", cx);
        workspace.push_test_panel("bottom-right", cx);
        workspace.set_active(1, cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("super-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(
            (workspace.active_row, workspace.test_focus_position()),
            (1, Some(1))
        );
        assert!(workspace.row_progress.is_animating());
    });
    let outgoing = vcx.debug_bounds("row-transition-outgoing").unwrap();
    let incoming = vcx.debug_bounds("row-transition-incoming").unwrap();
    assert!(
        incoming.origin.y > outgoing.origin.y,
        "down enters from below"
    );

    vcx.simulate_keystrokes("super-h");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("super-k");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(
            (workspace.active_row, workspace.test_focus_position()),
            (0, Some(1))
        );
    });
    let outgoing = vcx.debug_bounds("row-transition-outgoing").unwrap();
    let incoming = vcx.debug_bounds("row-transition-incoming").unwrap();
    assert!(
        incoming.origin.y < outgoing.origin.y,
        "up enters from above"
    );

    vcx.simulate_keystrokes("super-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(
            (workspace.active_row, workspace.test_focus_position()),
            (1, Some(0))
        );
    });

    std::thread::sleep(transition::policy(Transition::Row).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.outgoing_row, None);
        assert!(!workspace.row_progress.is_animating());
    });
}

#[gpui::test]
fn horizontal_panel_moves_animate_both_swapped_panels(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("left", cx);
        workspace.push_test_panel("right", cx);
        workspace.set_active(1, cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("super-shift-h");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active, 0);
        assert!(workspace.slots[0].order_offset.is_animating());
        assert!(workspace.slots[1].order_offset.is_animating());
    });

    std::thread::sleep(transition::policy(Transition::PanelOrder).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert!(!workspace.slots[0].order_offset.is_animating());
        assert!(!workspace.slots[1].order_offset.is_animating());
    });

    vcx.simulate_keystrokes("super-shift-l");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active, 1);
        assert!(workspace.slots[0].order_offset.is_animating());
        assert!(workspace.slots[1].order_offset.is_animating());
    });
}

#[gpui::test]
fn moving_a_panel_between_strips_animates_and_stops_at_boundaries(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("moving", cx);
        workspace.active_row = 1;
        workspace.push_test_panel("already-below", cx);
        workspace.set_active(0, cx);
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("super-shift-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active_row, 1);
        assert_eq!(workspace.slots[workspace.active].row, 1);
        assert!(workspace.row_progress.is_animating());
    });
    let outgoing = vcx.debug_bounds("row-transition-outgoing").unwrap();
    let incoming = vcx.debug_bounds("row-transition-incoming").unwrap();
    assert!(incoming.origin.y > outgoing.origin.y);

    // Repeated moves stop at the fourth strip without corrupting focus or
    // starting a transition for the impossible fifth move.
    vcx.simulate_keystrokes("super-shift-j super-shift-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| assert_eq!(workspace.active_row, 3));
    std::thread::sleep(transition::policy(Transition::Row).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.simulate_keystrokes("super-shift-j");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, _| {
        assert_eq!(workspace.active_row, 3);
        assert_eq!(workspace.outgoing_row, None);
        assert!(!workspace.row_progress.is_animating());
    });
}

/// The same harness, confirming a no-op keypress teaches nothing and earns
/// nothing: pressing into the edge of a strip is not evidence either way.
#[gpui::test]
fn a_keypress_that_does_nothing_changes_no_belief(cx: &mut gpui::TestAppContext) {
    let window = cx.update(|cx| {
        crate::bind_workspace_keys(cx);
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
                workspace.push_test_panel("only", cx);
                let _ = window;
                workspace
            })
        })
        .unwrap()
    });
    window
        .update(cx, |workspace, window, cx| {
            window.focus(&workspace.focus_handle, cx);
        })
        .unwrap();

    // One panel: every navigation key is a no-op.
    cx.simulate_keystrokes(*window, "super-h super-l super-j super-k");
    window
        .update(cx, |workspace, _, _| {
            let coach = workspace.test_coach();
            assert_eq!(coach.overall_mastery(learning::now()), 0.0);
            assert_eq!(coach.effort_saved, 0);
            assert_eq!(coach.effort_wasted, 0);
            assert_eq!(coach.active_hint_id(), None);
        })
        .unwrap();
}

/// Using a shortcut for real should register as knowledge, through the same
/// keymap the user types on.
#[gpui::test]
fn real_keystrokes_build_recognized_mastery(cx: &mut gpui::TestAppContext) {
    let window = cx.update(|cx| {
        crate::bind_workspace_keys(cx);
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
                for name in ["one", "two", "three"] {
                    workspace.push_test_panel(name, cx);
                }
                let _ = window;
                workspace
            })
        })
        .unwrap()
    });
    window
        .update(cx, |workspace, window, cx| {
            window.focus(&workspace.focus_handle, cx);
        })
        .unwrap();

    cx.simulate_keystrokes(*window, "super-l super-l super-f super-r");
    window
        .update(cx, |workspace, _, _| {
            let coach = workspace.test_coach();
            let now = learning::now();
            assert!(
                coach.mastery("focus_left_right", now) > 0.0,
                "navigating should register"
            );
            assert!(
                coach.mastery("maximize", now) > 0.0,
                "super-f should register"
            );
            assert!(
                coach.mastery("cycle_width", now) > 0.0,
                "super-r should register"
            );
            assert!(coach.effort_saved > 0);
            assert_eq!(coach.effort_wasted, 0, "no slow paths were taken");
        })
        .unwrap();
}

#[test]
fn catalog_chord_parsing_handles_pairs_and_ranges() {
    // The keymap check is only as good as its reading of the catalog's
    // human-facing key strings, so pin that reading down.
    assert!(advertises("super-h / super-l", "super-h"));
    assert!(advertises("super-h / super-l", "super-l"));
    assert!(!advertises("super-h / super-l", "super-j"));
    assert!(advertises("super-tab", "super-tab"));
    // Ranges stand for every key they span, and nothing outside it.
    assert!(advertises("super-1 .. super-4", "super-1"));
    assert!(advertises("super-1 .. super-4", "super-4"));
    assert!(!advertises("super-1 .. super-4", "super-5"));
    assert!(!advertises("super-1 .. super-4", "alt-2"));
}

#[test]
fn a_short_click_hop_is_read_as_plain_navigation() {
    // Clicking the neighbour is what super-h/l would have done.
    assert_eq!(click_skill(1, 1, 5), "focus_left_right");
    assert_eq!(click_skill(2, 2, 5), "focus_left_right");
    // A long hop that stops mid-strip is still ordinary navigation: no
    // single "jump" key would have landed there.
    assert_eq!(click_skill(4, 4, 9), "focus_left_right");
}

#[test]
fn a_long_click_hop_to_an_end_is_read_as_a_missed_jump() {
    // Crossing the strip to its far end is what super-end exists for.
    assert_eq!(click_skill(4, 4, 5), "focus_first_last");
    assert_eq!(click_skill(3, 0, 5), "focus_first_last");
}

#[test]
fn every_instrumented_skill_id_exists_in_the_catalog() {
    // The instrumentation refers to skills by string, so a typo would
    // silently stop teaching. Keep the two in step.
    for id in [
        "focus_left_right",
        "focus_up_down",
        "focus_first_last",
        "focus_previous",
        "overview",
        "move_panel",
        "move_panel_strip",
        "move_panel_end",
        "cycle_width",
        "maximize",
        "width_presets",
        "new_panel",
        "close_panel",
    ] {
        assert!(learning::skill(id).is_some(), "unknown skill id {id}");
    }
}

#[test]
fn the_catalog_covers_every_bound_workspace_action() {
    // Every shortcut the app binds should be teachable, or the coach will
    // report fluency it never actually measured.
    let taught: Vec<&str> = learning::SKILLS.iter().map(|skill| skill.keys).collect();
    for keys in [
        "super-h / super-l",
        "super-j / super-k",
        "super-u / super-p",
        "super-tab",
        "super-o",
        "super-shift-h / super-shift-l",
        "super-shift-j / super-shift-k",
        "super-shift-home / super-shift-end",
        "super-r",
        "super-f",
        "super-1 .. super-4",
        "super-enter",
        CLOSE_PANEL_CHORD,
    ] {
        assert!(taught.contains(&keys), "{keys} is bound but never taught");
    }
}
