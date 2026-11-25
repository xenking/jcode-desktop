//! Workspace test suite. Shared helpers live here, scenarios in the children.
use super::*;

thread_local! {
    pub(super) static SIDEBAR_TITLE_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn session_info(id: &str, title: Option<&str>) -> jcode_sdk::SessionInfo {
    jcode_sdk::SessionInfo {
        user_connected: false,
        session_id: id.into(),
        working_dir: None,
        title: title.map(str::to_owned),
        status: "idle".into(),
        transcript_bytes: None,
        saved: false,
        updated_at_ms: None,
        last_active_at_ms: None,
        archived: false,
        archived_at_ms: None,
        save_label: None,
        parent_session_id: None,
        agent_label: None,
        swarm_status: None,
        edit_stats: None,
        context: None,
    }
}

pub(super) fn sync_applets(workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, app| {
        workspace.update(app, |workspace, cx| {
            workspace.sync_applet_panels(window, cx)
        });
    });
    cx.run_until_parked();
}

pub(super) fn click_sidebar_navigation(
    workspace: &Entity<Workspace>,
    cx: &mut gpui::VisualTestContext,
    selector: &'static str,
) {
    if workspace.read_with(cx, |w, _| {
        w.layout_mode == crate::config::LayoutMode::FolderTabs
    }) {
        let header = cx.debug_bounds("sidebar-section-trigger").unwrap();
        cx.update(|window, cx| window.simulate_mouse_move(header.center(), cx));
        cx.run_until_parked();
        let expanded: &'static str = Box::leak(format!("{selector}-expanded").into_boxed_str());
        let choice = cx
            .debug_bounds(expanded)
            .unwrap_or_else(|| panic!("missing sidebar choice {expanded}"));
        let trigger = cx.debug_bounds("sidebar-section-trigger").unwrap();
        assert!(
            choice.left() >= trigger.right(),
            "menu opens beside the sidebar"
        );
        let popout = cx.debug_bounds("sidebar-roller-popout").unwrap();
        cx.simulate_click(choice.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        // Leave the header and its menu before using the page.
        cx.update(|window, cx| {
            window.simulate_mouse_move(gpui::point(px(130.0), popout.bottom() + px(40.0)), cx);
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds(expanded).is_none());
        return;
    }

    // Use the real navigation scroll path so tabs beyond the narrow sidebar
    // viewport must actually become reachable before clicking them.
    for _ in 0..20 {
        let tabs = cx.debug_bounds("sidebar-navigation-tabs").unwrap();
        let tab = cx.debug_bounds(selector).unwrap();
        if tab.left() >= tabs.left() && tab.right() <= tabs.right() {
            cx.simulate_click(tab.center(), gpui::Modifiers::default());
            cx.run_until_parked();
            if selector.ends_with("-tab") {
                let selected = cx.debug_bounds(selector).unwrap();
                let directory = cx.debug_bounds("default-directory-button").unwrap();
                assert_eq!(selected.bottom(), directory.top());
                assert_eq!(selected.size.height, px(34.0));
            }
            return;
        }
        let delta = if tab.left() < tabs.left() {
            40.0
        } else {
            -40.0
        };
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: tabs.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(delta), px(0.0))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        cx.run_until_parked();
    }
    panic!("sidebar tab {selector} was not reachable");
}

/// The platform spelling of the workspace modifier. macOS binds `cmd`
/// explicitly because ScrollWM owns the Option-key motions there.
#[cfg(target_os = "macos")]
pub(super) const MOD: &str = "cmd";
#[cfg(not(target_os = "macos"))]
pub(super) const MOD: &str = "super";

pub(super) fn focused_workspace(
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<Workspace>, &mut gpui::VisualTestContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, cx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    cx.update(|window, cx| {
        let handle = workspace.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    (workspace, cx)
}

/// Stand in for the bundled Gmail provider: register it, then answer the
/// launch Desktop queued by mounting a panel, like provider.py does.
pub(super) fn fake_gmail_provider(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, app| {
        let runtime = crate::applet_runtime::get(app);
        runtime
            .apply(
                GMAIL_APPLET,
                serde_json::from_value(serde_json::json!({
                    "type": "register",
                    "manifest": {
                        "schema": jcode_applet_types::SCHEMA,
                        "id": GMAIL_APPLET,
                        "title": "Gmail",
                        "launchers": [{"trigger": "sidebar", "placement": {"kind": "panel"}}]
                    }
                }))
                .unwrap(),
            )
            .unwrap();
        runtime
            .apply(
                GMAIL_APPLET,
                serde_json::from_value(serde_json::json!({
                    "type": "mount",
                    "instance": "gmail#launch0",
                    "placement": {"kind": "panel"},
                    "document": {"revision": 1, "title": "Gmail",
                        "view": {"type": "text", "text": "Inbox"}}
                }))
                .unwrap(),
            )
            .unwrap();
        let _ = window;
    });
}

#[path = "workspace_session_catalog_tests.rs"]
mod session_catalog;
#[path = "workspace_sidebar_tests.rs"]
mod sidebar;
#[path = "workspace_folder_strip_tests.rs"]
mod folder_strip;
#[path = "workspace_shortcut_tests.rs"]
mod shortcut;
#[path = "workspace_tutorial_tests.rs"]
mod tutorial;
#[path = "workspace_accounts_panel_tests.rs"]
mod accounts_panel;
