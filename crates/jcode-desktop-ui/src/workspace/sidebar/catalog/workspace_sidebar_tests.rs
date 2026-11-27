//! Workspace tests: sidebar.
use super::*;

/// Includes GPUI's next frame, not just mouse event dispatch. The test
/// platform excludes compositor presentation and physical input latency.
#[gpui::test]
#[ignore = "manual sidebar hover profiler"]
fn sidebar_hover_frame_profile(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    for count in [10, 100, 500] {
        workspace.update(vcx, |workspace, cx| {
            workspace
                .sidebar_closed_sessions
                .extend((0..count).map(|index| format!("session_fox_{index}")));
            workspace.sessions = (0..count)
                .map(|index| {
                    session_info(
                        &format!("session_fox_{index}"),
                        Some(&format!(
                            "Investigate sidebar responsiveness in session {index}"
                        )),
                    )
                })
                .collect();
            cx.notify();
        });
        vcx.run_until_parked();
        let positions = [
            vcx.debug_bounds("sidebar-session-0").unwrap().center(),
            vcx.debug_bounds("sidebar-session-1").unwrap().center(),
        ];
        let mut samples = Vec::new();
        for iteration in 0..60 {
            let started = Instant::now();
            vcx.update(|window, cx| {
                window.simulate_mouse_move(positions[iteration % 2], cx);
            });
            vcx.run_until_parked();
            if iteration >= 10 {
                samples.push(started.elapsed().as_micros());
            }
        }
        samples.sort_unstable();
        println!(
            "SIDEBAR_HOVER sessions={count} p50={}us p95={}us",
            samples[24], samples[47]
        );
    }
}

#[gpui::test]
fn sidebar_hover_only_renders_visible_history(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .extend((0..500).map(|index| format!("session_fox_{index}")));
        workspace.sessions = (0..500)
            .map(|index| session_info(&format!("session_fox_{index}"), Some("Previous work")))
            .collect();
        cx.notify();
    });
    vcx.run_until_parked();
    let position = vcx.debug_bounds("sidebar-session-1").unwrap().center();
    SIDEBAR_TITLE_RENDERS.with(|count| count.set(0));
    vcx.update(|window, cx| window.simulate_mouse_move(position, cx));
    vcx.run_until_parked();
    let rendered = SIDEBAR_TITLE_RENDERS.with(|count| count.get());
    assert!(rendered > 0, "hover must actually produce a frame");
    assert!(rendered < 50, "hover rendered {rendered} of 500 rows");
    assert!(vcx.debug_bounds("sidebar-session-499").is_none());

    workspace.update(vcx, |workspace, cx| {
        workspace.sidebar_sessions_list.scroll_to_end();
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("sidebar-session-499").is_some(),
        "virtualized history remains reachable"
    );
    let last = vcx.debug_bounds("sidebar-session-499").unwrap();
    vcx.simulate_click(last.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(
            workspace.slots[workspace.active].panel.session_id(cx),
            "session_fox_499",
            "a newly visible row must retain its own click target"
        );
    });
}

#[gpui::test]
fn sidebar_virtual_history_keeps_scroll_across_updates(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .extend((0..100).map(|index| format!("session_fox_{index}")));
        workspace.sessions = (0..100)
            .map(|index| session_info(&format!("session_fox_{index}"), Some("Previous work")))
            .collect();
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        workspace.sidebar_sessions_list.scroll_to(gpui::ListOffset {
            item_ix: 50,
            offset_in_item: px(0.0),
        });
        cx.notify();
    });
    vcx.run_until_parked();
    let before = vcx.debug_bounds("sidebar-session-50").unwrap();
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .insert("session_fox_100".into());
        workspace.sessions[50].title = Some("Renamed without changing row height".into());
        workspace
            .sessions
            .push(session_info("session_fox_100", Some("New history")));
        cx.notify();
    });
    vcx.run_until_parked();
    assert_eq!(vcx.debug_bounds("sidebar-session-50").unwrap(), before);
    // Metadata can add a line to an already measured, off-screen row.
    workspace.update(vcx, |workspace, cx| {
        workspace.sessions[0].working_dir = Some("/project".into());
        workspace.sidebar_sessions_list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.0),
        });
        cx.notify();
    });
    vcx.run_until_parked();
    // The directory becomes a project header above the row.
    let header = vcx.debug_bounds("sidebar-project-0").unwrap();
    assert!(header.bottom() <= vcx.debug_bounds("sidebar-session-0").unwrap().top());
    workspace.update(vcx, |workspace, cx| {
        workspace.sessions.clear();
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-session-0").is_none());
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.sidebar_sessions_list.item_count()),
        0
    );
}

#[gpui::test]
fn sidebar_divides_saved_sessions_from_other_sessions(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .extend(["session_fox_saved".into(), "session_owl_previous".into()]);
        let mut saved = session_info("session_fox_saved", Some("saved work"));
        saved.saved = true;
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    saved,
                    session_info("session_owl_previous", Some("previous work")),
                ],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("sidebar-session-divider").is_some(),
        "a horizontal rule should separate saved sessions from other sessions"
    );
}

#[gpui::test]
fn sidebar_separates_open_panels_from_other_sessions(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .insert("session_owl_closed".into());
        workspace.push_test_panel("session_fox_open", cx);
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    session_info("session_owl_closed", Some("previous work")),
                    session_info("session_fox_open", Some("visible work")),
                ],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();

    workspace.read_with(vcx, |workspace, _| {
        assert_eq!(
            workspace.sidebar_session_layout[0].session_id,
            "session_fox_open"
        );
        assert_eq!(workspace.sidebar_session_layout[0].section, Some("Live"));
        assert_eq!(workspace.sidebar_session_layout[1].section, Some("Past"));
        assert!(!workspace.sidebar_session_layout[1].divider);
    });
    assert!(
        vcx.debug_bounds("sidebar-section-1").is_some(),
        "sessions without a panel are labelled apart from open panels"
    );
}

