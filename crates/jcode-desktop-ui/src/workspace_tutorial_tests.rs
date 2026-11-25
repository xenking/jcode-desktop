//! Workspace tests: tutorial.
use super::*;

#[test]
fn first_spawn_fills_the_viewport_and_later_spawns_use_the_default_width() {
    assert_eq!(spawned_panel_width(0), 1.0);
    assert_eq!(spawned_panel_width(1), DEFAULT_WIDTH);
    assert_eq!(spawned_panel_width(4), DEFAULT_WIDTH);
}

#[test]
fn the_lone_full_width_panel_halves_when_a_second_one_spawns() {
    // Spawning the second panel leaves two equal halves: the newcomer opens
    // at the default width and the incumbent gives up the extra space.
    assert_eq!(spawned_panel_width(1), DEFAULT_WIDTH);
    assert_eq!(demoted_width(1.0, 2), DEFAULT_WIDTH);
    // Every later spawn leaves the existing panels untouched, so the strip
    // keeps scrolling at the default width.
    assert_eq!(demoted_width(DEFAULT_WIDTH, 3), DEFAULT_WIDTH);
    assert_eq!(demoted_width(1.0, 3), 1.0);
    // A width the user picked is never overridden.
    assert_eq!(demoted_width(0.25, 2), 0.25);
    assert_eq!(demoted_width(0.75, 2), 0.75);
}

#[test]
fn the_minimap_scale_fits_the_widest_strip() {
    // A tall track never constrains these cases, so the width rule alone
    // decides the scale. A canvas narrower than the viewport still maps
    // the full viewport, so the lens can never overflow the track.
    assert_eq!(
        minimap_scale(160.0, 1000.0, 1000.0, 800.0, 500.0),
        160.0 / 1000.0
    );
    // A wide canvas is compressed to fit the track exactly.
    assert_eq!(
        minimap_scale(160.0, 1000.0, 1000.0, 800.0, 4000.0),
        160.0 / 4000.0
    );
    // Degenerate inputs never divide by zero.
    assert!(minimap_scale(160.0, 1.0, 0.0, 0.0, 0.0).is_finite());
}

#[test]
fn the_minimap_keeps_the_canvas_aspect_ratio() {
    // On a common landscape canvas the height cap wins, and a half-width
    // panel maps taller than wide, matching how it looks on screen.
    let scale = minimap_scale(160.0, 32.0, 1656.0, 1000.0, 1656.0);
    assert_eq!(scale, 32.0 / 1000.0);
    let mapped_w = 1656.0 * 0.5 * scale;
    let mapped_h = 1000.0 * scale;
    assert!(
        mapped_w < mapped_h,
        "a half-width panel should read taller than wide ({mapped_w} vs {mapped_h})"
    );
    // A full-width panel is wider than tall on screen, and stays that way.
    assert!(1656.0 * scale > mapped_h);
}

#[gpui::test]
fn minimap_is_hidden_by_default(cx: &mut gpui::TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("one", cx);
        workspace
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("panel-0").is_some());
    assert!(cx.debug_bounds("minimap").is_none());
}

#[gpui::test]
fn minimap_status_accents_leave_room_for_focus_and_viewport(cx: &mut gpui::TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.enable_test_minimap();
        workspace.push_test_panel("one", cx);
        workspace
    });
    cx.run_until_parked();
    let panel = cx.debug_bounds("minimap-panel-0").unwrap();
    let status = cx.debug_bounds("minimap-panel-0-idle").unwrap();
    let lens = cx.debug_bounds("minimap-viewport").unwrap();
    let pin = cx.debug_bounds("minimap-you-pin").unwrap();
    assert_eq!(status.size.height, px(2.0));
    assert_eq!(status.size.width, panel.size.width);
    assert_eq!(status.top(), panel.top());
    assert!(lens.top() < panel.top());
    assert!(lens.bottom() > panel.bottom());
    assert!(pin.top() >= status.bottom());
    assert_eq!(pin.size.width, px(4.0));
}

