//! Workspace tests: folder_strip.
use super::*;

#[gpui::test]
fn folder_picker_wheel_scrolls_back_up_over_an_empty_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let root = tempfile::tempdir().unwrap();
    for index in 0..60 {
        std::fs::create_dir(root.path().join(format!("dir_{index:02}"))).unwrap();
    }
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("empty", cx);
        workspace
    });
    workspace.update_in(vcx, |workspace, window, cx| {
        workspace.open_folder(&OpenFolder, window, cx);
        workspace.browse_picker_to(root.path().to_path_buf(), cx);
    });
    vcx.run_until_parked();
    let list = vcx.debug_bounds("folder-picker-list").unwrap();
    assert!(vcx.debug_bounds("folder-picker-entry-59").is_some());
    let parent_top = |cx: &mut gpui::VisualTestContext| {
        cx.debug_bounds("folder-picker-parent").unwrap().top()
    };
    let wheel = |cx: &mut gpui::VisualTestContext, lines| {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Lines(gpui::point(0.0, lines)),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
    };
    let top = parent_top(vcx);
    wheel(vcx, -6.0);
    let scrolled = parent_top(vcx);
    assert!(scrolled < top, "wheel down should move the folder rows");
    wheel(vcx, 3.0);
    assert!(
        parent_top(vcx) > scrolled,
        "wheel up should scroll the folder rows back"
    );
    workspace.read_with(vcx, |workspace, _| assert_eq!(workspace.active_row, 0));
}

#[gpui::test]
fn folder_picker_shows_only_local_children_and_nested_recents(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("browse");
    let child = base.join("child");
    let nested = child.join("nested");
    let outside = root.path().join("browse-other");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update_in(vcx, |workspace, window, cx| {
        for (id, path) in [("nested", &nested), ("self", &base), ("outside", &outside)] {
            let mut session = session_info(id, None);
            session.working_dir = Some(path.to_string_lossy().into_owned());
            workspace.sessions.push(session);
        }
        workspace.open_folder(&OpenFolder, window, cx);
        workspace.browse_picker_to(base.clone(), cx);
    });
    vcx.run_until_parked();
    let first = vcx
        .debug_bounds("folder-picker-entry-0")
        .expect("nested recent");
    assert!(
        vcx.debug_bounds("folder-picker-entry-1").is_some(),
        "direct child is listed"
    );
    assert!(
        vcx.debug_bounds("folder-picker-entry-2").is_none(),
        "neither the current folder nor an unrelated session folder belongs in the list"
    );
    vcx.simulate_click(first.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.picker_dir(cx).as_deref(), Some(nested.as_path()));
    });
}