#[gpui::test]
fn sidebar_closing_panels_do_not_reappear_from_cached_history(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for id in ["closing-idle", "closing-connected", "keep"] {
            workspace.push_test_panel(id, cx);
            let mut session = session_info(id, Some(id));
            session.user_connected = id == "closing-connected";
            workspace.sessions.push(session);
        }
        workspace.slots[0].closing = true;
        workspace.slots[1].closing = true;
        workspace.active = 2;
        workspace
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _| {
        assert_eq!(
            workspace
                .sidebar_session_layout
                .iter()
                .map(|row| row.session_id.as_str())
                .collect::<Vec<_>>(),
            ["keep"]
        );
        assert!(workspace.sidebar_session_layout[0].open);
        assert_eq!(
            workspace.sessions.len(),
            3,
            "sidebar suppression must preserve durable history"
        );
    });
}

#[gpui::test]
fn sidebar_external_user_connections_are_live_without_becoming_local_panels(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("local-user", cx);
        workspace
            .slots
            .last()
            .unwrap()
            .panel
            .update(cx, |panel, _| {
                panel.working_dir = Some("/presence/local".into())
            });
        workspace
    });
    for status in ["idle", "active"] {
        workspace.update(vcx, |workspace, cx| {
            let mut external = session_info("external-user", Some("Standalone user"));
            external.working_dir = Some("/presence/external".into());
            external.user_connected = true;
            external.status = status.into();
            workspace
                .sidebar_closed_sessions
                .insert("loaded-history".into());
            let mut history = session_info("loaded-history", Some("Loaded but disconnected"));
            history.working_dir = external.working_dir.clone();
            history.status = "active".into();
            workspace.apply(
                Update::Sessions {
                    sessions: vec![history, external],
                },
                cx,
            );
            cx.notify();
        });
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, _| {
            let external = workspace
                .sidebar_session_layout
                .iter()
                .find(|row| row.session_id == "external-user")
                .unwrap();
            assert_eq!(external.section, Some("Live"));
            assert!(
                !external.open,
                "external presence must not turn into local panel membership"
            );
            assert!(
                !external.collapsed,
                "live external project must not default to collapsed history"
            );
            let history = workspace
                .sidebar_session_layout
                .iter()
                .find(|row| row.session_id == "loaded-history")
                .unwrap();
            assert_eq!(history.section, Some("Past"));
            assert!(!history.open);
            assert_eq!(
                workspace.slots.len(),
                1,
                "listing must not attach to or open external sessions"
            );
        });
    }
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_projects
            .collapsed
            .insert("/presence/external".into(), true);
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _| {
        assert!(
            workspace
                .sidebar_session_layout
                .iter()
                .find(|row| row.session_id == "external-user")
                .unwrap()
                .collapsed,
            "explicit manual collapse must survive connected presence"
        );
    });
}

#[gpui::test]
fn sidebar_session_history_moves_when_the_user_scrolls(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .extend((0..80).map(|index| format!("session_fox_history_{index}")));
        workspace.apply(
            Update::Sessions {
                sessions: (0..80)
                    .map(|index| {
                        session_info(
                            Box::leak(format!("session_fox_history_{index}").into_boxed_str()),
                            Some("previous work"),
                        )
                    })
                    .collect(),
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();

    let before = workspace.read_with(vcx, |workspace, _| {
        workspace
            .sidebar_sessions_list
            .scroll_px_offset_for_scrollbar()
            .y
    });
    let list = vcx
        .debug_bounds("sidebar-session-list")
        .expect("session list should paint");
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -4.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();

    let after = workspace.read_with(vcx, |workspace, _| {
        workspace
            .sidebar_sessions_list
            .scroll_px_offset_for_scrollbar()
            .y
    });
    assert_ne!(after, before, "wheel input must move the session history");
    let scrollbar = vcx
        .debug_bounds("sidebar-scrollbar")
        .expect("overflowing session history should paint a scrollbar");
    assert!(
        scrollbar.right() == list.right() + px(SIDEBAR_SCROLLBAR_OUTSET - 4.0),
        "the thin scrollbar sits in the gap beyond the session tabs"
    );
    assert_eq!(scrollbar.size.width, px(3.0));
    let gutter = vcx.debug_bounds("sidebar-scroll-gutter").unwrap();
    assert_eq!(gutter.right(), list.right() + px(SIDEBAR_SCROLLBAR_OUTSET));
    assert_eq!(gutter.size.width, px(crate::scrollbar::GUTTER));
}

#[gpui::test]
fn sidebar_gutter_scrolls_sessions_and_files_without_moving_tabs(
    cx: &mut gpui::TestAppContext,
) {
    let project = tempfile::tempdir().unwrap();
    for index in 0..80 {
        std::fs::write(project.path().join(format!("file-{index}.rs")), "").unwrap();
    }
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        w.push_test_panel("project", cx);
        w.slots[0].panel.update_chat(cx, |panel, _| {
            panel.working_dir = Some(project.path().display().to_string());
        });
        w.sidebar_closed_sessions
            .extend((0..80).map(|index| format!("session_fox_gutter_{index}")));
        w.apply(
            Update::Sessions {
                sessions: (0..80)
                    .map(|index| {
                        let mut session = session_info(
                            &format!("session_fox_gutter_{index}"),
                            Some("previous work"),
                        );
                        // Same project as the open panel, so history stays expanded.
                        session.working_dir = Some(project.path().display().to_string());
                        session
                    })
                    .collect(),
            },
            cx,
        );
        cx.notify();
    });
    for mode in [
        crate::config::LayoutMode::FolderTabs,
        crate::config::LayoutMode::Normal,
    ] {
        for view in [SidebarView::Sessions, SidebarView::Files] {
            workspace.update(vcx, |w, cx| {
                w.layout_mode = mode;
                w.sidebar_view = view;
                cx.notify();
            });
            vcx.run_until_parked();
            let directory = vcx.debug_bounds("default-directory-button").unwrap();
            let machines = vcx.debug_bounds("machine-destination-switcher").unwrap();
            let gutter = vcx.debug_bounds("sidebar-scroll-gutter").unwrap();
            let body = vcx.debug_bounds("sidebar-tab-body").unwrap();
            assert!(gutter.top() >= directory.bottom());
            assert!(gutter.top() >= machines.bottom());
            assert!(vcx.debug_bounds("sidebar-workflow-switch").is_none());
            if view == SidebarView::Sessions {
                let catalog = vcx.debug_bounds("sidebar-session-catalog-controls").unwrap();
                assert_eq!(catalog.top(), machines.bottom());
                assert_eq!(gutter.top(), catalog.bottom());
            } else {
                assert!(vcx.debug_bounds("sidebar-session-catalog-controls").is_none());
                assert_eq!(gutter.top(), machines.bottom());
            }
            assert_eq!(gutter.top(), body.top());
            assert_eq!(gutter.bottom(), body.bottom());
            let outset = if mode == crate::config::LayoutMode::FolderTabs {
                SIDEBAR_SCROLLBAR_OUTSET
            } else {
                0.0
            };
            assert_eq!(gutter.right(), body.right() + px(outset));
            if view == SidebarView::Sessions && mode == crate::config::LayoutMode::FolderTabs {
                let scrollbar = vcx.debug_bounds("sidebar-scrollbar").unwrap();
                let tab = vcx.debug_bounds("sidebar-session-0").unwrap();
                let canvas = vcx.debug_bounds("workspace-canvas").unwrap();
                // The sheet is flush with the sidebar, so the thumb stays
                // inside the sidebar between the tabs and the canvas.
                assert!(scrollbar.left() >= tab.right());
                assert!(scrollbar.right() <= canvas.left());
            }
            let tabs_before =
                workspace.read_with(vcx, |w, _| w.sidebar_navigation_scroll.offset());
            let offset = |w: &Workspace| match view {
                SidebarView::Sessions => {
                    w.sidebar_sessions_list.scroll_px_offset_for_scrollbar().y
                }
                _ => w.sidebar_scroll.offset().y,
            };
            for (delta, expected) in [(-37.0, -37.0), (12.0, -25.0), (10000.0, 0.0)] {
                let position = vcx.debug_bounds("sidebar-scroll-gutter").unwrap().center();
                vcx.simulate_event(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(delta))),
                    modifiers: gpui::Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                });
                vcx.run_until_parked();
                workspace.read_with(vcx, |w, _| {
                    assert_eq!(
                        offset(w),
                        px(expected),
                        "gutter wheel must scroll exactly once"
                    );
                    assert_eq!(w.sidebar_navigation_scroll.offset(), tabs_before);
                });
            }
            let position = vcx.debug_bounds("sidebar-scroll-gutter").unwrap().center();
            for lines in [-4.0, 10000.0] {
                vcx.simulate_event(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Lines(gpui::point(0.0, lines)),
                    modifiers: gpui::Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                });
                vcx.run_until_parked();
                workspace.read_with(vcx, |w, _| {
                    if lines < 0.0 {
                        assert!(offset(w) < px(0.0));
                    } else {
                        assert_eq!(offset(w), px(0.0));
                    }
                });
            }
            assert_eq!(
                vcx.debug_bounds("default-directory-button").unwrap(),
                directory
            );
            assert_eq!(
                vcx.debug_bounds("machine-destination-switcher").unwrap(),
                machines
            );
        }
    }
}