/// The minimap must actually paint in the top right and jumping through it
/// must work: this clicks the real minimap rectangle for the third panel in
/// a real rendered frame and asserts focus moved there.
#[gpui::test]
fn clicking_a_minimap_panel_jumps_focus_there(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.enable_test_minimap();
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

    let map = cx
        .debug_bounds("minimap")
        .expect("the minimap should paint");
    let window_width = cx.update(|window, _| f32::from(window.viewport_size().width));
    assert!(
        f32::from(map.right()) <= window_width + 1.0
            && f32::from(map.origin.x) > window_width / 2.0,
        "the minimap should sit in the top right"
    );
    let canvas = cx
        .debug_bounds("workspace-canvas")
        .expect("workspace canvas");
    assert!(
        map.top() == canvas.top() + px(MINIMAP_TOP),
        "the minimap should sit below any top-right window controls"
    );
    let initial_pin = cx
        .debug_bounds("minimap-you-pin")
        .expect("the focused panel should have a persistent visual pin");
    let initial_panel = cx
        .debug_bounds("minimap-panel-0")
        .expect("the focused panel should appear on the map");
    assert!(
        initial_panel.contains(&initial_pin.center()),
        "the location pin should start inside the focused panel"
    );

    let target = cx
        .debug_bounds("minimap-panel-2")
        .expect("the third panel should appear on the map");
    cx.simulate_click(target.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.test_focus_position(),
            Some(2),
            "clicking the minimap rectangle should jump focus to that panel"
        );
    });
    let moved_pin = cx
        .debug_bounds("minimap-you-pin")
        .expect("the location pin should remain visible after focus moves");
    let focused_panel = cx
        .debug_bounds("minimap-panel-2")
        .expect("the newly focused panel should remain on the map");
    assert!(
        focused_panel.contains(&moved_pin.center()),
        "the location pin should move into the newly focused panel"
    );
}

/// Clicking an empty minimap track switches to that strip, mirroring the
/// workspace bar.
#[gpui::test]
fn clicking_a_minimap_track_switches_strips(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.enable_test_minimap();
        workspace.push_test_panel("one", cx);
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();

    let track = cx
        .debug_bounds("minimap-row-2")
        .expect("every strip should have a track on the map");
    cx.simulate_click(track.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 2,
            "clicking the third track should select strip 3"
        );
    });
}



#[gpui::test]
fn enter_opens_a_session_directly_right_of_the_focused_panel(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);
    let (bridge, commands) = harness::spawn_recording();
    workspace.update(cx, |workspace, _| workspace.bridge = bridge);

    cx.simulate_keystrokes(&format!("{MOD}-t"));
    cx.run_until_parked();
    cx.simulate_keystrokes(&format!("{MOD}-enter"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, cx| workspace.request_deferred_sessions(cx));

    let Command::CreateSession {
        working_dir,
        request_id,
    } = commands.try_recv().unwrap()
    else {
        panic!("Enter must request a session rather than a terminal");
    };
    workspace.update(cx, |workspace, cx| {
        let mut session = session_info("new-session", None);
        session.working_dir = working_dir;
        workspace.apply(
            Update::SessionCreated {
                session,
                request_id,
            },
            cx,
        );
    });
    cx.run_until_parked();

    workspace.update(cx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2, "Enter should open a second panel");
        assert_eq!(workspace.slots[0].panel.session_id(cx), "terminal");
        assert_eq!(workspace.slots[1].panel.session_id(cx), "new-session");
        assert_eq!(
            workspace.active, 1,
            "the new session lands right of the focused panel and takes focus"
        );
    });
}

#[gpui::test]
fn semicolon_requests_a_session_panel(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);

    cx.simulate_keystrokes(&format!("{MOD}-;"));
    cx.run_until_parked();

    workspace.update(cx, |workspace, _cx| {
        assert!(
            workspace.test_coach().trace("new_panel").recalled > 0,
            "the session shortcut should register as the new_panel action"
        );
    });
}

#[gpui::test]
fn the_sidebar_toggles_and_gives_its_width_back_to_the_panels(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);
    let visible_first = workspace.update(cx, |workspace, _| workspace.show_sidebar);

    cx.simulate_keystrokes(&format!("{MOD}-b"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.show_sidebar, !visible_first,
            "the shortcut should flip sidebar visibility"
        );
    });

    cx.simulate_keystrokes(&format!("{MOD}-b"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.show_sidebar, visible_first,
            "toggling twice should restore the original state"
        );
    });

    // Ctrl+Shift+E is the explorer-style alias and must drive the same
    // action, not merely exist in the keymap.
    cx.simulate_keystrokes("ctrl-shift-e");
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.show_sidebar, !visible_first,
            "the ctrl-shift-e alias should toggle the sidebar too"
        );
    });
}

