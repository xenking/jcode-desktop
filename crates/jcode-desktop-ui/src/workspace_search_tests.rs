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
