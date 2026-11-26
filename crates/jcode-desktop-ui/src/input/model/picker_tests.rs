//! Regression coverage for cached, viewport-only model suggestions.
use super::*;

struct Fixture(Entity<PromptInput>);

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .pt(px(500.))
            .px(px(20.))
            .child(self.0.clone())
    }
}

fn routes(count: usize) -> Vec<jcode_sdk::ModelRouteInfo> {
    (0..count)
        .map(|index| jcode_sdk::ModelRouteInfo {
            account: None,
            icon: None,
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: format!("atlas-{index:04}"),
            provider: "OpenAI".into(),
            api_method: "openai-api-key".into(),
            available: true,
            detail: String::new(),
            usage: None,
        })
        .collect()
}

#[gpui::test]
fn model_suggestions_cache_invalidates_for_query_routes_current_and_groups(
    cx: &mut gpui::TestAppContext,
) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        let mut routes = routes(8);
        input.set_model_routes(Vec::new(), &routes, None, cx);
        input.set_content("/model ".into(), cx);
        let collapsed = input.command_suggestions();
        assert_eq!(collapsed.len(), 5); // Three models, disclosure, Manage.
        assert!(Arc::ptr_eq(&collapsed, &input.command_suggestions()));

        // Changing only highlight must not rebuild the metadata vector.
        input.command_selection = 1;
        assert!(Arc::ptr_eq(&collapsed, &input.command_suggestions()));

        input.set_content("/model atlas-0007".into(), cx);
        let filtered = input.command_suggestions();
        assert!(!Arc::ptr_eq(&collapsed, &filtered));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].value, "/model openai-api:atlas-0007");

        routes[7].available = false;
        input.set_model_routes(Vec::new(), &routes, None, cx);
        let unavailable = input.command_suggestions();
        assert!(!Arc::ptr_eq(&filtered, &unavailable));
        assert!(unavailable.is_empty());

        input.set_content("/model ".into(), cx);
        let before_current = input.command_suggestions();
        input.set_current_model(Some("atlas-0006".into()), cx);
        let current = input.command_suggestions();
        assert!(!Arc::ptr_eq(&before_current, &current));
        assert_eq!(current[0].value, "/model openai-api:atlas-0006");
        assert!(current[0].active_choice);
        assert!(current[0].help.is_empty());

        let group = current.iter().find_map(|row| row.toggle.clone()).unwrap();
        input.toggle_model_group(group.clone(), cx);
        let expanded = input.command_suggestions();
        assert!(!Arc::ptr_eq(&current, &expanded));
        assert_eq!(expanded.len(), 9); // Seven models, disclosure and Manage.
        assert_eq!(expanded[expanded.len() - 2].value, "Show fewer models");
        input.toggle_model_group(group, cx);
        let recollapsed = input.command_suggestions();
        assert!(!Arc::ptr_eq(&expanded, &recollapsed));
        assert_eq!(recollapsed.as_ref(), current.as_ref());
    });
}

#[gpui::test]
fn unchanged_runtime_routes_skip_rebuilding_the_catalog(cx: &mut gpui::TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        let mut routes = routes(8);
        input.set_model_routes(Vec::new(), &routes, None, cx);
        input.set_content("/model ".into(), cx);
        let first = input.command_suggestions();
        let revision = input.suggestions_revision;

        // A usage-tick rebroadcast with an identical catalog is a no-op.
        input.set_model_routes(Vec::new(), &routes, None, cx);
        assert_eq!(input.suggestions_revision, revision);
        assert!(Arc::ptr_eq(&first, &input.command_suggestions()));

        // The current model still applies without rebuilding routes.
        input.set_model_routes(Vec::new(), &routes, Some("atlas-0006".into()), cx);
        assert!(input.command_suggestions()[0].active_choice);

        // A real catalog change is still applied.
        routes[7].available = false;
        input.set_model_routes(Vec::new(), &routes, Some("atlas-0006".into()), cx);
        assert!(
            !input
                .command_models
                .iter()
                .any(|m| m.ends_with("atlas-0007"))
        );

        // Offline fallback models replace the catalog, so the next identical
        // RuntimeInfo must restore routes rather than hit a stale cache.
        input.set_command_models(vec!["fallback".into()], cx);
        input.set_model_routes(Vec::new(), &routes, Some("atlas-0006".into()), cx);
        assert_eq!(input.command_models.len(), 7);
    });
}