/// Reusable rendered-geometry guard. Checks the actual GPUI layout rather
/// than duplicating positioning formulas or comparing source constants.
fn assert_no_visual_overlap(
    a_name: &str,
    a: gpui::Bounds<gpui::Pixels>,
    b_name: &str,
    b: gpui::Bounds<gpui::Pixels>,
) {
    assert!(
        a.right() <= b.left()
            || b.right() <= a.left()
            || a.bottom() <= b.top()
            || b.bottom() <= a.top(),
        "{a_name} {a:?} overlaps {b_name} {b:?}",
    );
}

#[gpui::test]
fn contextual_tutorial_controls_drive_the_actions_they_depict(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);
    workspace.update(cx, |workspace, cx| {
        workspace.sidebar_view = SidebarView::Learn;
        cx.notify();
    });
    cx.run_until_parked();

    let new_session = cx.debug_bounds("tutorial-new").expect("new session guide");
    let right_guide = cx
        .debug_bounds("tutorial-nav-right")
        .expect("right navigation guide");
    assert_no_visual_overlap("new session", new_session, "right guide", right_guide);
    let dock = cx.debug_bounds("tutorial-guides").unwrap();
    let canvas = cx.debug_bounds("workspace-canvas").unwrap();
    assert_no_visual_overlap("Learn panel", dock, "workspace canvas", canvas);

    cx.simulate_click(new_session.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert!(
            workspace.test_coach().trace("new_panel").recalled > 0,
            "the + guide should dispatch the same learned action as the shortcut"
        );
    });

    // The new session draft already exists without a backend reply. Two
    // terminals give the arrow controls additional synchronous neighbours.
    cx.simulate_keystrokes(&format!("{MOD}-t {MOD}-t"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 3);
        assert_eq!(workspace.active, 2);
        workspace.set_active(0, cx);
        cx.notify();
    });
    cx.run_until_parked();

    let canvas = cx
        .debug_bounds("workspace-canvas")
        .expect("workspace canvas");
    let camera_before = workspace.update(cx, |workspace, _| workspace.camera_target[0]);
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: canvas.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-100.0), px(0.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    workspace.update(cx, |workspace, cx| {
        assert!(
            workspace.camera_target[0] > camera_before,
            "the Learn panel must leave canvas touchpad gestures working"
        );
        // Precise scrolling interpolates camera_x over frames. Present a
        // settled frame without relying on wall-clock scheduling latency.
        workspace.camera_started[0] = Some(Instant::now() - TOUCH_PAN_DURATION);
        cx.notify();
    });
    cx.run_until_parked();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    workspace.read_with(cx, |workspace, _| {
        assert!(
            workspace.camera_x[0] > camera_before,
            "the Learn panel must leave the canvas visibly panned"
        );
    });

    let right = cx
        .debug_bounds("tutorial-nav-right")
        .expect("right arrow guide");
    cx.simulate_click(right.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(workspace.active, 1, "the right arrow should focus right");
    });

    let left = cx
        .debug_bounds("tutorial-nav-left")
        .expect("left arrow guide");
    cx.simulate_click(left.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(workspace.active, 0, "the left arrow should focus left");
    });

    cx.simulate_keystrokes(&format!("{MOD}-l"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.active, 1,
            "the shortcut should restore right focus"
        );
    });

    // Put the focused panel on the second strip, then traverse both ways
    // using the rendered tutorial arrows rather than keyboard dispatch.
    cx.simulate_keystrokes(&format!("{MOD}-shift-j"));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| assert_eq!(workspace.active_row, 1));

    let up = cx.debug_bounds("tutorial-nav-up").expect("up arrow guide");
    cx.simulate_click(up.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 0,
            "the up arrow should focus the strip above"
        );
    });

    let down = cx
        .debug_bounds("tutorial-nav-down")
        .expect("down arrow guide");
    cx.simulate_click(down.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 1,
            "the down arrow should focus the strip below"
        );
    });
}