#[gpui::test]
fn ctrl_o_uses_the_in_app_picker_and_creates_a_session(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(move |_window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("ctrl-o");
    vcx.run_until_parked();
    assert!(
        !vcx.did_prompt_for_paths(),
        "directory selection must never invoke an OS path dialog"
    );
    assert!(
        vcx.debug_bounds("folder-picker-overlay").is_some(),
        "the picker should paint inside the Jcode window"
    );

    let root = std::env::temp_dir().join(format!(
        "jcode-picker-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let selected = root.join("alpha");
    std::fs::create_dir_all(&selected).unwrap();
    std::fs::create_dir(root.join("beta")).unwrap();
    std::fs::write(root.join("not-a-directory.txt"), "ignored").unwrap();

    workspace.update(vcx, |workspace, cx| {
        workspace.with_picker(cx, |picker| picker.dir = root.clone());
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("folder-picker-parent").is_some(),
        "the parent directory must be available without a search"
    );
    assert!(vcx.debug_bounds("folder-picker-home").is_some());
    assert!(vcx.debug_bounds("folder-picker-computer").is_some());
    vcx.simulate_keystrokes("a l p h a");
    vcx.run_until_parked();
    let first_folder = vcx
        .debug_bounds("folder-picker-entry-0")
        .expect("the first child directory should paint");
    vcx.simulate_click(first_folder.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        assert_eq!(
            workspace.picker_dir(cx).as_deref(),
            Some(selected.as_path())
        );
    });

    let cancel = vcx
        .debug_bounds("folder-picker-cancel")
        .expect("cancel should paint");
    vcx.simulate_click(cancel.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        assert!(workspace.picker_dir(cx).is_none());
    });

    vcx.simulate_keystrokes("ctrl-o");
    workspace.update(vcx, |workspace, cx| {
        workspace.with_picker(cx, |picker| picker.dir = selected.clone());
        cx.notify();
    });
    vcx.run_until_parked();
    let open = vcx
        .debug_bounds("folder-picker-open")
        .expect("open this folder should paint");
    vcx.simulate_click(open.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(commands.try_recv().is_err(), "a draft creates nothing yet");
    workspace.update(vcx, |workspace, cx| workspace.request_deferred_sessions(cx));

    match commands
        .try_recv()
        .expect("folder selection should create a session")
    {
        Command::CreateSession { working_dir, .. } => {
            assert_eq!(
                working_dir.as_deref(),
                Some(selected.to_string_lossy().as_ref())
            );
        }
        _ => panic!("folder selection sent the wrong runtime command"),
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn folder_picker_dismissal_restores_navigation_focus(cx: &mut gpui::TestAppContext) {
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

    // Both cancellation and submission unmount the focused search input.
    // Submission must not depend on a future runtime reply to repair focus.
    for button in ["folder-picker-cancel", "folder-picker-open", "enter"] {
        vcx.simulate_keystrokes("ctrl-o");
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let search = workspace.read(cx).picker_search(cx).unwrap();
            assert!(search.read(cx).focus_handle.is_focused(window));
        });
        if button == "enter" {
            vcx.simulate_keystrokes(". enter");
        } else {
            let bounds = vcx.debug_bounds(button).expect("picker button must render");
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
        }
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("folder-picker-overlay").is_none());
        let focused = workspace.read_with(vcx, |workspace, _| workspace.active);
        vcx.update(|window, cx| {
            assert_eq!(
                workspace.read(cx).navigation_state(window, cx)["keyboard_panel"],
                focused,
                "{button} must focus the selected panel, including a newly created draft"
            );
        });
        for (chord, expected) in [
            ("super-h", focused - 1),
            ("super-l", focused),
            ("super-l", focused + 1),
            ("super-h", focused),
        ] {
            vcx.simulate_keystrokes(chord);
            vcx.run_until_parked();
            vcx.update(|window, cx| {
                let workspace = workspace.read(cx);
                assert_eq!(workspace.active, expected, "{chord} after {button}");
                assert_eq!(
                    workspace.navigation_state(window, cx)["keyboard_panel"],
                    expected
                );
            });
        }
    }
}

#[test]
fn directory_browser_lists_only_sorted_directories_and_reports_missing_paths() {
    let root = std::env::temp_dir().join(format!("jcode-picker-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("zeta")).unwrap();
    std::fs::create_dir(root.join("Alpha")).unwrap();
    std::fs::create_dir(root.join(".hidden")).unwrap();
    std::fs::write(root.join("file.txt"), "ignored").unwrap();

    let names = directory_entries(&root)
        .unwrap()
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Alpha", "zeta"]);
    assert!(directory_entries(&root.join("missing")).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_picker_search_exposes_every_directory_not_only_likely_ones() {
    let root =
        std::env::temp_dir().join(format!("jcode-picker-unrestricted-{}", std::process::id()));
    let ordinary = root.join("an-arbitrary-folder");
    let hidden = root.join(".hidden").join("recent-project");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&ordinary).unwrap();
    std::fs::create_dir_all(&hidden).unwrap();

    let mut hidden_session = session_info("session_fox_hidden", None);
    hidden_session.working_dir = Some(hidden.to_string_lossy().into_owned());
    let entries = ranked_folder_matches_for_sessions(&[hidden_session], &root, "");
    assert!(entries.contains(&(ordinary, String::new())));
    assert!(!entries.iter().any(|(path, _)| path == &hidden));
    assert!(filesystem_root().is_absolute());

    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn ctrl_o_focuses_search_and_enter_opens_the_matching_folder(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(move |_window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });

    let root = std::env::temp_dir().join(format!("jcode-picker-search-{}", std::process::id()));
    let selected = root.join("alpha-project");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&selected).unwrap();
    std::fs::write(root.join("project-notes.md"), "test").unwrap();

    vcx.simulate_keystrokes("ctrl-o");
    workspace.update(vcx, |workspace, cx| {
        workspace.with_picker(cx, |picker| picker.dir = root.clone());
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("folder-picker-search").is_some());
    vcx.update(|window, cx| {
        let search = workspace.read(cx).picker_search(cx).unwrap();
        assert!(search.read(cx).focus_handle.is_focused(window));

        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    let search_bounds = vcx.debug_bounds("folder-picker-search").unwrap();
    vcx.simulate_click(search_bounds.center(), gpui::Modifiers::default());
    vcx.update(|window, cx| {
        let search = workspace.read(cx).picker_search(cx).unwrap();
        assert!(search.read(cx).focus_handle.is_focused(window));
    });

    vcx.simulate_keystrokes("n o t e s enter");
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| workspace.request_deferred_sessions(cx));
    match commands
        .try_recv()
        .expect("enter should open the search match")
    {
        Command::CreateSession { working_dir, .. } => {
            assert_eq!(
                working_dir.as_deref(),
                Some(root.to_string_lossy().as_ref())
            );
        }
        _ => panic!("search submitted the wrong runtime command"),
    }
    workspace.update(vcx, |workspace, cx| {
        assert!(workspace.picker_dir(cx).is_none());
        assert!(workspace.picker_search(cx).is_none());
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn picker_starts_with_ranked_likely_recent_and_frequent_directories() {
    let root = std::env::temp_dir().join(format!("jcode-picker-ranked-{}", std::process::id()));
    let projects = root.join("projects");
    let recent = root.join("recent-repo");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::create_dir_all(&recent).unwrap();
    std::fs::write(root.join("important-notes.md"), "test").unwrap();

    let mut sessions = Vec::new();
    for id in ["one", "two"] {
        let mut session = session_info(id, None);
        session.working_dir = Some(recent.to_string_lossy().into_owned());
        sessions.push(session);
    }
    let workspace = WorkspaceRankFixture { sessions };
    let entries = ranked_folder_matches_for_sessions(&workspace.sessions, &root, "");
    assert_eq!(entries[0], (recent.clone(), "frequent · 2 sessions".into()));
    assert!(entries.contains(&(projects, "likely".into())));

    let searched = ranked_folder_matches_for_sessions(&workspace.sessions, &root, "proj");
    assert!(searched.iter().any(|(path, _)| path.ends_with("projects")));
    let file_match = ranked_folder_matches_for_sessions(&workspace.sessions, &root, "notes");
    assert_eq!(file_match[0].0, root);
    assert!(file_match[0].1.starts_with("contains file"));
    std::fs::remove_dir_all(root).unwrap();
}

struct WorkspaceRankFixture {
    sessions: Vec<jcode_sdk::SessionInfo>,
}

#[test]
fn new_panel_lands_right_of_the_focused_one() {
    // Focused panel is index 1 of a three-panel strip: the new panel takes
    // index 2, pushing the old index 2 to the right.
    assert_eq!(insert_index(1, true, Some(2), 3), 2);
    // Focused panel is the rightmost: append.
    assert_eq!(insert_index(2, true, Some(2), 3), 3);
    // Focus is on another strip: append after that strip's last panel.
    assert_eq!(insert_index(0, false, Some(4), 6), 5);
    // Empty strip: append at the end of the slot list.
    assert_eq!(insert_index(0, false, None, 3), 3);
}

#[gpui::test]
fn session_shortcuts_use_the_directory_pinned_once(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let root = tempfile::tempdir().unwrap();
    let favorite = root.path().join("favorite");
    let other = root.path().join("other");
    std::fs::create_dir(&favorite).unwrap();
    std::fs::create_dir(&other).unwrap();
    let session = |id: &str, path: &Path| {
        let mut session = session_info(id, None);
        session.working_dir = Some(path.to_string_lossy().into_owned());
        session
    };
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("existing", cx);
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    session("one", &favorite),
                    session("two", &favorite),
                    session("three", &other),
                ],
            },
            cx,
        );
        // Later usage must not move the shortcut, even when it overtakes
        // the original favorite entirely.
        workspace.apply(
            Update::Sessions {
                sessions: vec![session("four", &other)],
            },
            cx,
        );
        assert_eq!(workspace.pinned_working_dir.as_deref(), favorite.to_str());
        workspace
    });
    // Dispatch with the prompt focused, not just the workspace root.
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_active(window, cx))
    });
    vcx.run_until_parked();
    for (chord, expected) in [
        ("super-enter", Some(favorite.to_string_lossy().into_owned())),
        (
            "ctrl-alt-enter",
            Some(favorite.to_string_lossy().into_owned()),
        ),
        ("super-n", Some(favorite.to_string_lossy().into_owned())),
        ("super-;", Some(favorite.to_string_lossy().into_owned())),
        ("super-'", Some(favorite.to_string_lossy().into_owned())),
    ] {
        vcx.simulate_keystrokes(&platform_chord(chord));
        vcx.run_until_parked();
        assert!(commands.try_recv().is_err(), "{chord} only opens a draft");
        workspace.update(vcx, |workspace, cx| workspace.request_deferred_sessions(cx));
        match commands
            .try_recv()
            .expect("shortcut should create a session")
        {
            Command::CreateSession {
                working_dir,
                request_id,
            } => {
                assert_eq!(working_dir, expected, "{chord}");
                assert!(
                    request_id
                        .as_deref()
                        .is_some_and(Panel::is_pending_session_id)
                );
            }
            _ => panic!("{chord} dispatched the wrong command"),
        }
        assert!(commands.try_recv().is_err(), "one panel per keypress");
    }
}

#[test]
fn most_used_directory_ignores_unusable_paths_and_breaks_ties_stably() {
    let root = tempfile::tempdir().unwrap();
    let alpha = root.path().join("alpha");
    let beta = root.path().join("beta");
    std::fs::create_dir(&alpha).unwrap();
    std::fs::create_dir(&beta).unwrap();
    let mut sessions = Vec::new();
    for path in [
        &beta,
        &alpha,
        &root.path().join("missing"),
        &PathBuf::from("relative"),
    ] {
        let mut session = session_info("test", None);
        session.working_dir = Some(path.to_string_lossy().into_owned());
        sessions.push(session);
    }
    assert_eq!(most_used_working_dir(&sessions).as_deref(), alpha.to_str());
    sessions.reverse();
    assert_eq!(most_used_working_dir(&sessions).as_deref(), alpha.to_str());
    assert_eq!(most_used_working_dir(&[]), None);
}

#[test]
fn closing_focuses_the_right_neighbour() {
    // Closed index 1 of [0,1,2]; remaining strip indices are [0,1] and the
    // panel formerly at 2 now sits at 1, so focus stays at 1.
    assert_eq!(focus_after_close(1, &[0, 1]), 1);
    // Closed the rightmost: fall back to the new rightmost.
    assert_eq!(focus_after_close(2, &[0, 1]), 1);
    // Closed the only panel.
    assert_eq!(focus_after_close(0, &[]), 0);
}

#[test]
fn width_presets_cycle_and_wrap() {
    assert_eq!(next_preset(0.25), 0.5);
    assert_eq!(next_preset(0.5), 0.75);
    assert_eq!(next_preset(0.75), 0.25);
    // A maximized panel wraps to the narrowest preset.
    assert_eq!(next_preset(1.0), 0.25);
}

#[test]
fn maximize_toggles_back_to_the_previous_width() {
    // Maximize a half-width panel, then restore it.
    let (width, restore) = toggle_maximize(0.5, None);
    assert_eq!((width, restore), (1.0, Some(0.5)));
    assert_eq!(toggle_maximize(width, restore), (0.5, None));
    // A quarter-width panel restores to a quarter.
    let (width, restore) = toggle_maximize(0.25, None);
    assert_eq!(toggle_maximize(width, restore), (0.25, None));
    // Already full width with nothing stored: toggle to the default so the
    // key is never a no-op.
    assert_eq!(toggle_maximize(1.0, None), (DEFAULT_WIDTH, None));
    // Round trip from there still works.
    let (width, restore) = toggle_maximize(DEFAULT_WIDTH, None);
    assert_eq!(width, 1.0);
    assert_eq!(toggle_maximize(width, restore).0, DEFAULT_WIDTH);
}

#[test]
fn strip_description_reports_focus_and_widths() {
    assert_eq!(
        describe_strip(&[0.5, 0.25], Some(1), 2),
        "strip=2 focus=1 widths=0.50,0.25"
    );
    // An empty strip has no focused position.
    assert_eq!(describe_strip(&[], None, 0), "strip=0 focus=- widths=");
}

#[test]
fn two_finger_pan_moves_and_clamps_the_camera() {
    // Total content 2000 wide in a 1000 viewport: pan freely inside range.
    assert_eq!(pan_camera(100.0, 250.0, 2000.0, 1000.0), 350.0);
    // Panning left of the start clamps at the strut.
    assert_eq!(pan_camera(50.0, -500.0, 2000.0, 1000.0), -STRUT);
    // Panning past the end clamps at the last panel's right edge.
    assert_eq!(pan_camera(900.0, 500.0, 2000.0, 1000.0), 1000.0);
    // Content narrower than the viewport cannot pan at all.
    assert_eq!(pan_camera(0.0, 300.0, 500.0, 1000.0), -GAP);
}

#[test]
fn touchpad_pan_filters_frame_jitter_then_settles_exactly() {
    let target = 100.0;
    let first_frame =
        smoothed_camera_position(0.0, target, Duration::from_millis(8), TOUCH_PAN_DURATION);
    let second_frame =
        smoothed_camera_position(0.0, target, Duration::from_millis(16), TOUCH_PAN_DURATION);

    assert!(first_frame > 0.0 && first_frame < target);
    assert!(second_frame > first_frame && second_frame < target);
    assert_eq!(
        smoothed_camera_position(0.0, target, TOUCH_PAN_DURATION, TOUCH_PAN_DURATION),
        target,
        "smoothing must never lose touchpad travel"
    );
}

/// Public acceptance path: a precise scroll event enters through GPUI and
/// the rendered panel position advances over frames instead of jumping.
#[gpui::test]
fn precise_horizontal_scroll_moves_rendered_panels_smoothly(cx: &mut gpui::TestAppContext) {
    let (_workspace, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for name in ["one", "two", "three"] {
            workspace.push_test_panel(name, cx);
        }
        workspace
    });
    cx.run_until_parked();

    let initial = cx
        .debug_bounds("panel-0")
        .expect("the first panel should paint");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: initial.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-120.), px(0.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });

    std::thread::sleep(Duration::from_millis(8));
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    let first_frame = cx
        .debug_bounds("panel-0")
        .expect("the panning panel should remain rendered");
    let first_travel = f32::from(initial.origin.x - first_frame.origin.x);
    assert!(
        first_travel > 0.0 && first_travel < 120.0,
        "the first presented frame should interpolate rather than jump: {first_travel}"
    );

    std::thread::sleep(TOUCH_PAN_DURATION);
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    let settled = cx
        .debug_bounds("panel-0")
        .expect("the settled panel should remain rendered");
    let settled_travel = f32::from(initial.origin.x - settled.origin.x);
    assert!(
        (settled_travel - 120.0).abs() < 0.5,
        "the rendered canvas must settle at the full gesture distance: {settled_travel}"
    );
}