#[gpui::test]
fn effort_choice_remains_active_as_the_keyboard_highlight_moves(cx: &mut gpui::TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        input.set_effort_state(vec!["low", "medium", "high"], Some("medium".into()), cx);
        input.start_picker_search("/effort", Vec::new(), cx);
        let rows = input.command_suggestions();
        assert_eq!(input.command_selection, 1);
        assert_eq!(rows[1].value, "/effort medium");
        assert!(rows[1].active_choice);
        assert_eq!(rows[1].help, "Set reasoning effort");

        input.command_selection = 2;
        let rows = input.command_suggestions();
        assert!(!rows[2].active_choice);
        assert!(rows[1].active_choice);
    });
}

#[gpui::test]
fn model_suggestions_cache_survives_real_hover_selection(cx: &mut gpui::TestAppContext) {
    let (fixture, vcx) = cx.add_window_view(|_, cx| {
        Fixture(cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {})))
    });
    let input = fixture.read_with(vcx, |fixture, _| fixture.0.clone());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, size(px(1000.), px(800.)));
    input.update(vcx, |input, cx| {
        input.set_model_routes(Vec::new(), &routes(40), None, cx);
        input.set_content("/model atlas".into(), cx);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.run_until_parked();
    for (selector, selected) in [("slash-command-row-1", 1), ("slash-command-row-0", 0)] {
        let position = vcx.debug_bounds(selector).unwrap().center();
        assert!(
            vcx.debug_bounds("slash-command-scroll")
                .unwrap()
                .contains(&position)
        );
        // Compare in one synchronous update to avoid dependence on a frame's
        // elapsed time crossing the cache's relative-label minute boundary.
        vcx.update(|window, cx| {
            let before = input.read(cx).command_suggestions();
            window.simulate_mouse_move(position, cx);
            assert_eq!(input.read(cx).command_selection, selected);
            assert!(Arc::ptr_eq(&before, &input.read(cx).command_suggestions()));
            window.draw(cx).clear(cx);
            assert!(Arc::ptr_eq(&before, &input.read(cx).command_suggestions()));
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds(selector)
                .unwrap()
                .contains(&vcx.debug_bounds("picker-keyboard-focus").unwrap().center())
        );
    }
}

#[gpui::test]
fn model_picker_virtualizes_thousand_routes_and_clicks_exact_last_route(
    cx: &mut gpui::TestAppContext,
) {
    let submitted = std::rc::Rc::new(RefCell::new(Vec::<String>::new()));
    let sink = submitted.clone();
    let (fixture, vcx) = cx.add_window_view(|_, cx| {
        Fixture(cx.new(|cx| {
            PromptInput::new(cx, "test", move |text, _, _, _| {
                sink.borrow_mut().push(text)
            })
        }))
    });
    let input = fixture.read_with(vcx, |fixture, _| fixture.0.clone());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, size(px(1000.), px(800.)));
    let mut catalog = routes(1000);
    // Newest first puts atlas-0000 last. The current longer name matches the
    // final short name's query and is ranked first. Clicking the short name
    // must not submit that first match.
    catalog[1].model = "atlas-0000-long".into();
    input.update(vcx, |input, cx| {
        input.set_model_routes(Vec::new(), &catalog, Some("atlas-0000-long".into()), cx);
        input.set_content("/model atlas".into(), cx);
        let rows = input.command_suggestions();
        assert_eq!(rows.len(), 1000);
        assert_eq!(rows[0].value, "/model openai-api:atlas-0000-long");
        assert_eq!(rows[999].value, "/model openai-api:atlas-0000");
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.run_until_parked();
    // debug_bounds accepts static selectors. These test-only names are leaked
    // once for the process lifetime, never from production rendering.
    let selectors: Vec<&'static str> = (0..1000)
        .map(|index| {
            Box::leak(format!("slash-command-row-{index}").into_boxed_str()) as &'static str
        })
        .collect();
    let visible = selectors
        .iter()
        .filter(|selector| vcx.debug_bounds(selector).is_some())
        .count();
    assert!((2..50).contains(&visible), "painted {visible} of 1000 rows");
    assert!(vcx.debug_bounds("slash-command-row-999").is_none());

    input.update_in(vcx, |input, window, cx| {
        input.command_selection = 998;
        input.history_next(&HistoryNext, window, cx);
        assert_eq!(input.command_selection, 999);
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.run_until_parked();
    let last = vcx
        .debug_bounds("slash-command-row-999")
        .expect("last route is reachable");
    assert!(
        vcx.debug_bounds("slash-command-scroll")
            .unwrap()
            .contains(&last.center())
    );
    assert!(vcx.debug_bounds("slash-command-row-0").is_none());
    let visible = selectors
        .iter()
        .filter(|selector| vcx.debug_bounds(selector).is_some())
        .count();
    assert!((1..50).contains(&visible), "painted {visible} rows at end");
    vcx.simulate_click(last.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    // A click switches to that exact route at once.
    assert_eq!(
        submitted.borrow().as_slice(),
        ["/model openai-api:atlas-0000"]
    );
}

/// The picker's effort step has no hover, so each row keeps the level name.
/// The level's shape is drawn beside it, so the label holds no text glyph.
#[test]
fn effort_step_rows_keep_the_name_and_no_text_glyph() {
    use super::model_picker::effort_row_label;
    assert_eq!(effort_row_label("none"), "None");
    assert_eq!(effort_row_label("low"), "Low");
    assert_eq!(effort_row_label("medium"), "Medium");
    assert_eq!(effort_row_label("high"), "High");
    assert_eq!(effort_row_label("xhigh"), "xHigh");
    assert_eq!(effort_row_label("max"), "Max");
    assert_eq!(effort_row_label("swarm"), "Swarm");
}

/// Provider headers show the credential's quota bars and resets, not how it
/// signs in, and the highlighted model row carries its effort ladder.
#[gpui::test]
fn headers_show_quota_and_the_highlighted_row_offers_effort(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        cx.set_global(crate::panel::usage::StatusAccounts(vec![
            crate::accounts::Account {
                id: "openai".into(),
                display_name: "OpenAI".into(),
                status: "available".into(),
                auth_kind: "OAuth".into(),
                method: String::new(),
                limits: Vec::new(),
                usage_reports: vec![crate::accounts::UsageReport {
                    identity: None,
                    credits: None,
                    provider_name: "OpenAI (ChatGPT) (a@x)".into(),
                    account_label: None,
                    limits: vec![
                        crate::accounts::UsageLimit {
                            name: "5-hour window".into(),
                            usage_percent: 30.,
                            reset_in: Some("2h".into()),
                        },
                        crate::accounts::UsageLimit {
                            name: "7-day window".into(),
                            usage_percent: 100.,
                            reset_in: Some("2d 20h".into()),
                        },
                    ],
                    extra_info: Vec::new(),
                    banked_reset: None,
                }],
            },
        ]))
    });
    let (fixture, vcx) = cx.add_window_view(|_, cx| {
        Fixture(cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {})))
    });
    let input = fixture.read_with(vcx, |fixture, _| fixture.0.clone());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, size(px(1000.), px(800.)));
    input.update_in(vcx, |input, window, cx| {
        window.focus(&input.focus_handle, cx);
        let route = jcode_sdk::ModelRouteInfo {
            account: None,
            icon: None,
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: "gpt-5.6-sol".into(),
            provider: "OpenAI".into(),
            api_method: "openai-oauth".into(),
            available: true,
            detail: String::new(),
            usage: None,
        };
        input.set_model_routes(Vec::new(), &[route], None, cx);
        input.set_effort_state(
            crate::effort::ladder(None, Some("gpt-5.6-sol")),
            Some("high".into()),
            cx,
        );
        input.set_content("/model ".into(), cx);
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("model-picker-quota-0").is_some(),
        "quota beside the header"
    );
    assert!(vcx.debug_bounds("model-picker-quota-0-0").is_some());
    assert!(vcx.debug_bounds("model-picker-quota-0-1").is_some());
    assert!(
        vcx.debug_bounds("model-picker-auth-0").is_none(),
        "no Signed in badge"
    );
    assert!(vcx.debug_bounds("model-row-efforts-0").is_some());
    let medium = vcx
        .debug_bounds("model-row-effort-0-medium")
        .expect("medium mark on the highlighted row");
    vcx.simulate_click(medium.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    input.read_with(vcx, |input, _| {
        assert_eq!(input.row_effort("openai-oauth:gpt-5.6-sol"), Some("medium"));
    });
}

fn account_routes() -> Vec<jcode_sdk::ModelRouteInfo> {
    ["claude-otter", "claude-fox", "claude-panda"]
        .into_iter()
        .map(|label| jcode_sdk::ModelRouteInfo {
            account: Some(format!("claude:{label}")),
            icon: Some(
                jcode_session_types::account_icon::account_icon(label, None)
                    .replace("🦊", "🚀"),
            ),
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: "claude-opus-4-8".into(),
            provider: "Anthropic".into(),
            api_method: format!("claude-oauth@{label}"),
            available: true,
            detail: String::new(),
            usage: None,
        })
        .collect()
}

#[gpui::test]
fn the_same_model_lists_one_route_per_account_and_marks_the_serving_one(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, vcx) = cx.add_window_view(|_, cx| {
        Fixture(cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {})))
    });
    let input = fixture.read_with(vcx, |fixture, _| fixture.0.clone());
    let handle = vcx.update(|window, _| window.window_handle());
    vcx.simulate_window_resize(handle, size(px(1000.), px(900.)));
    input.update(vcx, |input, cx| {
        input.set_model_routes(
            Vec::new(),
            &account_routes(),
            Some("claude-opus-4-8".into()),
            cx,
        );
        input.set_current_account(Some("claude:claude-fox".into()), cx);
        input.set_content("/model ".into(), cx);
        let rows = input.command_suggestions();
        let models: Vec<_> = rows.iter().filter(|row| row.value.starts_with("/model ")).collect();
        assert_eq!(models.len(), 3, "one selectable route per account");
        let specs: HashSet<_> = models.iter().map(|row| row.value.clone()).collect();
        assert_eq!(specs.len(), 3, "each account has its own spec");
        let active: Vec<_> = models.iter().filter(|row| row.active_choice).collect();
        assert_eq!(active.len(), 1);
        assert!(
            active[0].value.contains("claude-fox"),
            "the account the session runs on is the marked one: {}",
            active[0].value
        );
    });
    vcx.run_until_parked();
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.run_until_parked();
    let icons: Vec<_> = (0..12)
        .filter_map(|index| {
            let selector =
                Box::leak(format!("model-picker-account-icon-{index}").into_boxed_str()) as &'static str;
            vcx.debug_bounds(selector)
        })
        .collect();
    assert_eq!(icons.len(), 3, "every account group leads with its icon");
    let active_marks = (0..12)
        .filter(|index| {
            let selector =
                Box::leak(format!("model-picker-account-active-{index}").into_boxed_str()) as &'static str;
            vcx.debug_bounds(selector).is_some()
        })
        .count();
    assert_eq!(active_marks, 1, "only the serving account's group says active");
    for index in 0..12 {
        let label = Box::leak(format!("model-picker-group-label-{index}").into_boxed_str()) as &'static str;
        let icon = Box::leak(format!("model-picker-account-icon-{index}").into_boxed_str()) as &'static str;
        if let (Some(label), Some(icon)) = (vcx.debug_bounds(label), vcx.debug_bounds(icon)) {
            assert!(icon.right() <= label.left(), "icon first: {icon:?} {label:?}");
            assert_eq!(icon.top() < label.bottom() && label.top() < icon.bottom(), true, "one line");
        }
    }
}
