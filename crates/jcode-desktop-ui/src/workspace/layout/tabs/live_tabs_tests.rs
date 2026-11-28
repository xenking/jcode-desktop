use super::*;

gpui::actions!(jcode_desktop_host, [CloseWindow]);

#[gpui::test]
fn next_workspace_pill_reaches_the_next_occupied_workspace_by_mouse(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("first", cx);
        w.active_row = 2;
        w.push_test_panel("third", cx);
        w.active_row = 0;
        w.active = 0;
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [1440., 800., 480.] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
        vcx.run_until_parked();
        let next = vcx.debug_bounds("tab-next-workspace").unwrap();
        let plus = vcx.debug_bounds("tab-new-session").unwrap();
        let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
        assert!(tabs.right() <= next.left(), "width={width}");
        assert!(next.right() <= plus.left(), "width={width}");
    }
    // Skips empty workspace 2 and lands on workspace 3, then wraps back.
    for expected in [2, 0] {
        let next = vcx.debug_bounds("tab-next-workspace").unwrap();
        vcx.simulate_click(next.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), expected);
    }
}

#[gpui::test]
#[cfg(not(target_os = "windows"))]
fn tab_close_window_is_separate_from_new_session_and_dispatches_host_action(
    cx: &mut gpui::TestAppContext,
) {
    let requests = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update({
        let requests = requests.clone();
        move |cx| {
            cx.on_action(move |_: &CloseWindow, _| requests.set(requests.get() + 1));
        }
    });
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("Keep this session", cx);
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [1440., 800., 480.] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
        vcx.run_until_parked();
        let close = vcx.debug_bounds("tab-close-window").unwrap();
        let plus = vcx.debug_bounds("tab-new-session").unwrap();
        let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
        let files = vcx.debug_bounds("workspace-files-toggle").unwrap();
        let next = vcx.debug_bounds("tab-next-workspace").unwrap();
        let actions = [files, next, plus, close];
        assert!(tabs.right() <= files.left());
        for action in actions {
            assert_eq!(action.size, gpui::size(px(28.), px(28.)));
            assert_eq!(action.top(), plus.top());
        }
        let gaps = [
            next.left() - files.right(),
            plus.left() - next.right(),
            close.left() - plus.right(),
        ];
        assert_eq!(gaps, [px(3.); 3]);
        assert!(close.right() <= px(width));
        vcx.simulate_click(close.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.slots.len()), 1);
    }
    assert_eq!(requests.get(), 3);
}

#[test]
fn coaching_chip_never_claims_current_or_incoming_tab_space() {
    let left = TabGeometry {
        left: 100.0,
        width: 200.0,
        height: 32.0,
    };
    let right = TabGeometry {
        left: 600.0,
        ..left
    };
    assert_eq!(coach_chip_left(1000.0, &[left], &[left]), Some(692.0));
    assert_eq!(coach_chip_left(1000.0, &[left], &[right]), None);
    assert_eq!(coach_chip_left(1000.0, &[right], &[left]), None);
    for available in [0.0, 240.0, 400.0, 600.0] {
        assert_eq!(coach_chip_left(available, &[left], &[left]), None);
    }
}

#[test]
fn a_machine_named_strip_reserves_its_name_before_that_machines_first_workspace_only() {
    // Workspaces 1 and 2 here, then workspaces 1 and 2 of the next machine.
    let rows = [0, 0, 1, ROWS_PER_SPACE, ROWS_PER_SPACE + 1];
    let named = TabLayout::grouped_named(1400.0, &rows, 0, true);
    let plain = TabLayout::grouped_named(1400.0, &rows, 0, false);
    assert_eq!(plain, TabLayout::grouped(1400.0, &rows, 0));
    let (label, machine) = (TAB_GROUP_LABEL_WIDTH, TAB_MACHINE_LABEL_WIDTH);
    assert_eq!(plain[0].left, label);
    assert_eq!(named[0].left, label + machine, "this machine's name leads its first group");
    let between = |tabs: &[TabGeometry], i: usize| tabs[i].left - tabs[i - 1].left - tabs[i - 1].width;
    // Workspace 2 of the same machine carries only its digit; the next machine
    // brings its own name.
    assert!((between(&named, 2) - (TAB_GROUP_GAP + label)).abs() < 0.001);
    assert!((between(&named, 3) - (TAB_GROUP_GAP + label + machine)).abs() < 0.001);
    assert!((between(&named, 4) - (TAB_GROUP_GAP + label)).abs() < 0.001);
}