#[test]
fn touchpad_camera_focuses_the_panel_nearest_the_viewport_center() {
    let viewport = 1200.0;
    let panels = || [(0, 0.5), (1, 0.5), (2, 0.5)];

    assert_eq!(
        panel_at_viewport_center(panels(), -STRUT, viewport),
        Some(0)
    );
    assert_eq!(panel_at_viewport_center(panels(), 400.0, viewport), Some(1));
    assert_eq!(panel_at_viewport_center(panels(), 900.0, viewport), Some(2));
    assert_eq!(panel_at_viewport_center([], 0.0, viewport), None);
    // Compact widths use full-size panels, including when hit-testing pans.
    assert_eq!(panel_at_viewport_center(panels(), 400.0, 1000.0), Some(0));
    assert_eq!(panel_at_viewport_center(panels(), 900.0, 1000.0), Some(1));
}

/// The reticle holds fully lit while deltas keep arriving, fades linearly
/// once they stop, and disappears entirely after the fade.
#[test]
fn the_gesture_reticle_holds_then_fades_then_vanishes() {
    assert_eq!(gesture_alpha(Duration::ZERO), Some(1.0));
    assert_eq!(gesture_alpha(GESTURE_HOLD), Some(1.0));
    let mid =
        gesture_alpha(GESTURE_HOLD + GESTURE_FADE / 2).expect("mid-fade must still be visible");
    assert!(
        (mid - 0.5).abs() < 0.01,
        "halfway through the fade should be about half lit: {mid}"
    );
    assert_eq!(gesture_alpha(GESTURE_HOLD + GESTURE_FADE), None);
    assert_eq!(gesture_alpha(Duration::from_secs(60)), None);
}