#[gpui::test]
fn tutorial_lists_basics_and_arrangement_and_marks_learning(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);
    workspace.update(cx, |workspace, cx| {
        workspace.sidebar_view = SidebarView::Learn;
        cx.notify();
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("tutorial-new").is_some());
    assert!(cx.debug_bounds("tutorial-close").is_some());
    assert!(cx.debug_bounds("tutorial-width-presets").is_some());
    assert!(cx.debug_bounds("tutorial-resize").is_some());

    let now = learning::now();
    workspace.update(cx, |workspace, cx| {
        workspace
            .test_coach_mut()
            .used_shortcut("focus_left_right", now);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("tutorial-learned-focus_left_right")
            .is_some(),
        "learned controls should expose their green completion state"
    );

    workspace.update(cx, |workspace, cx| {
        for skill in ["focus_up_down", "new_panel", "close_panel"] {
            workspace.test_coach_mut().used_shortcut(skill, now);
        }
        cx.notify();
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("tutorial-next").is_none());
    assert!(cx.debug_bounds("tutorial-back").is_none());
    assert!(cx.debug_bounds("tutorial-new").is_some());
    assert!(cx.debug_bounds("tutorial-close").is_some());
    assert!(cx.debug_bounds("tutorial-width-presets").is_some());
    assert!(cx.debug_bounds("tutorial-move-left").is_some());
    assert!(cx.debug_bounds("tutorial-maximize").is_some());
}

#[gpui::test]
fn onboarding_is_not_painted_over_the_workspace(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) = focused_workspace(cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("tutorial-guides").is_none());
    workspace.update(cx, |workspace, cx| {
        for skill in ONBOARDING_SKILLS {
            workspace.coach.used_shortcut(skill, learning::now());
        }
        workspace.sidebar_view = SidebarView::Learn;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("tutorial-guides").is_some(),
        "Learn remains available as a reference"
    );
}

#[gpui::test]
fn showcase_is_on_by_default_and_paints_motions_with_tutorial_open(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, cx) = focused_workspace(cx);
    workspace.update(cx, |workspace, cx| {
        workspace.sidebar_view = SidebarView::Learn;
        cx.notify();
    });
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| assert!(workspace.showcase_mode));

    workspace.update(cx, |workspace, cx| {
        workspace.showcase_motion("L", false, "Focus right", cx);
        let cue = workspace.showcase_cue.as_ref().expect("showcase cue");
        assert_eq!(cue.action, "Focus right");
        assert_eq!(cue.tutorial_group, "navigate");
        assert_eq!(
            cue.shortcut,
            if cfg!(target_os = "macos") {
                "Cmd + L"
            } else {
                "Super + L"
            }
        );
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("showcase-shortcut").is_some(),
        "showcase motions must stay visible even with the tutorial open"
    );
    assert!(
        cx.debug_bounds("tutorial-guides").is_some(),
        "tutorial mode should keep contextual controls visible"
    );
    assert!(
        cx.debug_bounds("tutorial-nav-left").is_some()
            && cx.debug_bounds("tutorial-nav-right").is_some(),
        "navigation should use directional arrow controls"
    );
    assert!(
        cx.debug_bounds("tutorial-new").is_some()
            && cx.debug_bounds("tutorial-close").is_some()
            && cx.debug_bounds("tutorial-resize").is_some(),
        "all navigation, session, and width lessons should be listed together"
    );
    assert!(
        cx.debug_bounds("showcase-key").is_some()
            && cx.debug_bounds("showcase-action").is_some(),
        "showcase feedback must include both the shortcut and its action"
    );

    cx.simulate_keystrokes(&format!("{MOD}-b"));
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.showcase_cue.as_ref().map(|cue| cue.action),
            Some("Focus right"),
            "an unrelated shortcut must not replace the displayed motion"
        );
    });

    // A real focus-left keystroke, through the actual keymap, must produce
    // the matching cue even when focus cannot move (no panels exist).
    cx.simulate_keystrokes(&format!("{MOD}-h"));
    workspace.update(cx, |workspace, _| {
        let cue = workspace.showcase_cue.as_ref().expect("focus-left cue");
        assert_eq!(cue.action, "Focus left");
        assert_eq!(
            cue.shortcut,
            if cfg!(target_os = "macos") {
                "Cmd + H"
            } else {
                "Super + H"
            }
        );
    });

    cx.simulate_keystrokes(&format!("{MOD}-shift-s"));
    workspace.update(cx, |workspace, _| {
        assert!(!workspace.showcase_mode);
        assert!(workspace.showcase_cue.is_none());
    });
}