#[test]
fn workspace_tab_groups_prioritize_current_row_and_keep_number_order() {
    let rows = [0, 0, 1, 1, 1, 3, 3];
    for selected in 0..rows.len() {
        let tabs = TabLayout::grouped(1400.0, &rows, selected);
        for (i, tab) in tabs.iter().enumerate() {
            assert_eq!(
                tab.width,
                if i == selected {
                    208.0
                } else if rows[i] == rows[selected] {
                    196.0
                } else {
                    56.0
                }
            );
            assert_eq!(
                tab.height,
                TAB_HEIGHT - if i == selected { 0.0 } else { 4.0 }
            );
            if i > 0 && rows[i - 1] != rows[i] {
                assert!(
                    (tab.left
                        - tabs[i - 1].left
                        - tabs[i - 1].width
                        - TAB_GROUP_GAP
                        - TAB_GROUP_LABEL_WIDTH)
                        .abs()
                        < 0.001
                );
            }
        }
        assert_eq!(tabs[0].left, TAB_GROUP_LABEL_WIDTH);
        let last = tabs.last().unwrap();
        assert!(last.left + last.width <= 1400.001);
    }
    // A single group preserves the tab motion within its label reservation.
    let old = TabLayout::new(512.0 - TAB_GROUP_LABEL_WIDTH, 12);
    for selected in 0..12 {
        assert_eq!(
            TabLayout::grouped(512.0, &[2; 12], selected),
            (0..12)
                .map(|i| {
                    let mut tab = old.geometry(i, selected);
                    tab.left += TAB_GROUP_LABEL_WIDTH - old.start;
                    tab
                })
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn crowded_workspace_tabs_keep_the_original_overlap() {
    let tabs = TabLayout::grouped(892.0, &[0; 12], 0);
    assert_eq!(tabs[1].left + tabs[1].width - tabs[2].left, 136.0);
}

#[test]
fn workspace_tab_groups_keep_crowded_sessions_exposed_and_resize_in_bounds() {
    assert!(TabLayout::grouped(0.0, &[], 0).is_empty());
    for counts in [
        [1, 1, 1, 1],
        [1, 200, 1, 1],
        [80, 2, 100, 40],
        [20, 30, 40, 50],
    ] {
        let rows: Vec<_> = counts
            .iter()
            .enumerate()
            .flat_map(|(row, count)| std::iter::repeat_n(row, *count))
            .collect();
        for available in [0.0, 40.0, 100.0, 352.0, 512.0, 800.0, 1600.0] {
            for selected in [0, counts[0], counts[0] + counts[1], rows.len() - 1] {
                let tabs = TabLayout::grouped(available, &rows, selected);
                for (i, tab) in tabs.iter().enumerate() {
                    assert!(tab.left.is_finite() && tab.width.is_finite());
                    assert!(
                        tab.left >= 0.0 && tab.left + tab.width <= available + 0.001,
                        "available={available}, selected={selected}, i={i}, tab={tab:?}"
                    );
                    if available > 0.0 {
                        assert!(
                            TabLayout::exposed(&tabs, i, selected).1 > 0.0,
                            "every session must retain a click target: available={available}, selected={selected}, i={i}"
                        );
                        if i > 0 && rows[i - 1] != rows[i] {
                            assert!(tab.left > tabs[i - 1].left + tabs[i - 1].width);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn workspace_tab_groups_animate_focus_and_snap_safely_on_resize() {
    let now = Instant::now();
    let duration = Duration::from_millis(150);
    let rows = [0, 0, 1, 1, 2, 2];
    let targets = |available, selected| {
        TabLayout::grouped(available, &rows, selected)
            .into_iter()
            .enumerate()
            .map(|(i, tab)| (i as u64, tab))
            .collect::<Vec<_>>()
    };
    let mut motion = TabMotion::default();
    let first = motion.sample(&targets(1200.0, 0), 1200.0, duration, now);
    assert_eq!(
        first,
        motion.sample(&targets(1200.0, 3), 1200.0, duration, now)
    );
    let middle = motion.sample(
        &targets(1200.0, 3),
        1200.0,
        duration,
        now + Duration::from_millis(50),
    );
    assert!(middle[3].width > first[3].width && middle[3].width < 208.0);
    for i in [2, 4] {
        assert!(
            (middle[i].left
                - middle[i - 1].left
                - middle[i - 1].width
                - TAB_GROUP_GAP
                - TAB_GROUP_LABEL_WIDTH)
                .abs()
                < 0.001
        );
    }
    assert_eq!(
        middle,
        motion.sample(
            &targets(1200.0, 5),
            1200.0,
            duration,
            now + Duration::from_millis(50)
        ),
        "retargeting must not jump"
    );
    let resized = motion.sample(
        &targets(120.0, 5),
        120.0,
        duration,
        now + Duration::from_millis(60),
    );
    assert!(!motion.is_animating());
    assert_eq!(resized, TabLayout::grouped(120.0, &rows, 5));
    assert!(
        resized
            .iter()
            .all(|tab| tab.left >= 0.0 && tab.left + tab.width <= 120.001)
    );
    let reduced = motion.sample(&targets(120.0, 0), 120.0, Duration::ZERO, now);
    assert_eq!(reduced, TabLayout::grouped(120.0, &rows, 0));
    assert!(!motion.is_animating());
}

#[gpui::test]
fn workspace_tab_groups_preserve_click_keyboard_and_empty_row_navigation(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for i in 0..9 {
            w.push_test_panel(&format!("group-session-{i}"), cx);
            w.slots[i].row = i / 3;
        }
        w
    });
    let handle = vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
        window.window_handle()
    });
    for width in [1400.0, 640.0, 1000.0] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.0)));
        vcx.run_until_parked();
        for selected in [4, 8, 0, 7, 3, 2, 6, 5, 1] {
            workspace.update(vcx, |w, cx| {
                // Measure resting layout, not a tab popped out under the pointer.
                w.live_tabs.hovered = None;
                w.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
            let track = vcx.debug_bounds("live-session-tabs").unwrap();
            for row in 0..3 {
                let label = vcx
                    .debug_bounds(format!("workspace-tab-group-label-{row}").leak())
                    .unwrap();
                let first = vcx
                    .debug_bounds(format!("live-session-tab-{}", row * 3).leak())
                    .unwrap();
                assert!(label.left() >= track.left() - px(0.01));
                assert!(label.right() <= first.left() + px(0.01));
                if row > 0 {
                    let previous = vcx
                        .debug_bounds(format!("live-session-tab-{}", row * 3 - 1).leak())
                        .unwrap();
                    assert!(label.left() > previous.right());
                }
                for position in row * 3..row * 3 + 3 {
                    assert!(
                        vcx.debug_bounds(
                            format!("workspace-tab-badge-{position}-row-{row}").leak()
                        )
                        .is_none(),
                        "workspace numbers belong beside the group, never inside a session tab"
                    );
                }
            }
            let targets = workspace.read_with(vcx, |w, _| w.live_tabs.hit_targets().to_vec());
            assert_eq!(targets.len(), 9);
            for &(index, _) in &targets {
                let tab = vcx
                    .debug_bounds(format!("live-session-tab-{index}").leak())
                    .unwrap();
                assert!(tab.left() >= track.left() - px(0.01));
                assert!(tab.right() <= track.right() + px(0.01));
            }
            let x = targets
                .iter()
                .find(|(index, _)| *index == selected)
                .unwrap()
                .1;
            vcx.simulate_click(
                gpui::point(track.left() + px(x), track.bottom() - px(12.0)),
                gpui::Modifiers::default(),
            );
            vcx.run_until_parked();
            // Leave the tab row so the next pass measures resting geometry.
            vcx.simulate_mouse_move(
                gpui::point(px(5.0), px(600.0)),
                None,
                gpui::Modifiers::default(),
            );
            vcx.run_until_parked();
            workspace.update_in(vcx, |w, window, cx| {
                assert_eq!(w.active, selected, "width={width}");
                assert_eq!(w.active_row, selected / 3);
                assert_eq!(w.navigation_state(window, cx)["keyboard_panel"], selected);
                assert!(!w.overview);
            });
        }
    }
    for row in [2, 1, 0] {
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        let label = vcx
            .debug_bounds(format!("workspace-tab-group-label-{row}").leak())
            .unwrap();
        vcx.simulate_click(label.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), row);
    }
    // Workspace keys still select remembered sessions and the empty row
    // receives its own full-size placeholder rather than false focus.
    vcx.simulate_keystrokes("super-j super-j super-j");
    vcx.run_until_parked();
    assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 3);
    assert!(vcx.debug_bounds("live-session-empty-tab-title").is_some());
    vcx.simulate_keystrokes("super-k");
    vcx.run_until_parked();
    assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 2);
    assert!(vcx.debug_bounds("live-session-empty-tab").is_none());
    vcx.simulate_keystrokes("super-p");
    vcx.run_until_parked();
    assert_eq!(workspace.read_with(vcx, |w, _| w.active), 8);
}

#[gpui::test]
fn workspace_identity_follows_rows_and_number_badges_navigate(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.show_sidebar = false;
        workspace.enable_test_minimap();
        for row in 0..ROWS_PER_SPACE {
            workspace.push_test_panel(&format!("identity-{row}"), cx);
            workspace.slots[row].row = row;
        }
        workspace
    });
    vcx.run_until_parked();
    for row in 0..ROWS_PER_SPACE {
        assert!(
            vcx.debug_bounds(format!("workspace-tab-group-label-{row}").leak())
                .is_some()
        );
        assert!(
            vcx.debug_bounds(format!("workspace-tab-accent-{row}-row-{row}").leak())
                .is_none(),
            "workspace identity must not add a heavy stripe over the tab outline"
        );
    }
    for row in [1, 3, 2, 0] {
        let badge = vcx
            .debug_bounds(format!("workspace-map-badge-{row}").leak())
            .unwrap();
        let track = vcx
            .debug_bounds(format!("minimap-row-{row}").leak())
            .unwrap();
        assert!(
            badge.right() < track.left(),
            "numbers must not obscure the map"
        );
        vcx.simulate_click(badge.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(
            workspace.read_with(vcx, |workspace, _| workspace.active_row),
            row
        );
        assert!(
            vcx.debug_bounds(format!("workspace-map-badge-{row}-active").leak())
                .is_some()
        );
    }
    workspace.update(vcx, |workspace, cx| {
        workspace.slots[0].row = 1;
        workspace.set_active(0, cx);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("workspace-tab-group-label-1").is_some());
    assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_none());
    let empty = vcx.debug_bounds("workspace-map-badge-0").unwrap();
    vcx.simulate_click(empty.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("live-session-empty-tab-title").is_some());
    assert!(vcx.debug_bounds("workspace-map-badge-0-active").is_some());
    assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
    workspace.update(vcx, |workspace, cx| {
        workspace.show_minimap = false;
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
    assert!(vcx.debug_bounds("minimap").is_none());
}

#[gpui::test]
fn live_tabs_ring_the_outline_while_working_and_keep_the_emoji_still(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.show_sidebar = false;
        workspace.push_test_panel("activity-tab", cx);
        workspace.push_test_panel("quiet-tab", cx);
        workspace
    });
    vcx.run_until_parked();
    let idle_title_left = vcx.debug_bounds("live-session-tab-0-title").unwrap().left();
    for (status, active) in [
        ("running", true),
        ("thinking", true),
        ("generating", true),
        ("streaming", true),
        ("running_tools", true),
        ("busy", true),
        ("idle", false),
        ("attached", false),
        ("connected", false),
        ("lost: disconnected", false),
        ("error", false),
        ("crashed", false),
    ] {
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                Update::Event {
                    session_id: "activity-tab".into(),
                    event: jcode_sdk::ApiEvent::SessionStatus {
                        session_id: "activity-tab".into(),
                        status: status.into(),
                        pending_soft_interrupts: None,
                    },
                },
                cx,
            );
            cx.notify();
        });
        vcx.run_until_parked();
        // Live turns paint the outline ring. Quiet states never animate,
        // and the emoji itself never animates in any state.
        let quiet = matches!(status, "idle" | "attached" | "connected");
        if active || quiet {
            assert_eq!(
                vcx.debug_bounds("live-session-tab-0-ring").is_some(),
                active,
                "status {status}",
            );
        }
        assert!(vcx.debug_bounds("live-session-tab-1-spinner").is_none());
        assert!(vcx.debug_bounds("panel-session-title").is_none());
        // Active sessions also show an inline transcript status.
        assert_eq!(
            vcx.debug_bounds("panel-activity-label").is_some(),
            active,
            "inline status {status}",
        );
        let title = vcx.debug_bounds("live-session-tab-0-title").unwrap();
        assert!(vcx.debug_bounds("live-session-tab-0-spinner").is_none());
        assert!(vcx.debug_bounds("live-session-tab-1-ring").is_none());
        let emoji = vcx.debug_bounds("live-session-tab-0-emoji").unwrap();
        assert!(emoji.right() <= title.left());
        assert_eq!(
            title.left(),
            idle_title_left,
            "activity must not shift the title"
        );
    }
}

