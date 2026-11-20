use super::*;

#[gpui::test]
fn tab_row_starts_after_window_controls_and_reclaims_fps_space(cx: &mut gpui::TestAppContext) {
    let (_, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.show_sidebar = true;
        for index in 0..12 {
            workspace.push_test_panel(&format!("tabbar-{index}"), cx);
        }
        workspace
    });
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, gpui::size(px(1120.0), px(700.0)));
    for fullscreen in [false, true] {
        if fullscreen {
            vcx.update(|window, _| window.toggle_fullscreen());
            vcx.update(|window, _| window.refresh());
        }
        vcx.run_until_parked();
        let row = vcx.debug_bounds("workspace-tab-row").unwrap();
        let workspace_one = vcx.debug_bounds("workspace-tab-group-label-0").unwrap();
        let first = vcx.debug_bounds("live-session-tab-0").unwrap();
        let titled = (0..12)
            .filter(|index| {
                vcx.debug_bounds(Box::leak(
                    format!("live-session-tab-{index}-title").into_boxed_str(),
                ))
                .is_some()
            })
            .count();
        let inset = if cfg!(target_os = "macos") && !fullscreen {
            76.0
        } else {
            0.0
        };
        if cfg!(target_os = "macos") {
            assert_eq!(f32::from(workspace_one.left()), inset);
        }
        assert_eq!(first.left(), workspace_one.right());
        let track = vcx.debug_bounds("live-session-tabs").unwrap();
        assert_eq!(workspace_one.left(), track.left());
        if cfg!(target_os = "macos") && !fullscreen {
            let next = vcx
                .debug_bounds("compact-sidebar-toggle")
                .unwrap_or_else(|| vcx.debug_bounds("tab-next-workspace-slot").unwrap());
            let caption = vcx.debug_bounds("macos-caption-drag").unwrap();
            assert!(next.left() - track.right() >= px(16.0));
            assert!(caption.contains(&gpui::point(track.right() + px(8.0), track.center().y)));
        }
        assert!(track.left() >= row.left() + px(inset));
        assert!(vcx.debug_bounds("fps-counter").is_none());
        assert!(vcx.debug_bounds("fps-counter-slot").is_none());
        if cfg!(target_os = "macos") {
            assert!(
                titled == 12,
                "only {titled} of 12 tabs have a title at 1120px"
            );
        }
    }
}

#[gpui::test]
fn workspace_controls_share_one_row_without_fps_or_footer_metadata(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("fps-layout", cx);
        workspace
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for width in [1440.0, 1000.0, 800.0, 640.0, 420.0, 360.0] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(700.0)));
        for layout in [
            crate::config::LayoutMode::FolderTabs,
            crate::config::LayoutMode::Normal,
        ] {
            for sidebar in [true, false] {
                for minimap in [true, false] {
                    workspace.update(vcx, |workspace, cx| {
                        workspace.layout_mode = layout;
                        workspace.show_sidebar = sidebar;
                        workspace.show_minimap = minimap;
                        cx.notify();
                    });
                    vcx.run_until_parked();
                    let row = vcx.debug_bounds("workspace-tab-row").unwrap();
                    let menu = vcx.debug_bounds("compact-sidebar-toggle");
                    assert_eq!(menu.is_some(), !sidebar || responsive::is_compact(width));
                    assert!(vcx.debug_bounds("fps-counter").is_none());
                    let tabs = vcx.debug_bounds("live-session-tabs").unwrap();
                    let plus = vcx.debug_bounds("tab-new-session").unwrap();
                    let body = vcx.debug_bounds("workspace-body").unwrap();
                    let canvas = vcx.debug_bounds("workspace-canvas").unwrap();
                    assert_eq!(
                        vcx.debug_bounds("minimap").is_some(),
                        minimap && live_tabs::minimap_fits_header(f32::from(canvas.size.width)),
                        "minimap yields to navigation without changing its saved preference"
                    );
                    assert_eq!(workspace.read_with(vcx, |w, _| w.show_minimap), minimap);
                    assert!(vcx.debug_bounds("fps-header").is_none());
                    assert!(vcx.debug_bounds("edge-new-session").is_none());
                    assert_eq!(body.top(), px(0.0));
                    assert_eq!(row.top(), canvas.top() + px(STRIP_PADDING_TOP));
                    assert_eq!(row.size.height, px(FOLDER_CONTENT_INSET));
                    if let Some(menu) = menu {
                        if cfg!(target_os = "macos") {
                            assert!(menu.left() >= tabs.right() && menu.right() <= plus.left());
                        } else {
                            assert!(menu.left() >= row.left() && menu.right() <= tabs.left());
                        }
                        assert!(menu.top() >= row.top() && menu.bottom() <= row.bottom());
                        assert!((menu.center().y - plus.center().y).abs() < px(1.0));
                    }
                    assert!(
                        plus.left() >= tabs.right() && plus.right() <= row.right(),
                        "width={width}, sidebar={sidebar}, minimap={minimap}, plus={plus:?}, tabs={tabs:?}, row={row:?}"
                    );
                    assert_eq!(plus.top(), tabs.top());
                    assert_eq!(plus.size.height, px(28.0));
                    assert!(
                        (tabs.left()
                            - canvas.left()
                            - px(workspace.read_with(vcx, |w, _| w.live_tabs.header_offset)))
                        .abs()
                            < px(1.0),
                        "native navigation coordinates exclude footer metadata"
                    );
                    assert!(
                        tabs.size.width > px(0.0),
                        "width={width}, sidebar={sidebar}, minimap={minimap}"
                    );
                    assert!(
                        vcx.debug_bounds("workspace-version").is_none(),
                        "the chrome never carries a version strip"
                    );
                    assert!(vcx.debug_bounds("workspace-build").is_none());
                    if sidebar && !responsive::is_compact(width) {
                        assert_eq!(
                            vcx.debug_bounds("sidebar").unwrap().top(),
                            if cfg!(target_os = "macos") {
                                row.bottom()
                            } else {
                                body.top()
                            },
                        );
                    }
                }
            }
        }
    }
}