#[gpui::test]
fn showcase_repeated_press_restarts_animation_and_cancels_old_expiry(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, cx) = focused_workspace(cx);
    cx.simulate_keystrokes(&format!("{MOD}-h"));
    let old_start = Instant::now() - Duration::from_secs(1);
    workspace.update(cx, |workspace, _| {
        workspace.showcase_cue.as_mut().unwrap().started_at = Some(old_start);
    });
    cx.executor().advance_clock(Duration::from_millis(1000));
    cx.simulate_keystrokes(&format!("{MOD}-h"));
    workspace.update(cx, |workspace, _| {
        let cue = workspace.showcase_cue.as_ref().unwrap();
        assert_eq!(cue.action, "Focus left");
        assert!(cue.started_at.unwrap() > old_start);
    });
    cx.executor().advance_clock(Duration::from_millis(850));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| assert!(workspace.showcase_cue.is_some()));
    // The fade wakeup must leave the cue mounted, not dismiss it early.
    cx.executor().advance_clock(Duration::from_millis(670));
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| assert!(workspace.showcase_cue.is_some()));
    cx.executor().advance_clock(SHOWCASE_FADE);
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| assert!(workspace.showcase_cue.is_none()));
}

#[gpui::test]
fn super_t_opens_and_paints_a_plain_terminal_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let workspace = Workspace::for_test(learning::Coach::new(), cx);
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    cx.simulate_keystrokes("super-t");
    cx.run_until_parked();

    workspace.update(cx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 1);
        assert_eq!(workspace.slots[0].panel.session_id(cx), "terminal");
    });
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("plain-terminal").is_some(),
        "the terminal surface should paint in the newly created panel"
    );

    // UI-only tests use an inert host. The host resource suite separately
    // creates a real PTY, reattaches a second generation, writes a command,
    // and verifies replayed output without opening a window.
    workspace.update(cx, |workspace, _| {
        assert!(
            workspace.slots[0].panel.terminal().is_some(),
            "terminal parser should exist even when the test host has no PTY"
        );
    });

    // Global workspace actions must continue to bubble while the terminal
    // owns keyboard focus rather than being swallowed as shell input.
    cx.simulate_keystrokes("super-o");
    cx.run_until_parked();
    workspace.update(cx, |workspace, _| {
        assert!(
            workspace.overview,
            "Super+O should still open overview from a terminal"
        );
    });
}

#[gpui::test]
fn super_shift_g_opens_and_paints_the_gmail_applet_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|window, cx| {
        let workspace = Workspace::for_test(learning::Coach::new(), cx);
        let _ = window;
        workspace
    });
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });

    cx.simulate_keystrokes("super-shift-g");
    workspace.update(cx, |workspace, _| {
        assert!(
            workspace.slots.is_empty(),
            "the panel waits for the provider"
        );
    });
    fake_gmail_provider(cx);
    sync_applets(&workspace, cx);

    workspace.update(cx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 1);
        assert_eq!(
            workspace.slots[0].panel.session_id(cx),
            "applet://gmail#launch0"
        );
    });
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    assert!(
        cx.debug_bounds("applet-body").is_some(),
        "the Gmail applet should paint in the newly created panel"
    );
    // Opening the public shortcut again focuses the existing inbox instead
    // of creating duplicate panels.
    cx.simulate_keystrokes("super-shift-g");
    workspace.update(cx, |workspace, _| {
        assert_eq!(workspace.slots.len(), 1);
    });
}

#[gpui::test]
fn theme_tab_and_shortcut_work_with_the_composer_focused(cx: &mut gpui::TestAppContext) {
    let _theme = crate::theme::test_theme_lock();
    cx.update(crate::bind_workspace_keys);
    let original = Theme::active_preset();
    Theme::select(ThemePreset::WarmNeutral);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("theme-test", cx);
        cx.notify();
    });
    vcx.run_until_parked();

    assert!(vcx.debug_bounds("theme-picker-button").is_none());
    click_sidebar_navigation(&workspace, vcx, "sidebar-theme-tab");
    assert!(vcx.debug_bounds("theme-settings").is_some());
    assert!(vcx.debug_bounds("sidebar-session-list").is_none());
    let selectors = [
        "theme-preset-0",
        "theme-preset-1",
        "theme-preset-2",
        "theme-preset-3",
        "theme-preset-4",
        "theme-preset-5",
        "theme-preset-6",
        "theme-preset-7",
        "theme-preset-8",
        "theme-preset-9",
        "theme-preset-10",
        "theme-preset-11",
        "theme-preset-12",
        "theme-preset-13",
        "theme-preset-14",
        "theme-preset-15",
        "theme-preset-16",
        "theme-preset-17",
        "theme-preset-18",
    ];
    assert_eq!(selectors.len(), ThemePreset::ALL.len());
    for (selector, preset) in selectors.into_iter().zip(ThemePreset::ALL) {
        let bounds = vcx
            .debug_bounds(selector)
            .expect("every preset has a picker row");
        vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(Theme::active_preset(), preset);
    }
    let light = vcx
        .debug_bounds("theme-preset-3")
        .expect("all presets paint");
    vcx.simulate_click(light.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(Theme::active_preset(), ThemePreset::NeutralLight);
    assert!(vcx.debug_bounds("theme-settings").is_some());
    assert!(vcx.debug_bounds("theme-picker-button").is_none());

    // Workspace::new focuses the active panel's PromptInput. The global
    // action must bubble out of that focused composer rather than typing.
    vcx.simulate_keystrokes("super-shift-t");
    vcx.run_until_parked();
    assert_eq!(Theme::active_preset(), ThemePreset::NeutralLight.next());
    Theme::select(original);
}