#[test]
fn live_tabs_leave_a_small_gap_between_uncrowded_sessions() {
    for rows in [vec![0, 0, 0, 0], vec![0, 0, 1, 1]] {
        for selected in 0..rows.len() {
            let tabs = TabLayout::grouped(1600.0, &rows, selected);
            for (i, pair) in tabs.windows(2).enumerate() {
                if rows[i] == rows[i + 1] {
                    assert!(
                        (pair[1].left - pair[0].left - pair[0].width - TAB_GAP).abs() < 0.001
                    );
                }
            }
        }
    }
}

/// A popped tab owns its whole popped body: hover stays on it while the
/// pointer travels to its close button at the body's right edge, and a
/// click there closes that tab. Its resting slot never moves.
#[gpui::test]
fn popped_tab_owns_its_body_and_its_close_sits_at_the_body_edge(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for i in 0..8 {
            w.push_test_panel(&format!("hover-session-{i}"), cx);
        }
        w
    });
    let handle = vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
        window.window_handle()
    });
    vcx.simulate_window_resize(handle, gpui::size(px(700.0), px(700.0)));
    workspace.update(vcx, |w, cx| {
        w.set_active(3, cx);
        cx.notify();
    });
    vcx.run_until_parked();
    let rest: Vec<_> = (0..8)
        .map(|i| {
            vcx.debug_bounds(format!("live-session-tab-{i}-slot").leak())
                .unwrap()
                .center()
        })
        .collect();
    let away = gpui::point(px(350.0), px(400.0));
    let settle = |vcx: &mut gpui::VisualTestContext| {
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
    };
    for i in [0, 1, 2, 4, 5, 6, 7] {
        vcx.simulate_mouse_move(away, None, gpui::Modifiers::default());
        settle(vcx);
        vcx.simulate_mouse_move(rest[i], None, gpui::Modifiers::default());
        settle(vcx);
        workspace.read_with(vcx, |w, _| {
            let key = w.slots[i].panel.entity_id().as_u64();
            assert_eq!(w.live_tabs.hovered, Some(key), "pointer over slot {i}");
            assert_eq!(w.active, 3, "hover never changes focus");
        });
        let body = vcx
            .debug_bounds(format!("live-session-tab-{i}").leak())
            .unwrap();
        let slot = vcx
            .debug_bounds(format!("live-session-tab-{i}-slot").leak())
            .unwrap();
        assert!(
            slot.contains(&rest[i]),
            "popped body still holds the pointer"
        );
        assert_eq!(slot, body, "the hit area is exactly the popped body");
        let close = vcx
            .debug_bounds(format!("close-session-button-{i}").leak())
            .unwrap();
        action_tests::assert_close_fits_cap(body, close);
        // Travel to the X: hover stays on this tab the whole way.
        for step in 1..=4 {
            let t = step as f32 / 4.0;
            let x = rest[i].x + (close.center().x - rest[i].x) * t;
            vcx.simulate_mouse_move(
                gpui::point(x, close.center().y),
                None,
                gpui::Modifiers::default(),
            );
            settle(vcx);
            workspace.read_with(vcx, |w, _| {
                let key = w.slots[i].panel.entity_id().as_u64();
                assert_eq!(w.live_tabs.hovered, Some(key), "tab {i} stays popped");
            });
        }
    }
    // Clicking the X at the popped edge closes that tab, not a neighbour.
    vcx.simulate_mouse_move(away, None, gpui::Modifiers::default());
    settle(vcx);
    vcx.simulate_mouse_move(rest[5], None, gpui::Modifiers::default());
    settle(vcx);
    let close = vcx.debug_bounds("close-session-button-5").unwrap();
    vcx.simulate_mouse_move(close.center(), None, gpui::Modifiers::default());
    settle(vcx);
    vcx.simulate_click(close.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        let closed: Vec<_> = w
            .slots
            .iter()
            .filter(|slot| slot.closing)
            .map(|slot| slot.panel.read(cx).session_id.clone())
            .collect();
        assert_eq!(closed, ["hover-session-5"]);
    });
}