/// A gesture that starts vertically belongs to the transcript for its
/// whole lifetime: no pan, no strip hop, even if it later drifts sideways.
#[test]
fn a_vertical_first_gesture_stays_with_the_panel() {
    let mut gesture = StripGesture::default();
    assert_eq!(
        gesture.feed(0.0, -40.0, gpui::TouchPhase::Started),
        Routed::ToPanel
    );
    assert_eq!(gesture.axis, GestureAxis::Vertical);
    // Sideways drift after the lock must not start panning.
    assert_eq!(
        gesture.feed(-80.0, -10.0, gpui::TouchPhase::Moved),
        Routed::ToPanel
    );
    assert_eq!(
        gesture.feed(0.0, -300.0, gpui::TouchPhase::Moved),
        Routed::ToPanel
    );
}

/// A gesture that starts horizontally owns the strip exclusively, and
/// enough vertical pull breaks the sticky axis and hops one strip per
/// gesture, in the natural direction (fingers up reveal the strip
/// below), without ever leaking a delta into the transcript.
#[test]
fn a_horizontal_gesture_breaks_out_vertically_one_strip_per_gesture() {
    let mut gesture = StripGesture::default();
    assert_eq!(
        gesture.feed(-40.0, 0.0, gpui::TouchPhase::Started),
        Routed::Strip {
            dx: -40.0,
            switch: 0,
            exclusive: true
        }
    );
    assert_eq!(gesture.axis, GestureAxis::Horizontal);
    // Pull short of the threshold: consumed, but no hop yet.
    assert_eq!(
        gesture.feed(0.0, -STRIP_BREAK * 0.6, gpui::TouchPhase::Moved),
        Routed::Strip {
            dx: 0.0,
            switch: 0,
            exclusive: true
        }
    );
    // Crossing the threshold hops down and latches.
    assert_eq!(
        gesture.feed(0.0, -STRIP_BREAK * 0.6, gpui::TouchPhase::Moved),
        Routed::Strip {
            dx: 0.0,
            switch: 1,
            exclusive: true
        }
    );
    // Even a direction reversal cannot trigger a second hop.
    assert_eq!(
        gesture.feed(0.0, STRIP_BREAK * 1.2, gpui::TouchPhase::Moved),
        Routed::Strip {
            dx: 0.0,
            switch: 0,
            exclusive: true
        }
    );
    // Lifting the fingers preserves the latch against trailing momentum.
    gesture.feed(0.0, 0.0, gpui::TouchPhase::Ended);
    assert_eq!(gesture.axis, GestureAxis::Undecided);
    assert_eq!(
        gesture.feed(0.0, -STRIP_BREAK * 3.0, gpui::TouchPhase::Moved),
        Routed::Strip {
            dx: 0.0,
            switch: 0,
            exclusive: true
        }
    );
    // An explicit new gesture decides its axis afresh.
    assert_eq!(
        gesture.feed(0.0, -40.0, gpui::TouchPhase::Started),
        Routed::ToPanel
    );
}

