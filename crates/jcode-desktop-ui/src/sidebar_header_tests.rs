use super::*;

#[gpui::test]
fn session_row_keeps_right_inset_and_context_menu_without_dots(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        let lead = tests::session_info("lead", Some("lead"));
        w.sessions = vec![lead.clone()];
        w.active = w.open_session(lead, cx);
        cx.notify();
    });
    vcx.run_until_parked();
    let row = vcx.debug_bounds("sidebar-session-0").unwrap();
    vcx.update(|window, cx| window.simulate_mouse_move(row.center(), cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-collection-button-0").is_none());
    let close = vcx.debug_bounds("sidebar-close-0").unwrap();
    assert_eq!(
        row.right() - close.right(),
        px(crate::scrollbar::GUTTER + 8.0)
    );
    let title = vcx.debug_bounds("sidebar-session-title-0").unwrap();
    vcx.simulate_mouse_down(title.center(), gpui::MouseButton::Right, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-collection-menu").is_some());
}