#[gpui::test]
fn workspace_number_stays_in_its_resting_slot_when_tab_pops_out(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for row in 0..3 {
            w.active_row = row;
            w.push_test_panel(&format!("workspace-{row}"), cx);
        }
        w.active_row = 0;
        w.active = 0;
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(480.0), px(600.0)));
    vcx.run_until_parked();
    let before = vcx.debug_bounds("workspace-tab-group-label-2").unwrap();
    workspace.update(vcx, |w, cx| {
        w.live_tabs.hovered = Some(w.slots[2].panel.entity_id().as_u64());
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.live_tabs.settle();
        cx.notify();
    });
    vcx.run_until_parked();
    assert_eq!(
        vcx.debug_bounds("workspace-tab-group-label-2").unwrap(),
        before
    );
    let tab = vcx.debug_bounds("live-session-tab-2").unwrap();
    assert!(
        tab.left() < before.right(),
        "pop-out must cover the resting badge"
    );
}

#[test]
fn stacked_tab_surfaces_occlude_underlying_titles_in_translucent_themes() {
    for alpha in [0.0, 0.35, 0.75, 1.0] {
        let color = gpui::Rgba {
            r: 0.12,
            g: 0.18,
            b: 0.24,
            a: alpha,
        };
        let surface = tab_surface(color);
        assert_eq!(surface.a, 1.0);
        assert_eq!(
            (surface.r, surface.g, surface.b),
            (color.r, color.g, color.b)
        );
    }
}