/// Tiny diagonal jitter below the lock threshold keeps the event shared:
/// horizontal motion pans but the transcript still sees the event.
#[test]
fn an_undecided_gesture_shares_its_deltas() {
    let mut gesture = StripGesture::default();
    assert_eq!(
        gesture.feed(-4.0, 3.0, gpui::TouchPhase::Started),
        Routed::Strip {
            dx: -4.0,
            switch: 0,
            exclusive: false
        }
    );
    assert_eq!(gesture.axis, GestureAxis::Undecided);
}

/// Horizontal panning still owns its vertical deltas, but cannot move down
/// a workspace or leak the blocked pull into the panel transcript.
#[gpui::test]
fn a_committed_pan_cannot_move_down_a_workspace(cx: &mut gpui::TestAppContext) {
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
    // A long transcript in the first panel, so a leaked vertical delta
    // would visibly scroll it.
    let panel = workspace
        .read_with(cx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    panel.update(cx, |panel, cx| {
        for n in 0..80 {
            panel
                .items
                .push(crate::panel::Item::Assistant(format!("message {n}")));
        }
        cx.notify();
    });
    cx.run_until_parked();

    let target = cx
        .debug_bounds("panel-0")
        .expect("the first panel should paint")
        .center();
    let swipe = |cx: &mut gpui::VisualTestContext, dx: f32, dy: f32, phase| {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: target,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(dx), px(dy))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: phase,
        });
        cx.run_until_parked();
    };

    // A gap resets a horizontal lock without an Ended event. Exercise it
    // over a transcript, not the empty destination strip where vertical
    // scrolling intentionally navigates rows.
    swipe(cx, -40.0, 0.0, gpui::TouchPhase::Started);
    cx.background_executor
        .advance_clock(GESTURE_RESET + Duration::from_millis(1));
    swipe(cx, 0.0, -STRIP_BREAK * 2.0, gpui::TouchPhase::Moved);
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(workspace.active_row, 0);
        assert_eq!(workspace.gesture.axis, GestureAxis::Vertical);
    });

    // Commit the gesture horizontally, then pull up past the threshold.
    swipe(cx, -40.0, 0.0, gpui::TouchPhase::Started);
    let scroll_before = panel.read_with(cx, |panel, _| panel.test_scroll_offset_y());
    swipe(cx, 0.0, -STRIP_BREAK * 0.6, gpui::TouchPhase::Moved);
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 0,
            "short of the threshold the sticky axis must hold"
        );
    });
    swipe(cx, 0.0, -STRIP_BREAK * 0.6, gpui::TouchPhase::Moved);
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 0,
            "even a large vertical pull must not hop down a strip"
        );
    });
    let scroll_after = panel.read_with(cx, |panel, _| panel.test_scroll_offset_y());
    assert_eq!(
        scroll_before, scroll_after,
        "a committed pan must consume its vertical deltas, not scroll the transcript"
    );
    workspace.update_in(cx, |w, window, cx| {
        w.row_progress
            .sample(Instant::now() + Duration::from_secs(1));
        cx.notify();
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
    for _ in 0..5 {
        swipe(cx, 0.0, -STRIP_BREAK * 2.0, gpui::TouchPhase::Moved);
        assert_eq!(
            workspace.read_with(cx, |w, _| w.active_row),
            0,
            "continued touch travel must not move down a workspace"
        );
    }
}

#[gpui::test]
fn vertical_scroll_over_an_empty_panel_cannot_move_down_a_workspace(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("empty", cx);
        workspace
    });
    let panel = workspace
        .read_with(cx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    panel.update(cx, |panel, cx| {
        panel.items.clear();
        cx.notify();
    });
    cx.run_until_parked();

    let target = cx
        .debug_bounds("panel-0")
        .expect("the empty panel should paint")
        .center();
    for phase in [gpui::TouchPhase::Started, gpui::TouchPhase::Moved] {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: target,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-STRIP_BREAK * 0.6))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: phase,
        });
        cx.run_until_parked();
    }
    workspace.read_with(cx, |workspace, _| {
        assert_eq!(
            workspace.active_row, 0,
            "vertical travel over an empty conversation must not move down a workspace"
        );
    });
}