#[gpui::test]
fn right_files_panel_browses_the_active_session_without_covering_canvas(
    cx: &mut gpui::TestAppContext,
) {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(project.path().join(".env.example"), "example\n").unwrap();
    std::fs::create_dir(project.path().join(".git")).unwrap();
    cx.update(crate::input::bind_keys);
    let (workspace, vcx) = focused_workspace(cx);
    vcx.simulate_resize(gpui::size(px(1440.0), px(900.0)));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("project", cx);
        workspace.slots[0].panel.update_chat(cx, |panel, _| {
            panel.working_dir = Some(project.path().display().to_string());
        });
        cx.notify();
    });
    vcx.run_until_parked();
    let canvas = vcx.debug_bounds("workspace-canvas").unwrap();
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-e"
    } else {
        "super-shift-e"
    });
    vcx.run_until_parked();
    let files = vcx
        .debug_bounds("workspace-files-panel")
        .expect("Files opens on the right");
    assert!(canvas.size.width > vcx.debug_bounds("workspace-canvas").unwrap().size.width);
    assert!(files.left() >= vcx.debug_bounds("workspace-canvas").unwrap().right());
    assert!(vcx.debug_bounds("right-file-tree-.env.example").is_some());
    assert!(vcx.debug_bounds("right-file-tree-.git").is_none());
    assert!(vcx.debug_bounds("right-file-tree-main.rs").is_none());
    let folder = vcx.debug_bounds("right-file-tree-src").unwrap();
    vcx.simulate_click(folder.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let file = vcx.debug_bounds("right-file-tree-main.rs").unwrap();
    vcx.simulate_click(file.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("code-file-contents").is_some());
    assert!(vcx.debug_bounds("right-file-tree-main.rs").is_some());
    let search = vcx.debug_bounds("workspace-files-search").unwrap();
    vcx.simulate_click(search.center(), gpui::Modifiers::default());
    vcx.simulate_input("src main");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("right-file-tree-src/main.rs").is_some());
    assert!(vcx.debug_bounds("right-file-tree-.env.example").is_none());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("right-file-tree-.env.example").is_some());
    let close = vcx.debug_bounds("workspace-files-close").unwrap();
    vcx.simulate_click(close.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("workspace-files-panel").is_none());
    assert_eq!(vcx.debug_bounds("workspace-canvas").unwrap(), canvas);
}