#[test]
fn hovered_tab_pops_out_on_top_without_leaving_the_pointer() {
    let order = TabLayout::hover_paint_order(4, 1, Some(3));
    assert_eq!(order.last(), Some(&3), "hovered tab paints above focus");
    assert_eq!(order.len(), 4);
    assert_eq!(TabLayout::hover_paint_order(4, 1, None).last(), Some(&1));
    for available in [100.0, 400.0, 1200.0] {
        for left in [0.0, 50.0, available - 40.0] {
            let rest = TabGeometry {
                left,
                width: 40.0,
                height: TAB_HEIGHT - 4.0,
            };
            let pop = TabLayout::pop_out(rest, 208.0, available);
            assert_eq!(pop.height, TAB_HEIGHT);
            assert!(pop.width >= rest.width);
            assert!(pop.left >= 0.0 && pop.left + pop.width <= available + 0.001);
            // Every point of the resting tab stays covered by the popped tab.
            assert!(pop.left <= rest.left + 0.001);
            assert!(pop.left + pop.width >= rest.left + rest.width - 0.001);
        }
    }
}

#[test]
fn live_tabs_share_crowded_width_and_keep_the_selection_largest() {
    for available in [0.0, 40.0, 180.0, 352.0, 800.0, 2400.0] {
        for count in 1..=200 {
            let layout = TabLayout::new(available, count);
            for selected in [0, count / 2, count - 1] {
                let first = layout.geometry(0, selected);
                let last = layout.geometry(count - 1, selected);
                assert!(first.left >= 0.0);
                assert!(last.left + last.width <= available + 0.001);
                assert!((first.left - (available - last.left - last.width)).abs() < 0.001);
                let active = layout.geometry(selected, selected);
                assert!(active.width >= layout.inactive_width);
                assert_eq!(
                    TabLayout::paint_order(count, selected).last(),
                    Some(selected)
                );
                if available > 0.0 && count > 1 {
                    assert!(layout.step > 0.0);
                }
            }
        }
    }
    let crowded = TabLayout::new(512.0, 12);
    assert!(crowded.active_width - crowded.inactive_width <= 12.0);
    assert!(crowded.step > 27.0, "inactive tab icons remain exposed");
    assert!(TabLayout::new(1152.0, 3).start > 0.0);
}

#[test]
fn live_tabs_center_two_and_four_panel_groups_with_subtle_focus_motion() {
    for available in [40.0, 192.0, 512.0, 1152.0, 1600.0] {
        for count in [2, 4, 6, 12, 200] {
            let layout = TabLayout::new(available, count);
            for selected in 0..count {
                let tabs: Vec<_> = (0..count).map(|i| layout.geometry(i, selected)).collect();
                let first = tabs[0];
                let last = tabs[count - 1];
                assert!((first.left + last.left + last.width - available).abs() < 0.001);
                for (i, tab) in tabs.iter().enumerate() {
                    assert!(tab.left >= 0.0);
                    assert!(tab.left + tab.width <= available + 0.001);
                    assert!(TabLayout::exposed(&tabs, i, selected).1 > 0.0);
                    let next = layout.geometry(i, (selected + 1) % count);
                    assert!((next.left - tab.left).abs() <= 12.001);
                    assert!((next.width - tab.width).abs() <= 12.001);
                    assert!((next.height - tab.height).abs() <= 4.001);
                }
            }
        }
    }
}

#[gpui::test]
fn live_tabs_two_panel_cluster_keeps_every_offscreen_tab_visible_and_clickable(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for i in 0..8 {
            w.push_test_panel(&format!("session-{i}"), cx);
            w.slots[i].width_fraction = 0.5;
            w.slots[i].animated_width = AnimatedValue::new(0.5, Duration::ZERO);
        }
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [1440.0, 1000.0, 640.0] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.0)));
        vcx.run_until_parked();
        for selected in [3, 4, 0, 7, 1, 6, 2, 5] {
            workspace.update(vcx, |w, cx| {
                w.camera_x[w.active_row] = w.camera_target[w.active_row];
                w.camera_started[w.active_row] = None;
                w.live_tabs.settle();
                cx.notify();
            });
            vcx.run_until_parked();
            let track = vcx.debug_bounds("live-session-tabs").unwrap();
            let targets = workspace.read_with(vcx, |w, _| w.live_tabs.hit_targets().to_vec());
            assert_eq!(targets.len(), 8, "off-screen panels must retain their tabs");
            for &(index, x) in &targets {
                let tab = vcx
                    .debug_bounds(Box::leak(
                        format!("live-session-tab-{index}").into_boxed_str(),
                    ))
                    .unwrap();
                assert!(tab.left() >= track.left() - px(0.01));
                assert!(tab.right() <= track.right() + px(0.01));
                assert!(tab.size.width > px(0.0));
                assert!(track.left() + px(x) > tab.left());
                assert!(track.left() + px(x) < tab.right());
            }
            let x = targets
                .iter()
                .find(|(index, _)| *index == selected)
                .unwrap()
                .1;
            vcx.simulate_click(
                gpui::point(track.left() + px(x), track.bottom() - px(12.0)),
                gpui::Modifiers::default(),
            );
            vcx.run_until_parked();
            assert_eq!(
                workspace.read_with(vcx, |w, _| w.active),
                selected,
                "window={width}, selected={selected}, track={track:?}, targets={targets:?}, camera={:?}",
                workspace.read_with(vcx, |w, _| (w.camera_x.clone(), w.camera_target.clone(), w.camera_dirty.clone()))
            );
        }
    }
}

