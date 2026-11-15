//! The global search screen owns the window while it is open.
//!
//! The row/selection/debounce logic is tested in `jcode-desktop-model`; these
//! tests prove the wiring a model test cannot: that the screen replaces the
//! workspace instead of floating over it, that the sidebar magnifier opens it,
//! and that Escape and the keybinding behave the way the hint text claims.

use super::*;

#[gpui::test]
fn search_screen_takes_the_whole_window_and_escape_restores_the_workspace(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    let canvas = vcx.debug_bounds("workspace-canvas").expect("canvas rendered");
    assert!(
        vcx.debug_bounds("global-search-screen").is_none(),
        "the screen is closed until asked for"
    );

    workspace.update_in(vcx, |w, window, cx| {
        w.open_search_screen(&GlobalSearch, window, cx);
        assert!(w.search_screen_open());
    });
    vcx.run_until_parked();

    let screen = vcx
        .debug_bounds("global-search-screen")
        .expect("the search screen rendered");
    assert!(
        vcx.debug_bounds("workspace-canvas").is_none(),
        "a full-height screen replaces the canvas rather than floating over it"
    );
    assert!(
        screen.size.height >= canvas.size.height * 0.95 && screen.size.width >= canvas.size.width
            * 0.95,
        "screen {screen:?} must fill the {canvas:?} the canvas had"
    );
    assert!(
        vcx.debug_bounds("global-search-input").is_some(),
        "the single query field is focused chrome, not a popover trigger"
    );
    assert!(
        vcx.debug_bounds("global-search-results").is_some(),
        "the results list is present before the first query"
    );

    // Escape is the documented way out, and it is a real key path rather than
    // a close button only.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        !workspace.read_with(vcx, |w, _| w.search_screen_open()),
        "escape closes the screen"
    );
    assert!(
        vcx.debug_bounds("workspace-canvas").is_some(),
        "the workspace comes back"
    );
}

#[gpui::test]
fn sidebar_magnifier_opens_the_search_screen(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w.show_sidebar = true;
        w
    });
    vcx.run_until_parked();
    // Compact and wide sidebars expose the same action in different chrome, so
    // accept either shape and click it.
    let button = vcx
        .debug_bounds("sidebar-search-icon")
        .or_else(|| vcx.debug_bounds("sidebar-search"))
        .expect("the sidebar renders a magnifier");
    assert!(!workspace.read_with(vcx, |w, _| w.search_screen_open()));
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        workspace.read_with(vcx, |w, _| w.search_screen_open()),
        "clicking the magnifier opens global search"
    );
}

/// The screen is useless if the query field never receives keystrokes. This
/// goes through the real dispatch path (keybindings, capture/bubble, IME)
/// instead of setting the text, because the bug this pins was invisible to a
/// test that assigned `content` directly.
#[gpui::test]
fn typing_into_the_search_screen_reaches_the_query_field(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    workspace.update_in(vcx, |w, window, cx| {
        w.open_search_screen(&GlobalSearch, window, cx);
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    let typed = workspace.read_with(vcx, |w, cx| {
        w.search_screen
            .as_ref()
            .map(|state| state.input.read(cx).content.to_string())
            .unwrap_or_default()
    });
    assert_eq!(typed, "snap", "typed characters must land in the query field");
}

fn search_query(workspace: &gpui::Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> String {
    workspace.read_with(cx, |w, cx| {
        w.search_screen
            .as_ref()
            .map(|state| state.input.read(cx).content.to_string())
            .unwrap_or_default()
    })
}

fn open_search(workspace: &gpui::Entity<Workspace>, vcx: &mut gpui::VisualTestContext) {
    workspace.update_in(vcx, |w, window, cx| {
        w.open_search_screen(&GlobalSearch, window, cx);
    });
    vcx.run_until_parked();
}

/// Clicking the screen's own chrome (the padding around the field, the counts,
/// the empty results area) used to hand focus back to the workspace handle the
/// screen root tracks: the caret vanished and every later keystroke was lost.
/// The single field must keep the keyboard wherever the click landed.
#[gpui::test]
fn clicking_the_search_chrome_keeps_the_query_field_typing(cx: &mut gpui::TestAppContext) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    let list = vcx
        .debug_bounds("global-search-results")
        .expect("the results area is on screen");
    // Empty results area: part of the screen, but not part of the field. This
    // click used to hand focus to the workspace handle the root tracks.
    vcx.simulate_click(
        gpui::point(list.center().x, list.bottom() - px(40.0)),
        gpui::Modifiers::default(),
    );
    vcx.run_until_parked();
    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    assert_eq!(
        search_query(&workspace, &mut vcx),
        "snap",
        "a click anywhere on the screen must leave the query field typing"
    );
}

/// `focus_active` runs after shortcuts and layout changes and targets a panel
/// that the full-window screen does not render. Moving focus there leaves a
/// stale caret on the field and swallows everything typed afterwards.
#[gpui::test]
fn focusing_the_active_panel_while_searching_keeps_the_query_field_typing(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    workspace.update_in(vcx, |w, window, cx| w.focus_active(window, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("s n a p");
    vcx.run_until_parked();

    assert_eq!(
        search_query(&workspace, &mut vcx),
        "snap",
        "the search field keeps the keyboard while the screen is open"
    );
    assert!(
        workspace.read_with(vcx, |w, _| w.search_screen_open()),
        "regaining focus must not close or replace the screen"
    );
}

/// The screen replaces the canvas, so it draws where the window's titlebar is.
/// On macOS that puts its heading under the traffic lights unless the screen
/// reserves the same inset the single-panel workspace does.
#[gpui::test]
fn the_search_heading_clears_the_titlebar_inset(cx: &mut gpui::TestAppContext) {
    let (workspace, mut vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("chat", cx);
        w
    });
    vcx.run_until_parked();
    open_search(&workspace, &mut vcx);

    let screen = vcx
        .debug_bounds("global-search-screen")
        .expect("the search screen rendered");
    let title = vcx
        .debug_bounds("global-search-title")
        .expect("the heading rendered");
    let inset = content_top_inset(false, false);
    assert!(
        title.top() >= screen.top() + px(inset),
        "heading {title:?} must sit below the {inset}px titlebar inset inside {screen:?}"
    );
}