#[gpui::test]
fn sidebar_files_reserve_a_stable_scrollbar_gutter(cx: &mut gpui::TestAppContext) {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("main.rs"), "fn main() {}\n").unwrap();
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("project", cx);
        workspace.slots[0].panel.update_chat(cx, |panel, _| {
            panel.working_dir = Some(project.path().display().to_string());
        });
        workspace.sidebar_view = SidebarView::Files;
        cx.notify();
    });
    vcx.run_until_parked();
    let before = vcx.debug_bounds("sidebar-file-list").unwrap();
    assert!(vcx.debug_bounds("sidebar-scrollbar").is_none());
    for index in 0..80 {
        std::fs::write(project.path().join(format!("z-file-{index}.rs")), "").unwrap();
    }
    workspace.update(vcx, |workspace, cx| {
        let mut tree = workspace.file_tree.borrow_mut();
        tree.rows.get_mut(project.path()).unwrap().0 -= Duration::from_secs(2);
        cx.notify();
    });
    vcx.run_until_parked();
    // The overlay reads the scroll bounds from the preceding layout pass.
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    let list = vcx.debug_bounds("sidebar-file-list").unwrap();
    let row = vcx.debug_bounds("file-tree-main.rs").unwrap();
    let scrollbar = vcx.debug_bounds("sidebar-scrollbar").unwrap();
    assert_eq!(
        before.size.width, list.size.width,
        "overflow must not shift file content horizontally"
    );
    assert!(list.right() + px(4.0) <= scrollbar.left());
    assert!(row.right() + px(4.0) <= scrollbar.left());
}

#[gpui::test]
fn open_sidebar_sessions_follow_strips_top_to_bottom(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.active_row = 1;
        workspace.push_test_panel("session_fox_lower", cx);
        workspace.active_row = 0;
        workspace.push_test_panel("session_owl_upper_left", cx);
        workspace.push_test_panel("session_hare_upper_right", cx);
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    session_info("session_fox_lower", Some("lower")),
                    session_info("session_hare_upper_right", Some("upper right")),
                    session_info("session_owl_upper_left", Some("upper left")),
                ],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();

    workspace.read_with(vcx, |workspace, _| {
        assert_eq!(
            workspace
                .sidebar_session_layout
                .iter()
                .map(|item| item.session_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "session_owl_upper_left",
                "session_hare_upper_right",
                "session_fox_lower"
            ]
        );
    });
    for (selector, expected_session) in [
        ("sidebar-session-0", "session_owl_upper_left"),
        ("sidebar-session-1", "session_hare_upper_right"),
        ("sidebar-session-2", "session_fox_lower"),
    ] {
        let row = vcx
            .debug_bounds(selector)
            .expect("open session row should paint");
        vcx.simulate_click(row.center(), gpui::Modifiers::default());
        workspace.update(vcx, |workspace, cx| {
            assert_eq!(
                workspace.slots[workspace.active].panel.session_id(cx),
                expected_session
            );
            assert_eq!(workspace.active_row, workspace.slots[workspace.active].row);
        });
    }
}

#[gpui::test]
fn sidebar_todos_button_spawns_a_persistent_movable_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("chat", cx);
        cx.notify();
    });
    vcx.run_until_parked();

    click_sidebar_navigation(&workspace, vcx, "sidebar-unfinished-work");
    workspace.update(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2);
        assert_eq!(
            workspace.slots[workspace.active].panel.session_id(cx),
            "unfinished-work"
        );
        // Leaving the navigation popup repaints the workspace while the
        // panel surface is cached. Repaint that entity so this frame's
        // debug-selector map includes its descendants before inspecting it.
        workspace.slots[workspace.active]
            .panel
            .update(cx, |_, cx| cx.notify());
    });
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("unfinished-work-list").is_some(),
        "clicking todos should spawn the dedicated panel"
    );

    vcx.simulate_keystrokes("super-h");
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _cx| {
        assert_eq!(
            workspace.active, 0,
            "focus-left should leave the focused todos panel"
        );
    });

    vcx.simulate_keystrokes("super-l");
    vcx.run_until_parked();

    // The todos view is a normal workspace slot, not an overlay. It must
    // therefore follow the same ordering and strip movement commands as a
    // chat panel and be included in the workspace snapshot.
    vcx.simulate_keystrokes("super-shift-h super-shift-j");
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2);
        assert_eq!(workspace.slots[0].panel.session_id(cx), "unfinished-work");
        assert_eq!(workspace.slots[0].row, 1);
        let panel_snapshot = workspace.slots[0].panel.read(cx).snapshot(cx);
        assert_eq!(panel_snapshot.session_id, "unfinished-work");
    });
}

#[gpui::test]
fn clicking_an_unfinished_work_card_opens_its_chat_session(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("current-chat", cx);
        cx.notify();
    });
    vcx.run_until_parked();

    click_sidebar_navigation(&workspace, vcx, "sidebar-unfinished-work");

    workspace.update(vcx, |workspace, cx| {
        let panel = workspace.slots[workspace.active].panel.clone();
        panel.update(cx, |panel, cx| {
            panel.set_unfinished_work(
                vec![crate::harness::UnfinishedSession {
                    session_id: "todo-chat".into(),
                    title: "Todo chat".into(),
                    working_dir: None,
                    todos: vec![crate::harness::UnfinishedTodo {
                        content: "Finish this work".into(),
                        status: "pending".into(),
                        group: None,
                    }],
                }],
                cx,
            );
        });
    });
    vcx.run_until_parked();

    workspace.update(vcx, |workspace, _cx| {
        workspace
            .sessions
            .push(session_info("todo-chat", Some("Todo chat")));
        assert!(
            workspace
                .sessions
                .iter()
                .any(|session| session.session_id == "todo-chat")
        );
    });

    let card = vcx
        .debug_bounds("unfinished-session-0")
        .expect("unfinished session card paints");
    vcx.simulate_click(card.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    workspace.read_with(vcx, |workspace, cx| {
        assert!(
            workspace
                .slots
                .iter()
                .any(|slot| slot.panel.session_id(cx) == "todo-chat"),
            "clicking the card should open the chat panel"
        );
        assert_eq!(
            workspace.slots[workspace.active].panel.session_id(cx),
            "todo-chat"
        );
    });
}