#[gpui::test]
fn live_tabs_four_panel_group_stays_left_aligned_when_focus_changes(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for i in 0..4 {
            w.push_test_panel(&format!("panel-{i}"), cx);
            w.slots[i].width_fraction = 0.25;
            w.slots[i].animated_width = AnimatedValue::new(0.25, Duration::ZERO);
        }
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(1600.0), px(700.0)));
    vcx.run_until_parked();
    let selectors = [
        "live-session-tab-0",
        "live-session-tab-1",
        "live-session-tab-2",
        "live-session-tab-3",
    ];
    let initial = selectors.map(|selector| vcx.debug_bounds(selector).unwrap());
    let track = vcx.debug_bounds("live-session-tabs").unwrap();
    assert_eq!(initial[0].left() - px(TAB_GROUP_LABEL_WIDTH), track.left());
    for pair in initial.windows(2) {
        assert_eq!(pair[1].left() - pair[0].right(), px(TAB_GAP));
    }
    for selected in [1, 2, 3, 0] {
        vcx.simulate_click(initial[selected].center(), gpui::Modifiers::default());
        vcx.simulate_mouse_move(
            gpui::point(px(5.0), px(600.0)),
            None,
            gpui::Modifiers::default(),
        );
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            w.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        assert_eq!(workspace.read_with(vcx, |w, _| w.active), selected);
        for (i, selector) in selectors.iter().enumerate() {
            let tab = vcx.debug_bounds(selector).unwrap();
            assert!((tab.left() - initial[i].left()).abs() <= px(12.01));
            let panel = vcx
                .debug_bounds(["panel-0", "panel-1", "panel-2", "panel-3"][i])
                .unwrap();
            if cfg!(target_os = "macos") {
                assert_eq!(tab.center().y, track.top() + px(TAB_HEIGHT / 2.0));
            }
            assert!(tab.bottom() + px(TAB_FLOAT_GAP) <= panel.top());
        }
    }
}

/// Tab and camera targets must already be final while the closing surface
/// is still mounted. Retirement must not trigger a second layout move.
#[gpui::test]
fn closing_tabs_target_survivors_before_surface_retirement(cx: &mut gpui::TestAppContext) {
    for initial in [0, 2, 4] {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.show_sidebar = false;
            w.show_minimap = false;
            for i in 0..5 {
                w.push_test_panel(&format!("panel-{i}"), cx);
                w.slots[i].width_fraction = 1.0;
                w.slots[i].close_progress = AnimatedValue::new(1.0, Duration::from_secs(60));
            }
            w.set_active(initial, cx);
            w
        });
        vcx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.resolve_camera_target(1200.0);
                w.camera_x[0] = w.camera_target[0];
                let before_camera = w.camera_target[0];
                let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                w.live_tabs.settle();
                let closed_id = w.slots[initial].panel.entity_id().as_u64();
                w.close_panel(&ClosePanel, window, cx);
                w.resolve_camera_target(1200.0);
                let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                assert_eq!(w.slots.len(), 5, "surface is still fading");
                assert_eq!(w.live_tabs.tabs.len(), 4);
                assert!(!w.live_tabs.tabs.contains_key(&closed_id));
                assert!(w.live_tabs.hit_targets().iter().all(|(i, _)| *i != initial));
                assert!(
                    w.camera_target[0] <= before_camera + 0.01,
                    "closing must not pan right toward the successor's old position"
                );
                let targets: HashMap<_, _> = w
                    .live_tabs
                    .tabs
                    .iter()
                    .map(|(id, tab)| (*id, tab.target))
                    .collect();
                let camera = w.camera_target[0];
                w.remove_finished_closing_panels(
                    Instant::now() + Duration::from_secs(61),
                    window,
                    cx,
                );
                w.resolve_camera_target(1200.0);
                let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
                assert_eq!(w.slots.len(), 4);
                assert_eq!(w.camera_target[0], camera, "no second camera move");
                for (id, tab) in &w.live_tabs.tabs {
                    assert_eq!(tab.target, targets[id], "no second tab move");
                }
                assert!(!w.slots[w.active].closing);
                assert!(
                    w.slots[w.active]
                        .panel
                        .read(cx)
                        .input_focus_handle(cx)
                        .is_focused(window)
                );
            });
        });
    }
}

#[gpui::test]
fn closing_final_tab_shows_empty_workspace_before_surface_retirement(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("last", cx);
        w.slots[0].close_progress = AnimatedValue::new(1.0, Duration::from_secs(60));
        w
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.close_panel(&ClosePanel, window, cx);
            let _ = w.render_workspace_bar(1200.0, false, 0.0, window, cx);
            assert_eq!(w.slots.len(), 1);
            assert!(w.live_tabs.hit_targets().is_empty());
            assert_eq!(w.live_tabs.tabs.len(), 1);
            assert!(w.live_tabs.tabs.contains_key(&u64::MAX));
            assert!(w.focus_handle.is_focused(window));
        });
    });
}

#[gpui::test]
fn live_tabs_stay_still_while_the_panel_camera_moves(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.show_sidebar = false;
        for i in 0..4 {
            w.push_test_panel(&format!("panel-{i}"), cx);
        }
        w
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(1200.0), px(700.0)));
    vcx.run_until_parked();
    let selectors = [
        "live-session-tab-0",
        "live-session-tab-1",
        "live-session-tab-2",
        "live-session-tab-3",
    ];
    let initial = selectors.map(|selector| vcx.debug_bounds(selector).unwrap());
    let panel_left = vcx.debug_bounds("panel-1").unwrap().left();
    for camera in [80.0, 160.0, 480.0] {
        workspace.update(vcx, |w, cx| {
            w.camera_x[0] = camera;
            w.camera_target[0] = camera;
            w.camera_started[0] = None;
            w.camera_dirty[0] = false;
            cx.notify();
        });
        vcx.run_until_parked();
        for (i, selector) in selectors.iter().enumerate() {
            assert_eq!(vcx.debug_bounds(selector).unwrap(), initial[i]);
        }
        assert!(
            (panel_left - vcx.debug_bounds("panel-1").unwrap().left() - px(camera)).abs()
                < px(0.01)
        );
    }
}