#[gpui::test]
fn empty_workspace_requires_a_shortcut_to_move_down(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    cx.run_until_parked();
    let target = cx.debug_bounds("empty-strip-hint").unwrap().center();
    for phase in [
        gpui::TouchPhase::Started,
        gpui::TouchPhase::Moved,
        gpui::TouchPhase::Ended,
    ] {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: target,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-STRIP_BREAK * 3.0))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: phase,
        });
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.active_row, 0);
            assert_eq!(w.gesture.pull, 0.0);
        });
    }
    cx.simulate_keystrokes("super-down");
    workspace.update_in(cx, |w, window, cx| {
        assert_eq!(
            w.active_row, 1,
            "the keyboard shortcut must still move down"
        );
        w.row_progress
            .sample(Instant::now() + Duration::from_secs(1));
        cx.notify();
        window.simulate_next_frame(cx);
    });
    cx.run_until_parked();
    let target = cx.debug_bounds("empty-strip-hint").unwrap().center();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: target,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(STRIP_BREAK * 3.0))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    cx.run_until_parked();
    assert_eq!(
        workspace.read_with(cx, |w, _| w.active_row),
        0,
        "upward workspace gestures are unchanged"
    );
}

/// Only the vertical pull during a horizontally locked touchpad gesture
/// paints the reticle and minimap dot. Horizontal movement alone stays
/// visually quiet.
#[gpui::test]
fn a_touchpad_swipe_paints_the_gesture_reticle_and_minimap_dot(cx: &mut gpui::TestAppContext) {
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
    assert!(
        cx.debug_bounds("gesture-reticle").is_none(),
        "no reticle before any gesture"
    );

    let panel = cx
        .debug_bounds("panel-0")
        .expect("the first panel should paint");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: panel.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-120.), px(0.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("gesture-reticle").is_none(),
        "a horizontal swipe must not paint the canvas reticle"
    );
    assert!(
        cx.debug_bounds("minimap-gesture-dot").is_none(),
        "a horizontal swipe must not paint the minimap dot"
    );

    // Once the gesture is horizontally locked, vertical travel pulls
    // toward another strip and should make both indicators visible.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: panel.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(20.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("gesture-reticle").is_some(),
        "vertical pull should paint the canvas reticle"
    );
    assert!(
        cx.debug_bounds("minimap-gesture-dot").is_some(),
        "the swipe should paint the minimap dot"
    );

    // The promise of the reticle: the panel under it is the panel that
    // holds focus. Swipe far enough to hand focus to the second panel
    // (but not to the clamped end of the strip, where the focal point
    // sits exactly on a panel boundary) and the ring must sit inside the
    // focused panel's rectangle.
    for _ in 0..4 {
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: panel.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(-120.), px(0.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
    }
    cx.run_until_parked();
    let focused = workspace.read_with(cx, |workspace, _| workspace.active);
    assert_eq!(
        focused, 1,
        "a 600px swipe over half-width panels should hand focus to the second panel"
    );
    let reticle = cx
        .debug_bounds("gesture-reticle")
        .expect("the reticle stays lit mid-swipe");
    let focused_panel = cx
        .debug_bounds("panel-1")
        .expect("the focused panel should paint");
    assert!(
        focused_panel.contains(&reticle.center()),
        "the reticle must sit over the panel that holds focus: \
         reticle={reticle:?}, panel={focused_panel:?}"
    );

    cx.background_executor
        .advance_clock(GESTURE_HOLD + GESTURE_FADE + Duration::from_millis(50));
    cx.run_until_parked();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| gpui::div(),
    );
    assert!(
        cx.debug_bounds("gesture-reticle").is_none(),
        "the reticle must vanish after the fade"
    );
    assert!(
        cx.debug_bounds("minimap-gesture-dot").is_none(),
        "the minimap dot must vanish after the fade"
    );
}