#[gpui::test]
fn clicking_an_unfinished_work_card_reuses_its_open_chat_session(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("current-chat", cx);
        cx.notify();
    });
    vcx.run_until_parked();
    click_sidebar_navigation(&workspace, vcx, "sidebar-unfinished-work");

    let chat = workspace.read_with(vcx, |workspace, _| workspace.slots[0].panel.clone());
    let work = workspace.read_with(vcx, |workspace, _| workspace.slots[1].panel.clone());
    work.update(vcx, |panel, cx| {
        panel.set_unfinished_work(
            vec![crate::harness::UnfinishedSession {
                session_id: "current-chat".into(),
                title: "Current chat".into(),
                working_dir: None,
                todos: vec![crate::harness::UnfinishedTodo {
                    content: "Finish this work".into(),
                    status: "pending".into(),
                    group: None,
                }],
            }],
            cx,
        );
    });

    // Route from both the active work panel and an inactive work panel
    // while its destination chat is already active. Neither may duplicate
    // the chat or leave focus on the card's containing panel.
    for source_active in [true, false] {
        workspace.update_in(vcx, |workspace, window, cx| {
            workspace.set_active(usize::from(source_active), cx);
            workspace.focus_active(window, cx);
            work.update(cx, |_, cx| cx.notify());
            cx.notify();
        });
        vcx.run_until_parked();
        let card = vcx
            .debug_bounds("unfinished-session-0")
            .expect("unfinished session card paints");
        vcx.simulate_click(card.center(), gpui::Modifiers::default());
        vcx.run_until_parked();

        workspace.read_with(vcx, |workspace, _| {
            assert_eq!(workspace.slots.len(), 2, "reuse, do not duplicate the chat");
            assert_eq!(workspace.slots[workspace.active].panel, chat);
        });
        workspace.update_in(vcx, |_, window, cx| {
            assert!(
                chat.read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        });
    }
}

#[gpui::test]
fn clicking_a_file_opens_it_in_a_new_panel(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let project = std::env::temp_dir().join(format!(
        "jcode-desktop-files-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("main.rs"), "fn main() {}\n").unwrap();

    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(crate::learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("project", cx);
        workspace.slots[0].panel.update_chat(cx, |panel, _| {
            panel.working_dir = Some(project.display().to_string());
        });
        cx.notify();
    });
    vcx.run_until_parked();

    click_sidebar_navigation(&workspace, vcx, "sidebar-files-tab");
    let source = vcx
        .debug_bounds("file-tree-main.rs")
        .expect("source file paints in the file tree");
    vcx.simulate_click(source.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("code-file-contents").is_some(),
        "clicking a source file should open its read-only panel"
    );
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2);
        assert!(
            workspace.slots[1]
                .panel
                .session_id(cx)
                .ends_with("/main.rs")
        );
    });
    std::fs::remove_dir_all(project).unwrap();
}

/// Clicking a sidebar row must open and activate that session through the
/// real painted element, not merely paint a row that looks clickable.
#[gpui::test]
fn clicking_a_sidebar_row_activates_that_session(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.sidebar_closed_sessions.extend([
            "session_fox_1234567890_deadbeef".into(),
            "session_owl_1234567890_deadbeef".into(),
        ]);
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    session_info("session_fox_1234567890_deadbeef", Some("fox work")),
                    session_info("session_owl_1234567890_deadbeef", Some("owl work")),
                ],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("sidebar-session-1")
        .expect("the sidebar must paint a second session row");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    workspace.update(vcx, |workspace, cx| {
        let active = workspace
            .slots
            .get(workspace.active)
            .expect("clicking a row must open a panel")
            .panel
            .read(cx)
            .session_id
            .clone();
        assert_eq!(
            active, "session_owl_1234567890_deadbeef",
            "clicking the row must activate the session it displays"
        );
    });
}

#[gpui::test]
fn clicking_scrolled_history_rows_reliably_activates_the_displayed_session(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .extend((0..80).map(|index| format!("session_fox_history_{index}")));
        workspace.apply(
            Update::Sessions {
                sessions: (0..80)
                    .map(|index| {
                        session_info(
                            Box::leak(format!("session_fox_history_{index}").into_boxed_str()),
                            Some("previous work"),
                        )
                    })
                    .collect(),
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();
    for mode in [
        crate::config::LayoutMode::FolderTabs,
        crate::config::LayoutMode::Normal,
    ] {
        workspace.update(vcx, |workspace, cx| {
            workspace.layout_mode = mode;
            cx.notify();
        });
        vcx.run_until_parked();
        for target in [30, 50, 20, 60] {
            workspace.update(vcx, |workspace, cx| {
                // Place the target at the top. Bottom-edge reveal relies on
                // estimated heights for rows not measured yet, which changes
                // with the fixed workflow switch and sidebar viewport size.
                workspace.sidebar_sessions_list.scroll_to(gpui::ListOffset {
                    item_ix: target,
                    offset_in_item: px(0.0),
                });
                cx.notify();
            });
            vcx.run_until_parked();
            let expected = workspace.read_with(vcx, |workspace, _| {
                workspace.sidebar_session_layout[target].session_id.clone()
            });
            let bounds = vcx
                .debug_bounds(Box::leak(
                    format!("sidebar-session-{target}").into_boxed_str(),
                ))
                .expect("scrolled target must be rendered");
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            workspace.read_with(vcx, |workspace, cx| {
                assert_eq!(
                    workspace.slots[workspace.active].panel.session_id(cx),
                    expected,
                    "history click must activate the displayed row"
                );
            });
            workspace.update_in(vcx, |workspace, window, cx| {
                assert_eq!(
                    workspace.navigation_state(window, cx)["keyboard_panel"],
                    workspace.active,
                    "history click must leave the opened session ready for typing"
                );
            });
            vcx.simulate_input("history draft");
            workspace.read_with(vcx, |workspace, cx| {
                assert!(
                    workspace.slots[workspace.active]
                        .panel
                        .read(cx)
                        .input
                        .read(cx)
                        .content
                        .ends_with("history draft"),
                    "typing must reach the clicked session"
                );
            });
        }
    }
}

#[gpui::test]
fn sidebar_top_folder_tabs_join_their_content_when_clicked(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    vcx.run_until_parked();
    for (selector, view) in [
        ("sidebar-files-tab", SidebarView::Files),
        ("sidebar-accounts-tab", SidebarView::Accounts),
        ("sidebar-sessions-tab", SidebarView::Sessions),
    ] {
        click_sidebar_navigation(&workspace, vcx, selector);
        assert_eq!(workspace.read_with(vcx, |w, _| w.sidebar_view), view);
        let header = vcx.debug_bounds("sidebar-navigation-tabs").unwrap();
        let trigger = vcx.debug_bounds("sidebar-section-trigger").unwrap();
        let destinations = vcx.debug_bounds("machine-destination-switcher").unwrap();
        let sidebar = vcx.debug_bounds("sidebar").unwrap();
        assert_eq!(
            destinations.top(),
            sidebar.top(),
            "the destination and directory row is the only header line"
        );
        assert!(header.top() >= destinations.top());
        assert!(header.bottom() <= destinations.bottom());
        assert!(trigger.left() >= header.left() && trigger.right() <= header.right());
        assert!(vcx.debug_bounds("sidebar-roller-next").is_none());
    }
}

#[gpui::test]
fn sidebar_accounts_tab_replaces_sessions_and_has_an_empty_state(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("accounts-section").is_none());
    assert!(vcx.debug_bounds("accounts-empty").is_none());
    click_sidebar_navigation(&workspace, vcx, "sidebar-accounts-tab");
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.sidebar_view),
        SidebarView::Accounts
    );
    assert!(vcx.debug_bounds("accounts-empty").is_some());
    assert!(vcx.debug_bounds("sidebar-session-list").is_none());
    assert!(vcx.debug_bounds("sidebar-scrollbar").is_none());
    workspace.update(vcx, |w, cx| {
        w.set_test_accounts(vec![accounts::Account {
            id: "openai".into(),
            display_name: "OpenAI".into(),
            status: "available".into(),
            auth_kind: "OAuth".into(),
            method: "OAuth".into(),
            usage_reports: Vec::new(),
            limits: Vec::new(),
        }]);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("accounts-section").is_some());
    assert!(vcx.debug_bounds("accounts-empty").is_none());
    click_sidebar_navigation(&workspace, vcx, "sidebar-sessions-tab");
    assert!(vcx.debug_bounds("accounts-section").is_none());
    assert!(vcx.debug_bounds("sidebar-session-list").is_some());
}