#[test]
fn live_tabs_motion_expands_lifts_and_retargets_without_jumping() {
    let now = Instant::now();
    let duration = Duration::from_millis(150);
    let layout = TabLayout::new(512.0, 12);
    let targets = |selected| {
        (0..12)
            .map(|index| (index as u64, layout.geometry(index, selected)))
            .collect::<Vec<_>>()
    };
    let mut motion = TabMotion::default();
    let initial = motion.sample(&targets(0), 512.0, duration, now);
    assert!(!motion.is_animating());
    let start = motion.sample(&targets(5), 512.0, duration, now);
    assert_eq!(initial, start);
    assert!(motion.is_animating());
    let middle = motion.sample(
        &targets(5),
        512.0,
        duration,
        now + Duration::from_millis(50),
    );
    assert!(middle[5].width > initial[5].width && middle[5].width < 208.0);
    assert!(middle[5].height > TAB_HEIGHT - 4.0 && middle[5].height < TAB_HEIGHT);
    assert!(middle[5].left < initial[5].left);
    let reversal = motion.sample(
        &targets(0),
        512.0,
        duration,
        now + Duration::from_millis(50),
    );
    assert_eq!(middle, reversal);
    let settled = motion.sample(
        &targets(0),
        512.0,
        duration,
        now + Duration::from_millis(250),
    );
    assert_eq!(settled, initial);
    assert!(!motion.is_animating());
    let reduced = motion.sample(&targets(5), 512.0, Duration::ZERO, now);
    assert_eq!(reduced[5], layout.geometry(5, 5));
    assert!(!motion.is_animating());
    let smaller = TabLayout::new(192.0, 12);
    let targets: Vec<_> = (0..12)
        .map(|i| (i as u64, smaller.geometry(i, 5)))
        .collect();
    let resized = motion.sample(&targets, 192.0, duration, now);
    assert!(!motion.is_animating());
    assert!(
        resized
            .iter()
            .all(|tab| tab.left >= 0.0 && tab.left + tab.width <= 192.001)
    );
}

#[gpui::test]
fn live_tabs_switch_rows_and_stay_detached_from_panels(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.show_sidebar = false;
        for name in ["First session", "Second session", "Another workspace"] {
            workspace.push_test_panel(name, cx);
        }
        workspace.slots[2].row = 1;
        workspace
    });
    vcx.run_until_parked();
    let first = vcx.debug_bounds("live-session-tab-0").unwrap();
    let other = vcx.debug_bounds("live-session-tab-2").unwrap();
    if cfg!(target_os = "macos") {
        assert_eq!(first.center().y, other.center().y);
    } else {
        assert_eq!(first.bottom(), other.bottom());
    }
    assert_eq!(first.size.height - other.size.height, px(4.0));
    assert_eq!(
        first.bottom() + px(TAB_FLOAT_GAP),
        vcx.debug_bounds("panel-0").unwrap().top()
    );
    assert!(vcx.debug_bounds("workspace-bar").is_none());
    vcx.simulate_click(other.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, _| {
        assert_eq!(workspace.active, 2);
        assert_eq!(workspace.active_row, 1);
        assert!(!workspace.overview);
    });
    assert!(vcx.debug_bounds("live-session-tab-2-focused").is_some());
    assert!(vcx.debug_bounds("live-session-tab-0-focused").is_none());
}

