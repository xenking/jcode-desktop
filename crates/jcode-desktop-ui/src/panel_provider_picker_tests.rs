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
    // The provider picker opens from the sidebar's quota row now; the bar no
    // longer carries a method pill, so the test calls the same entry point.
    vcx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_provider_picker(window, cx));
    });
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
    // The provider picker opens from the sidebar's quota row now; the bar no
    // longer carries a method pill, so the test calls the same entry point.
    vcx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_provider_picker(window, cx));
    });
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

/// Switch Claude -> OpenCode Go -> Claude through the inline picker, with the
/// session catalog listing only the current runtime's routes (as after a real
/// switch). The way back must stay clickable, the active row is marked, and a
/// provider without a working key is dimmed with a reason and sends nothing.
#[gpui::test]
fn picker_switches_there_and_back_and_marks_row_states(cx: &mut gpui::TestAppContext) {
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
    let route = |model: &str, provider: &str, method: &str| jcode_sdk::ModelRouteInfo {
        model: model.into(),
        provider: provider.into(),
        api_method: method.into(),
        available: true,
        detail: String::new(),
        usage: None,
    };
    let runtime = |provider: &str, model: &str, auth: &str, routes| ApiEvent::RuntimeInfo {
        session_id: "session-a".into(),
        provider: Some(provider.into()),
        model: Some(model.into()),
        reasoning_effort: None,
        auth_method: Some(auth.into()),
        account_label: None,
        routes,
    };
    // The composer bar no longer carries a login pill (usage moved to the
    // sidebar account block), so open the picker the way that block does.
    let open_picker = |vcx: &mut gpui::VisualTestContext| {
        vcx.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.toggle_provider_picker(window, cx));
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-provider-picker").is_some());
    };
    let search = |panel: &gpui::Entity<Panel>, vcx: &mut gpui::VisualTestContext, query: &str| {
        // The row list scrolls at 240px; narrow it so the row is on screen.
        panel.update(vcx, |panel, cx| {
            panel
                .login
                .as_ref()
                .unwrap()
                .search
                .update(cx, |input, cx| input.set_content(query.into(), cx));
            cx.notify();
        });
        vcx.run_until_parked();
    };
    let set_statuses = |panel: &gpui::Entity<Panel>, vcx: &mut gpui::VisualTestContext| {
        panel.update(vcx, |panel, cx| {
            let state = panel.login.as_mut().unwrap();
            state.status_task = None;
            state.status_loading = false;
            state.statuses = Some(
                [
                    ("claude".into(), ConnectionStatus::Connected),
                    ("opencode-go".into(), ConnectionStatus::Unverified),
                    ("openrouter".into(), ConnectionStatus::Failed),
                ]
                .into(),
            );
            cx.notify();
        });
        vcx.run_until_parked();
    };
    // Start on Claude; the catalog lists only Claude routes.
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &runtime(
                "claude",
                "claude-opus-5-5",
                "oauth",
                vec![route("claude-opus-5-5", "Anthropic", "claude-oauth")],
            ),
            cx,
        );
    });
    vcx.run_until_parked();
    open_picker(vcx);
    set_statuses(&panel, vcx);
    assert!(
        vcx.debug_bounds("picker-active-claude").is_some(),
        "active row marked"
    );
    assert!(
        vcx.debug_bounds("picker-available-opencode-go").is_some(),
        "OpenCode Go switchable even though this catalog lists no route for it"
    );
    assert!(
        vcx.debug_bounds("picker-unavailable-openrouter").is_some(),
        "rejected key is dimmed with a reason"
    );
    // A rejected provider sends nothing.
    search(&panel, vcx, "openrouter");
    let bad = vcx.debug_bounds("login-provider-openrouter").unwrap();
    vcx.simulate_click(bad.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "unavailable row must not switch"
    );
    // Claude -> OpenCode Go.
    search(&panel, vcx, "opencode");
    let go = vcx.debug_bounds("login-provider-opencode-go").unwrap();
    vcx.simulate_click(go.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let sent = commands.recv_timeout(std::time::Duration::from_millis(100));
    assert!(
        matches!(&sent, Ok(Command::SetModel { model, .. }) if model.starts_with("opencode-go:")),
        "expected an OpenCode Go switch"
    );
    // The daemon moved the session; its catalog now lists only OpenCode Go.
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &runtime(
                "OpenCode Go",
                "deepseek-v4.1-flash",
                "API key",
                vec![route(
                    "deepseek-v4.1-flash",
                    "OpenCode Go",
                    "openai-compatible:opencode-go",
                )],
            ),
            cx,
        );
    });
    vcx.run_until_parked();
    // OpenCode Go -> Claude: the way back must not be locked.
    open_picker(vcx);
    set_statuses(&panel, vcx);
    search(&panel, vcx, "");
    assert!(vcx.debug_bounds("picker-active-opencode-go").is_some());
    search(&panel, vcx, "claude");
    assert!(vcx.debug_bounds("picker-available-claude").is_some());
    let claude = vcx.debug_bounds("login-provider-claude").unwrap();
    vcx.simulate_click(claude.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let sent = commands.recv_timeout(std::time::Duration::from_millis(100));
    assert!(
        matches!(&sent, Ok(Command::SetModel { model, .. }) if model.starts_with("claude-oauth:")),
        "the way back to Claude must switch"
    );
}