#[gpui::test]
fn sidebar_folder_tab_reaches_the_canvas_edge(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        w.push_test_panel("session_fox_active", cx);
        w.apply(
            Update::Sessions {
                sessions: vec![session_info("session_fox_active", Some("active work"))],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("native-folder-surface").is_some());
    let frame = workspace.read_with(vcx, |w, _| w.folder_frame.clone());
    let frame = frame.borrow();
    assert!(frame.selected_tab.is_some());
    assert!(frame.active_panel.is_some());
    assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 0);
}

#[gpui::test]
fn accounts_sidebar_groups_all_supported_states_and_refreshes(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        w.sidebar_view = SidebarView::Accounts;
        w.accounts = accounts::parse(r#"{"providers":[
            {"id":"openai","display_name":"OpenAI","status":"not_configured","auth_kind":"OAuth"},
            {"id":"claude","display_name":"Claude","status":"expired","auth_kind":"OAuth"},
            {"id":"openrouter","display_name":"OpenRouter","status":"available","auth_kind":"API key"}
        ]}"#).unwrap();
        w.recent_accounts = vec!["openai".into()];
        cx.notify();
    });
    vcx.run_until_parked();
    let connected = vcx.debug_bounds("accounts-connected-heading").unwrap();
    let disconnected = vcx.debug_bounds("accounts-disconnected-heading").unwrap();
    let ready = vcx.debug_bounds("account-openrouter").unwrap();
    let signed_out = vcx.debug_bounds("account-openai").unwrap();
    assert!(connected.bottom() <= ready.top());
    assert!(ready.bottom() <= disconnected.top());
    assert!(disconnected.bottom() <= signed_out.top());
    assert!(vcx.debug_bounds("account-openai-history").is_none());
    for id in ["openai", "claude", "openrouter"] {
        let row = vcx
            .debug_bounds(Box::leak(format!("account-{id}").into_boxed_str()))
            .unwrap();
        let status = vcx
            .debug_bounds(Box::leak(format!("account-{id}-status").into_boxed_str()))
            .unwrap();
        assert!(status.top() >= row.top() && status.bottom() <= row.bottom());
    }
    workspace.update(vcx, |w, cx| {
        for account in &mut w.accounts {
            account.status = "not_configured".into();
        }
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("accounts-connected-heading").is_none());
    assert!(vcx.debug_bounds("accounts-disconnected-heading").is_some());
    assert!(vcx.debug_bounds("account-openrouter-status").is_some());
}

#[gpui::test]
fn jcode_account_row_shows_daily_limits_and_upgrade_pill(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    // Real `jcode usage --json` shape captured from the live gateway.
    let usage = r#"{"providers":[{"provider_name":"Jcode subscription","limits":[
        {"name":"Memory recall (daily)","usage_percent":100.0,"reset_in":"19h 34m"},
        {"name":"Browser automation (daily)","usage_percent":0.1,"reset_in":"19h 34m"}
      ],"extra_info":[["Plan","Plus"],["Upgrade","Pro raises daily limits: https://jcode.sh/pricing"]]}]}"#;
    workspace.update(vcx, |w, cx| {
        w.sidebar_view = SidebarView::Accounts;
        w.accounts = accounts::parse(
            r#"{"providers":[
            {"id":"jcode","display_name":"Jcode","status":"available","auth_kind":"API key"}
        ]}"#,
        )
        .unwrap();
        accounts::merge_usage_for_tests(&mut w.accounts, usage);
        cx.notify();
    });
    vcx.run_until_parked();
    let row = vcx.debug_bounds("account-jcode").unwrap();
    let memory = vcx.debug_bounds("account-jcode-limit-0").unwrap();
    let pill = vcx.debug_bounds("account-jcode-upgrade").unwrap();
    assert!(memory.top() >= row.top() && memory.bottom() <= row.bottom());
    assert!(
        pill.top() >= row.top() && pill.bottom() <= row.bottom(),
        "pill clipped: {pill:?} in {row:?}"
    );
    assert_eq!(
        workspace.read_with(vcx, |w, _| w.accounts[0].upgrade_offer()),
        Some((
            "Pro raises daily limits".into(),
            "https://jcode.sh/pricing".into()
        ))
    );
    // Below the hint threshold `jcode usage` omits Upgrade and no pill renders.
    workspace.update(vcx, |w, cx| {
        w.accounts[0].usage_reports[0]
            .extra_info
            .retain(|(key, _)| key != "Upgrade");
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("account-jcode-upgrade").is_none());
}