#[gpui::test]
fn settings_default_send_mode_controls_active_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("send-mode", cx);
        workspace
    });
    vcx.run_until_parked();
    click_sidebar_navigation(&workspace, vcx, "sidebar-settings-tab");
    let settings = vcx.debug_bounds("workspace-settings").unwrap();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: settings.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-600.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    let after = vcx
        .debug_bounds("settings-send-after-yield")
        .expect("settings choice");
    vcx.simulate_click(after.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(workspace.read_with(vcx, |w, cx| {
        w.slots[w.active].panel.read(cx).default_send_after_yield()
    }));
    let input =
        workspace.read_with(vcx, |w, cx| w.slots[w.active].panel.read(cx).input.clone());
    workspace.update(vcx, |w, cx| {
        w.slots[w.active]
            .panel
            .update(cx, |p, _| p.status = "running".into())
    });
    vcx.update(|window, cx| {
        let focus = input.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.simulate_input("settings decides this prompt");
    vcx.simulate_keystrokes("enter");
    workspace.read_with(vcx, |w, cx| {
        let queue =
            serde_json::to_value(w.slots[w.active].panel.read(cx).snapshot(cx).prompt_queue)
                .unwrap();
        assert_eq!(
            queue["prompts"][0]["content"],
            "settings decides this prompt"
        );
        assert_eq!(queue["prompts"][0]["after_yield"], true);
    });
    let steer = vcx.debug_bounds("settings-send-steer").unwrap();
    vcx.simulate_click(steer.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(!workspace.read_with(vcx, |w, cx| {
        w.slots[w.active].panel.read(cx).default_send_after_yield()
    }));
}

#[gpui::test]
fn settings_tab_toggles_window_preferences_and_preserves_theme_tab(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("settings-test", cx);
        workspace
    });
    vcx.run_until_parked();
    click_sidebar_navigation(&workspace, vcx, "sidebar-settings-tab");
    assert!(vcx.debug_bounds("workspace-settings").is_some());
    assert!(vcx.debug_bounds("theme-settings").is_none());
    assert!(vcx.debug_bounds("sidebar-session-list").is_none());
    let initial_layout = workspace.read_with(vcx, |w, _| w.layout_mode);
    let button = vcx.debug_bounds("settings-folder-tabs").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_ne!(
        workspace.read_with(vcx, |w, _| w.layout_mode),
        initial_layout
    );
    let button = vcx.debug_bounds("settings-folder-tabs").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.layout_mode),
        initial_layout
    );
    for selector in ["settings-minimap", "settings-showcase"] {
        let initial = workspace.read_with(vcx, |w, _| (w.show_minimap, w.showcase_mode));
        for _ in 0..2 {
            let before = workspace.read_with(vcx, |w, _| (w.show_minimap, w.showcase_mode));
            let button = vcx.debug_bounds(selector).unwrap();
            vcx.simulate_click(button.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            let after = workspace.read_with(vcx, |w, _| (w.show_minimap, w.showcase_mode));
            if selector == "settings-minimap" {
                assert_eq!(after, (!before.0, before.1));
                assert_eq!(vcx.debug_bounds("minimap").is_some(), after.0);
            } else {
                assert_eq!(after, (before.0, !before.1));
            }
        }
        assert_eq!(
            workspace.read_with(vcx, |w, _| (w.show_minimap, w.showcase_mode)),
            initial
        );
    }
    click_sidebar_navigation(&workspace, vcx, "sidebar-theme-tab");
    assert!(vcx.debug_bounds("workspace-settings").is_none());
    assert!(vcx.debug_bounds("theme-settings").is_some());
    click_sidebar_navigation(&workspace, vcx, "sidebar-sessions-tab");
    assert!(vcx.debug_bounds("theme-settings").is_none());
    assert!(vcx.debug_bounds("theme-picker-button").is_none());
}