/// Neither ordinary transcript scrolling nor horizontal minimap panning
/// is a vertical strip pull, so neither should light the indicators.
#[gpui::test]
fn transcript_scrolls_and_minimap_swipes_do_not_light_the_indicators(
    cx: &mut gpui::TestAppContext,
) {
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

    // A vertical wheel over the panel belongs to the transcript, not the
    // pan gesture: no reticle.
    let panel = cx
        .debug_bounds("panel-0")
        .expect("the first panel should paint");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: panel.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(60.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("gesture-reticle").is_none(),
        "vertical transcript scrolling must not light the reticle"
    );

    // A swipe over the minimap pans horizontally without a vertical strip
    // pull, so the indicators remain hidden.
    let map = cx
        .debug_bounds("minimap")
        .expect("the minimap should paint");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: map.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-30.), px(0.))),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("gesture-reticle").is_none(),
        "a minimap swipe must not light the canvas reticle"
    );
    assert!(
        cx.debug_bounds("minimap-gesture-dot").is_none(),
        "a minimap swipe must not light the map dot"
    );
}

#[test]
fn camera_never_centers_and_scrolls_minimally() {
    let viewport = 1000.0;
    // Fully visible: do not move.
    assert_eq!(scroll_into_view(0.0, 100.0, 400.0, viewport), 0.0);
    // Off to the right: bring its right edge to the viewport edge, which is
    // not the centered position (that would be 400.0).
    let target = scroll_into_view(0.0, 900.0, 400.0, viewport);
    assert!((target - (900.0 + 400.0 + STRUT - viewport)).abs() < 0.01);
    // Off to the left: bring its left edge to the viewport edge.
    let target = scroll_into_view(900.0, 100.0, 400.0, viewport);
    assert!((target - (100.0 - STRUT)).abs() < 0.01);
    // Wider than the viewport: pin to its left edge.
    let target = scroll_into_view(0.0, 500.0, 1200.0, viewport);
    assert!((target - (500.0 - STRUT)).abs() < 0.01);
}

#[test]
fn camera_easing_is_monotonic_and_settles() {
    assert_eq!(ease_out_cubic(0.0), 0.0);
    assert_eq!(ease_out_cubic(1.0), 1.0);
    let mut previous = 0.0;
    for step in 1..=10 {
        let value = ease_out_cubic(step as f32 / 10.0);
        assert!(value > previous, "easing must increase at {step}");
        previous = value;
    }
    // Cubic ease-out keeps the movement responsive while distributing it
    // across substantially more of the 150 ms transition.
    assert!((ease_out_cubic(0.1) - 0.271).abs() < 0.001);
    assert!((ease_out_cubic(0.2) - 0.488).abs() < 0.001);
}