#[gpui::test]
fn accounts_oauth_history_renders_named_periods_and_unavailable_state(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        w.sidebar_view = SidebarView::Accounts;
        w.accounts = accounts::parse(r#"{"providers":[{"id":"openai","display_name":"OpenAI","status":"available","auth_kind":"OAuth"}]}"#).unwrap();
        // Two OpenAI logins: the daemon resolves an icon for each.
        let mut view = crate::routing::feed::fixture();
        let template = view.accounts[0].clone();
        view.accounts = ["personal", "work"]
            .into_iter()
            .map(|label| jcode_session_types::routing::RoutingAccountView {
                id: format!("openai:{label}"),
                icon: Some(jcode_session_types::account_icon::account_icon(label, None)),
                ..template.clone()
            })
            .collect();
        w.set_test_routing_view(view);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("account-openai-history-unavailable")
            .is_some()
    );
    workspace.update(vcx, |w, cx| {
        w.accounts[0].usage_reports = ["personal", "work"].into_iter().map(|label| {
            accounts::UsageReport { identity: None, credits: None,
                limits: Vec::new(),
                banked_reset: None,
                provider_name: format!("OpenAI (ChatGPT) {label}"),
                account_label: Some(label.into()),
                extra_info: vec![
                    ("Today".into(), "12345 input / 456 output tokens (10000 cached input), $0.0200 API-equivalent estimate, not a bill; recorded only; since local midnight".into()),
                    ("Lifetime".into(), "No recorded usage".into()),
                ],
            }
        }).collect();
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("account-openai-history-unavailable")
            .is_none()
    );
    let row = vcx.debug_bounds("account-openai").unwrap();
    let first = vcx.debug_bounds("account-openai-history-report-0").unwrap();
    let second = vcx.debug_bounds("account-openai-history-report-1").unwrap();
    // Each login is one short line. Verbose token counts and caveats moved
    // to the tooltip, so the sidebar row stays compact.
    assert!(vcx.debug_bounds("account-openai-estimate-note").is_none());
    assert!(first.size.height <= px(14.0), "{first:?}");
    assert!(first.bottom() <= second.top());
    for line in [first, second] {
        let index = usize::from(line == second);
        let icon = vcx
            .debug_bounds(Box::leak(
                format!("account-openai-history-report-{index}-icon").into_boxed_str(),
            ))
            .expect("a named login's line leads with its icon");
        assert!(icon.left() <= line.left() + px(1.0) && icon.right() <= line.right());
    }
    assert!(
        second.bottom() <= row.bottom(),
        "history must not be clipped by compact row height"
    );
    assert!(
        first.right() <= row.right(),
        "summary must fit inside the sidebar"
    );
    let report = workspace.read_with(vcx, |w, _| w.accounts[0].usage_reports[0].clone());
    assert_eq!(report.period_amount("Today").as_deref(), Some("$0.02"));
    assert_eq!(report.period_amount("Lifetime").as_deref(), Some("$0.00"));
}

#[gpui::test]
fn accounts_fill_tab_with_mixed_quota_details(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    for count in [1, 2, 3, 4, 7, 12, 40] {
        workspace.update(vcx, |w, cx| {
            w.sidebar_view = SidebarView::Accounts;
            w.accounts_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
            w.accounts_scroll_remainder = 0.0;
            w.set_test_accounts(
                (0..count)
                    .map(|index| accounts::Account {
                        id: format!("mixed-{index}"),
                        display_name: format!("Account {index}"),
                        status: "available".into(),
                        auth_kind: "OAuth".into(),
                        method: "OAuth".into(),
                        usage_reports: Vec::new(),
                        limits: if index % 2 == 0 {
                            vec![accounts::UsageLimit {
                                name: "credits".into(),
                                usage_percent: 25.0,
                                reset_in: Some("2h".into()),
                            }]
                        } else {
                            Vec::new()
                        },
                    })
                    .collect(),
            );
            cx.notify();
        });
        vcx.run_until_parked();
        for delta in [0.0, -17.0, -31.0, -10000.0, 10000.0] {
            let list = vcx.debug_bounds("accounts-list").unwrap();
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: list.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(delta))),
                modifiers: gpui::Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
            let list = vcx.debug_bounds("accounts-list").unwrap();
            let body = vcx.debug_bounds("sidebar-tab-body").unwrap();
            let section = vcx.debug_bounds("accounts-section").unwrap();
            let machine_switcher = vcx.debug_bounds("machine-destination-switcher").unwrap();
            assert_eq!(section.origin.y, machine_switcher.bottom());
            assert_eq!(section.bottom(), body.bottom());
            assert!(list.size.height > px(ACCOUNT_ROW_HEIGHT * 3.0));
            assert!(list.bottom() <= body.bottom());
            assert!(body.bottom() - list.bottom() <= px(4.0));
            let (offset, max) = workspace.read_with(vcx, |w, _| {
                (
                    w.accounts_scroll.offset().y,
                    w.accounts_scroll.max_offset().y,
                )
            });
            assert!(offset <= px(0.0) && offset >= -max);
            if delta == -10000.0 {
                assert_eq!(offset, -max);
            } else if delta == 10000.0 {
                assert_eq!(offset, px(0.0));
            }
            let mut visible = 0;
            for index in 0..count {
                let row = vcx
                    .debug_bounds(Box::leak(format!("account-mixed-{index}").into_boxed_str()))
                    .unwrap();
                assert_eq!(row.size.height, px(ACCOUNT_ROW_HEIGHT));
                if row.bottom() > list.top() && row.top() < list.bottom() {
                    visible += 1;
                }
            }
            let capacity = (f32::from(list.size.height) / ACCOUNT_ROW_HEIGHT).floor() as usize;
            assert!(visible >= count.min(capacity));
            if count <= capacity {
                assert_eq!(offset, px(0.0));
                assert_eq!(visible, count);
            }
        }
    }
}

