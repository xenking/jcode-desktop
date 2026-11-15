use super::*;

#[gpui::test]
fn method_pill_selects_existing_provider_inline(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        let route = |model: &str, method: &str| jcode_sdk::ModelRouteInfo {
            model: model.into(),
            provider: String::new(),
            api_method: method.into(),
            available: true,
            detail: String::new(),
            usage: None,
        };
        panel.apply(
            &ApiEvent::RuntimeInfo {
                session_id: "session-a".into(),
                provider: Some("openai".into()),
                model: Some("gpt-5.6-sol".into()),
                reasoning_effort: None,
                auth_method: Some("oauth".into()),
                account_label: None,
                routes: vec![
                    route("gpt-5.6-sol", "openai-oauth"),
                    route("claude-fable-5", "claude-oauth"),
                ],
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let pill = vcx.debug_bounds("panel-login").unwrap();
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let picker = vcx
        .debug_bounds("composer-provider-picker")
        .expect("inline picker");
    let input = vcx.debug_bounds("prompt-input").unwrap();
    assert!(picker.bottom() <= input.top());
    assert!(vcx.debug_bounds("accounts-panel").is_none());
    assert!(
        vcx.debug_bounds("login-close").is_none(),
        "no Accounts sign-in UI"
    );
    let row = vcx
        .debug_bounds("login-provider-claude")
        .expect("connected provider row is shown");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::SetModel { session_id, model })
            if session_id == "session-a" && model == "claude-oauth:claude-fable-5"
    ));
    assert!(vcx.debug_bounds("composer-provider-picker").is_none());
    assert!(vcx.debug_bounds("login-busy").is_none());
}

#[gpui::test]
fn wheel_over_provider_picker_does_not_scroll_transcript(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        for n in 0..80 {
            panel
                .items
                .push(crate::panel::Item::Assistant(format!("message {n}")));
        }
        cx.notify();
    });
    vcx.run_until_parked();
    let pill = vcx.debug_bounds("panel-login").unwrap();
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let picker = vcx
        .debug_bounds("composer-provider-picker")
        .expect("inline picker");
    // Leave the live tail first so tail following cannot mask a leak.
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(picker.center().x, picker.top() - px(80.)),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., 6.)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Started,
    });
    for _ in 0..120 {
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
    }
    vcx.run_until_parked();
    let before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    assert!(!panel.read_with(vcx, |panel, _| panel.stick_to_bottom));
    let points = [
        gpui::point(picker.left() + px(6.), picker.top() + px(4.)),
        picker.center(),
        gpui::point(picker.right() - px(6.), picker.bottom() - px(4.)),
    ];
    for (point, precise) in points
        .into_iter()
        .flat_map(|point| [(point, true), (point, false)])
    {
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: point,
            delta: if precise {
                gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(120.)))
            } else {
                gpui::ScrollDelta::Lines(gpui::point(0., 3.))
            },
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        for _ in 0..120 {
            vcx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            vcx.update(|window, cx| window.simulate_next_frame(cx));
        }
        vcx.run_until_parked();
    }
    let after = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
    assert_eq!(
        before, after,
        "wheel over the picker must not scroll the transcript"
    );
    // The search row holds the Close button; no separate title row.
    let close = vcx.debug_bounds("provider-picker-close").expect("close");
    let search = vcx
        .debug_bounds("provider-picker-search")
        .expect("search row");
    assert!(close.top() >= search.top() && close.bottom() <= search.bottom());
}