#[gpui::test]
fn live_tabs_remain_reachable_during_resize_and_keyboard_navigation(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for index in 0..12 {
            workspace.push_test_panel(&format!("A long session title number {index}"), cx);
        }
        workspace
    });
    let handle = vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
        window.window_handle()
    });
    vcx.simulate_window_resize(handle, gpui::size(px(800.), px(600.)));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("super-p");
    vcx.run_until_parked();
    assert_eq!(workspace.read_with(vcx, |w, _| w.active), 11);
    let track = vcx.debug_bounds("live-session-tabs").unwrap();
    let selected = vcx.debug_bounds("live-session-tab-11").unwrap();
    assert!(selected.left() >= track.left());
    assert!(
        selected.right() <= track.right() + px(1.),
        "selected={selected:?}, track={track:?}"
    );
    workspace.update(vcx, |w, cx| {
        // Finish both independent animations before checking selection.
        w.camera_x[w.active_row] = w.camera_target[w.active_row];
        w.camera_started[w.active_row] = None;
        w.live_tabs.settle();
        cx.notify();
    });
    vcx.run_until_parked();
    assert_eq!(
        vcx.debug_bounds("live-session-tab-11").unwrap().size.width,
        px(TabLayout::grouped(f32::from(track.size.width), &[0; 12], 11)[11].width)
    );
    vcx.simulate_keystrokes("super-u");
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        // Finish both independent animations before checking selection.
        w.camera_x[w.active_row] = w.camera_target[w.active_row];
        w.camera_started[w.active_row] = None;
        w.live_tabs.settle();
        cx.notify();
    });
    vcx.run_until_parked();
    let first = vcx.debug_bounds("live-session-tab-0").unwrap();
    assert!(first.left() >= track.left());
    vcx.update(|window, cx| window.simulate_mouse_move(first.center(), cx));
    vcx.run_until_parked();
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("live-session-tab-tooltip").is_some());
    for width in [1440., 800., 640., 480., 1000.] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
        vcx.run_until_parked();
        // Resize retargets camera/tab motion. Click settled geometry rather
        // than bounds from an animation frame that can move before dispatch.
        workspace.update(vcx, |workspace, cx| {
            workspace.camera_x[workspace.active_row] =
                workspace.camera_target[workspace.active_row];
            workspace.camera_started[workspace.active_row] = None;
            workspace.live_tabs.settle();
            cx.notify();
        });
        vcx.run_until_parked();
        let track = vcx.debug_bounds("live-session-tabs").unwrap();
        let mut hidden = Vec::new();
        for index in 0..12 {
            let Some(tab) = vcx.debug_bounds(format!("live-session-tab-{index}").leak()) else {
                hidden.push(index);
                continue;
            };
            // Crowded folders retain exposed edges inside the centered group.
            assert!(tab.left() >= track.left());
            assert!(
                tab.right() <= track.right() + px(1.),
                "window={width} index={index} tab={tab:?} track={track:?}"
            );
            assert!(tab.size.width > px(0.) && tab.size.width <= px(208.));
            assert!(
                vcx.debug_bounds(Box::leak(
                    format!("live-session-tab-{index}-emoji").into_boxed_str()
                ))
                .is_some()
            );
        }
        let last = vcx.debug_bounds("live-session-tab-11").unwrap();
        let plus = vcx.debug_bounds("tab-new-session").unwrap();
        assert!(plus.left() >= last.right());
        assert!(vcx.debug_bounds("edge-new-session").is_none());
        let previous = vcx.debug_bounds("live-session-tab-10").unwrap();
        let x = track.left()
            + px(workspace.read_with(vcx, |w, _| {
                w.live_tabs
                    .hit_targets
                    .iter()
                    .find(|(i, _)| *i == 11)
                    .unwrap()
                    .1
            }));
        vcx.simulate_click(gpui::point(x, last.center().y), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(
            workspace.read_with(vcx, |w, _| w.active),
            11,
            "width={width}, last={last:?}, previous={previous:?}, x={x:?}"
        );
        workspace.update_in(vcx, |workspace, window, cx| {
            assert_eq!(workspace.navigation_state(window, cx)["keyboard_panel"], 11);
            assert_eq!(workspace.test_coach().trace("new_panel").slow_paths, 0);
        });
        if width > 480. {
            assert!(hidden.is_empty(), "window={width} hid tabs {hidden:?}");
        }
        if let Some(trigger) = vcx.debug_bounds("tab-overflow") {
            assert!(!hidden.is_empty());
            vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            for index in 0..12 {
                assert_eq!(
                    vcx.debug_bounds(format!("tab-overflow-item-{index}").leak())
                        .is_some(),
                    hidden.contains(&index),
                    "window={width} overflow availability for session {index}"
                );
            }
            let index = hidden[0];
            let item = vcx
                .debug_bounds(format!("tab-overflow-item-{index}").leak())
                .unwrap();
            vcx.simulate_click(item.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.active), index);
            assert!(
                vcx.debug_bounds(format!("live-session-tab-{index}").leak())
                    .is_some()
            );
            assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
            vcx.simulate_keystrokes("super-p");
            vcx.run_until_parked();
            assert_eq!(workspace.read_with(vcx, |w, _| w.active), 11);
        } else {
            assert!(hidden.is_empty(), "tabs {hidden:?} are unreachable");
        }
    }
}

#[gpui::test]
fn overflow_keeps_workspace_labels_and_promotes_the_chosen_session(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for index in 0..16 {
            workspace.push_test_panel(&format!("Overflow session {index}"), cx);
            workspace.slots[index].row = index / 8;
        }
        workspace
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(420.0), px(700.0)));
    vcx.run_until_parked();
    let trigger = vcx.debug_bounds("tab-overflow").unwrap();
    assert!(trigger.right() <= vcx.debug_bounds("workspace-files-toggle").unwrap().left());
    assert!(vcx.debug_bounds("workspace-tab-group-label-0").is_some());
    assert!(vcx.debug_bounds("workspace-tab-group-label-1").is_some());
    let hidden = workspace.read_with(vcx, |w, _| {
        (0..16)
            .find(|index| {
                !w.live_tabs
                    .hit_targets
                    .iter()
                    .any(|(visible, _)| visible == index)
            })
            .unwrap()
    });
    vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tab-overflow-menu").is_some());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
    vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let item = vcx
        .debug_bounds(format!("tab-overflow-item-{hidden}").leak())
        .unwrap();
    vcx.simulate_click(item.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("tab-overflow-menu").is_none());
    assert_eq!(workspace.read_with(vcx, |w, _| w.active), hidden);
    assert!(
        vcx.debug_bounds(format!("live-session-tab-{hidden}").leak())
            .is_some()
    );
}

#[gpui::test]
fn live_tabs_reserve_space_in_normal_mode_and_beside_minimap(cx: &mut gpui::TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.layout_mode = crate::config::LayoutMode::Normal;
        workspace.enable_test_minimap();
        for index in 0..24 {
            workspace.push_test_panel(&format!("Session {index}"), cx);
        }
        workspace
    });
    vcx.run_until_parked();
    let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
    let minimap = vcx.debug_bounds("minimap").unwrap();
    if cfg!(target_os = "macos") {
        assert!(tabs.bottom() < minimap.top());
    } else {
        assert!(tabs.right() < minimap.left());
    }
    assert!(tabs.bottom() <= vcx.debug_bounds("panel-0").unwrap().top());
    for index in 0..24 {
        let tab = vcx
            .debug_bounds(Box::leak(
                format!("live-session-tab-{index}").into_boxed_str(),
            ))
            .unwrap();
        assert!(tab.left() >= tabs.left());
        assert!(tab.right() <= tabs.right() + px(1.));
    }
}

#[gpui::test]
fn empty_workspace_does_not_mark_another_rows_session_as_focused(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("Session", cx);
        workspace.select_row(2, 0);
        workspace
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("live-session-empty-tab").is_some());
    assert!(vcx.debug_bounds("live-session-tab-0-focused").is_none());
    let tab = vcx.debug_bounds("live-session-tab-0").unwrap();
    vcx.simulate_click(tab.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(workspace.read_with(vcx, |w, _| w.active_row), 0);
    assert!(vcx.debug_bounds("live-session-empty-tab").is_none());
    assert!(vcx.debug_bounds("live-session-tab-0-focused").is_some());
}