#[gpui::test]
fn accounts_scroll_independently_and_prioritize_active(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.push_test_panel("active", cx);
        workspace.slots[0].panel.update_chat(cx, |panel, _| {
            panel.provider = Some("provider-9".into());
        });
        workspace.sidebar_view = SidebarView::Accounts;
        workspace.accounts = (0..40)
            .map(|index| accounts::Account {
                id: format!("provider-{index}"),
                display_name: format!("Provider {index}"),
                status: "available".into(),
                auth_kind: "OAuth".into(),
                method: "OAuth".into(),
                usage_reports: Vec::new(),
                limits: Vec::new(),
            })
            .collect();
        workspace.recent_accounts = vec!["provider-7".into(), "provider-4".into()];
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    let list = vcx.debug_bounds("accounts-list").unwrap();
    assert!(list.size.height > px(ACCOUNT_ROW_HEIGHT * 3.0));
    let section = vcx.debug_bounds("accounts-section").unwrap();
    let body = vcx.debug_bounds("sidebar-tab-body").unwrap();
    let machine_switcher = vcx.debug_bounds("machine-destination-switcher").unwrap();
    assert_eq!(section.origin.y, machine_switcher.bottom());
    assert_eq!(section.bottom(), body.bottom());
    assert!(
        vcx.debug_bounds("account-provider-9").unwrap().top()
            < vcx.debug_bounds("account-provider-7").unwrap().top()
    );
    assert!(
        vcx.debug_bounds("account-provider-7").unwrap().top()
            < vcx.debug_bounds("account-provider-4").unwrap().top()
    );
    let scrollbar = vcx
        .debug_bounds("accounts-scrollbar")
        .expect("overflow needs a scrollbar");
    assert!(list.right() + px(4.0) <= scrollbar.left());
    let sidebar_before = workspace.read_with(vcx, |w, _| w.sidebar_scroll.offset());
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -4.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| {
        assert!(w.accounts_scroll.offset().y < px(0.0));
        assert_eq!(w.sidebar_scroll.offset(), sidebar_before);
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::Event {
                session_id: "active".into(),
                event: jcode_sdk::ApiEvent::TurnDone {
                    session_id: "active".into(),
                    pending_soft_interrupts: None,
                },
            },
            cx,
        );
        assert_eq!(
            workspace.recent_accounts,
            ["provider-9", "provider-7", "provider-4"]
        );
    });
}

/// The accounts strip must actually paint both a regular provider logo and
/// Jcode's donut. Seeding bypasses the CLI so the test needs no runtime or
/// credentials.
#[gpui::test]
fn connected_accounts_paint_in_the_sidebar(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (_workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.sidebar_view = SidebarView::Accounts;
        workspace.set_test_accounts(vec![
            accounts::Account {
                id: "openai".into(),
                display_name: "OpenAI".into(),
                status: "available".into(),
                auth_kind: "OAuth".into(),
                method: "OAuth".into(),
                usage_reports: Vec::new(),
                limits: vec![
                    accounts::UsageLimit {
                        name: "5 hour".into(),
                        usage_percent: 25.0,
                        reset_in: Some("2h".into()),
                    },
                    accounts::UsageLimit {
                        name: "Weekly".into(),
                        usage_percent: 80.0,
                        reset_in: Some("4d".into()),
                    },
                    accounts::UsageLimit {
                        name: "Gemini 3 Pro".into(),
                        usage_percent: 35.0,
                        reset_in: Some("1h".into()),
                    },
                ],
            },
            accounts::Account {
                id: "jcode".into(),
                display_name: "Jcode".into(),
                status: "expired".into(),
                auth_kind: "API key".into(),
                method: "API key (`JCODE_API_KEY`)".into(),
                usage_reports: Vec::new(),
                limits: Vec::new(),
            },
            accounts::Account {
                id: "antigravity".into(),
                display_name: "Antigravity".into(),
                status: "available".into(),
                auth_kind: "OAuth".into(),
                method: "OAuth".into(),
                usage_reports: Vec::new(),
                limits: vec![
                    accounts::UsageLimit {
                        name: "Claude".into(),
                        usage_percent: 10.0,
                        reset_in: Some("5h".into()),
                    },
                    accounts::UsageLimit {
                        name: "Gemini".into(),
                        usage_percent: 10.0,
                        reset_in: Some("5h".into()),
                    },
                ],
            },
        ]);
        let _ = window;
        workspace
    });
    vcx.run_until_parked();

    assert!(
        accounts::logo("openai").is_some(),
        "openai must have a vendored logo"
    );
    assert!(
        accounts::logo("jcode").is_some(),
        "Jcode Subscription must use the donut logo, not a lettermark"
    );
    let openai_row = vcx
        .debug_bounds("account-openai")
        .expect("the OpenAI account row should have painted");
    let jcode_row = vcx
        .debug_bounds("account-jcode")
        .expect("the Jcode account row and donut should have painted");
    let first_limit = vcx
        .debug_bounds("account-openai-limit-0")
        .expect("the first usage limit should paint");
    let second_limit = vcx
        .debug_bounds("account-openai-limit-1")
        .expect("the second usage limit should paint");
    assert!(
        vcx.debug_bounds("account-openai-limit-2").is_none(),
        "additional quotas should be hidden to keep the account compact"
    );
    assert!(
        vcx.debug_bounds("account-jcode-limit-unavailable")
            .is_none(),
        "missing quota data should not add a second line"
    );
    assert!(
        vcx.debug_bounds("account-antigravity-limit-0").is_some(),
        "Antigravity should show one representative quota"
    );
    assert!(
        vcx.debug_bounds("account-antigravity-limit-1").is_none(),
        "Antigravity's duplicate per-model quotas should be hidden"
    );
    assert_eq!(
        first_limit.origin.y, second_limit.origin.y,
        "usage limits should share one compact horizontal row"
    );
    assert!(
        first_limit.origin.x < second_limit.origin.x,
        "usage limits should occupy separate horizontal columns"
    );
    assert_eq!(
        first_limit.size.width, second_limit.size.width,
        "each quota column should retain a readable width"
    );
    assert!(
        openai_row.origin.y < jcode_row.origin.y,
        "available accounts should be listed above expired ones"
    );
}
