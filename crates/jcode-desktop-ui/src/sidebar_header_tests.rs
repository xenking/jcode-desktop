use super::*;

#[gpui::test]
fn header_badge_hover_has_a_row_for_each_running_worker_and_right_click_keeps_menu(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| {
        setup(w, cx, &["running", "ready", "running"]);
        w.worker_meta.insert(
            "worker-0".into(),
            jcode_sdk::WorkerMeta {
                model: Some("test-model".into()),
                ..Default::default()
            },
        );
    });
    vcx.run_until_parked();
    let badge = vcx.debug_bounds("sidebar-agents-0").unwrap();
    assert!(vcx.debug_bounds("sidebar-agents-count-0-2").is_some());
    vcx.update(|window, cx| window.simulate_mouse_move(badge.center(), cx));
    vcx.run_until_parked();
    vcx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-workers-tooltip").is_some());
    assert!(vcx.debug_bounds("sidebar-workers-tooltip-row-0").is_some());
    assert!(vcx.debug_bounds("sidebar-workers-tooltip-row-1").is_some());
    assert!(vcx.debug_bounds("sidebar-workers-tooltip-row-2").is_none());
    let title = vcx.debug_bounds("sidebar-session-title-0").unwrap();
    vcx.simulate_mouse_down(title.center(), gpui::MouseButton::Right, Default::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-collection-menu").is_some());
}

fn setup(w: &mut Workspace, cx: &mut Context<Workspace>, statuses: &[&str]) {
    let lead = tests::session_info("lead", Some("lead"));
    let mut sessions = vec![lead.clone()];
    for (i, status) in statuses.iter().enumerate() {
        let mut worker = tests::session_info(&format!("worker-{i}"), Some("worker"));
        worker.parent_session_id = Some("lead".into());
        worker.swarm_status = Some((*status).into());
        sessions.push(worker);
    }
    w.sessions = sessions;
    w.active = w.open_session(lead, cx);
    cx.notify();
}

#[gpui::test]
fn header_hides_badge_without_running_workers(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| setup(w, cx, &["ready", "blocked", "queued"]));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-agents-0").is_none());
}

#[gpui::test]
fn header_actions_keep_badge_fixed_and_aligned(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| setup(w, cx, &["running", "ready"]));
    vcx.run_until_parked();
    let badge = vcx.debug_bounds("sidebar-agents-0").unwrap();
    let row = vcx.debug_bounds("sidebar-session-0").unwrap();
    vcx.update(|window, cx| window.simulate_mouse_move(row.center(), cx));
    vcx.run_until_parked();
    let hovered_badge = vcx.debug_bounds("sidebar-agents-0").unwrap();
    assert!(vcx.debug_bounds("sidebar-agents-count-0-1").is_some());
    assert_eq!(badge.right(), hovered_badge.right());
    assert!(vcx.debug_bounds("sidebar-collection-button-0").is_none());
    let close = vcx.debug_bounds("sidebar-close-0").unwrap();
    let pin = vcx.debug_bounds("sidebar-pin-0").unwrap();
    assert_eq!(pin.size, close.size);
    assert_eq!(close.center().y, badge.center().y);
    assert_eq!(
        row.right() - close.right(),
        px(crate::scrollbar::GUTTER + 8.0)
    );
}

#[gpui::test]
fn header_badge_click_reveals_workers(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |w, cx| setup(w, cx, &["running"; 100]));
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.sidebar_sessions_list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(10.0),
        });
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| {
        assert!(w.sidebar_sessions_list.logical_scroll_top().offset_in_item > px(0.0))
    });
    let badge = vcx.debug_bounds("sidebar-agents-0").unwrap();
    vcx.simulate_click(badge.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-workers-0").is_some());
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.sidebar_sessions_list.logical_scroll_top().item_ix, 0);
        assert_eq!(
            w.sidebar_sessions_list.logical_scroll_top().offset_in_item,
            px(0.0)
        );
    });
}
