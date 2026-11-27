use super::*;
use jcode_session_types::routing::{RoutingApiKeyView, RoutingKeyView};

const NOW: i64 = 1_000_000;

fn window(
    window: &str,
    unit: &str,
    used: f64,
    limit: Option<f64>,
    resets_in: Option<i64>,
) -> RoutingWindowView {
    RoutingWindowView {
        window: window.into(),
        unit: unit.into(),
        used,
        limit,
        resets_at: resets_in.map(|secs| NOW + secs),
        model_class: None,
    }
}

fn key(
    account: &str,
    label: Option<&str>,
    active: bool,
    windows: Vec<RoutingWindowView>,
) -> RoutingKeyView {
    RoutingKeyView {
        account: account.into(),
        label: label.map(str::to_string),
        source: "stored".into(),
        active,
        routed: false,
        windows,
    }
}

fn budget(unit: &str, limit: f64, warn: Option<f64>, used: Option<f64>) -> RoutingBudgetView {
    RoutingBudgetView {
        name: "b".into(),
        scope_kind: "provider".into(),
        scope: "huggingface".into(),
        meter: "ledger".into(),
        window: "calendar-month".into(),
        unit: unit.into(),
        warn,
        limit,
        action: "ask".into(),
        used,
    }
}

#[test]
fn key_rows_show_label_active_marker_and_every_window_in_order() {
    let view = RoutingView {
        api_keys: vec![RoutingApiKeyView {
            provider: "opencode-go".into(),
            keys: vec![
                key(
                    "opencode-go",
                    None,
                    false,
                    vec![window("5h", "percent", 12.0, None, Some(7_380))],
                ),
                key(
                    "opencode-go:second",
                    Some("second"),
                    true,
                    vec![
                        window("5h", "percent", 74.0, None, Some(3_600)),
                        window("1w", "percent", 93.0, None, Some(266_400)),
                    ],
                ),
                key("opencode-go:idle", Some("idle"), false, Vec::new()),
            ],
        }],
        ..RoutingView::default()
    };
    let rows = SidebarRouting::from_view(&view, NOW);
    let titles: Vec<_> = rows.keys.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(
        titles,
        ["opencode-go", "opencode-go · second", "opencode-go · idle"]
    );
    assert!(rows.keys[1].active && !rows.keys[0].active);
    assert!(
        rows.keys.iter().all(|row| row.icon.is_some()),
        "three keys of one provider each lead with an icon"
    );
    assert_eq!(
        rows.keys[1].icon,
        crate::routing::icon_for(Some(&view), "opencode-go:second")
    );
    assert_eq!(rows.keys[0].windows[0].label, "5h · 12% · 2h 3m");
    assert_eq!(rows.keys[1].windows[0].label, "5h · 74% · 1h 0m");
    assert_eq!(
        rows.keys[1].windows[1].label, "1w · 93% · 3d 2h",
        "a week is 1w"
    );
    assert!(
        rows.keys[2].windows.is_empty(),
        "a labelled key with no reading still lists"
    );
}

#[test]
fn an_unlabelled_key_without_windows_is_not_listed() {
    let view = RoutingView {
        api_keys: vec![RoutingApiKeyView {
            provider: "deepseek".into(),
            keys: vec![key("deepseek", None, true, Vec::new())],
        }],
        ..RoutingView::default()
    };
    assert!(SidebarRouting::from_view(&view, NOW).is_empty());
}

#[test]
fn window_bars_use_the_account_row_colours() {
    let bar = |used| window_row(&window("5h", "percent", used, None, None), NOW);
    assert_eq!(
        (
            bar(69.0).tone,
            bar(70.0).tone,
            bar(89.0).tone,
            bar(90.0).tone
        ),
        (Tone::Accent, Tone::Warn, Tone::Warn, Tone::Error)
    );
    assert_eq!(bar(140.0).fraction, Some(1.0), "over 100 clamps");
    assert_eq!(
        bar(50.0).label,
        "5h · 50%",
        "no reset text without a reset time"
    );
}

#[test]
fn a_model_scoped_window_names_its_class_and_money_windows_show_amounts() {
    let mut scoped = window("1w", "percent", 31.0, None, None);
    scoped.model_class = Some("fable".into());
    assert_eq!(window_row(&scoped, NOW).label, "fable 1w · 31%");
    let spend = window_row(&window("month", "usd", 45.0, Some(50.0), None), NOW);
    assert_eq!(spend.label, "month · $45.00 / $50.00");
    assert_eq!(spend.fraction, Some(0.9));
    assert_eq!(spend.tone, Tone::Error);
    let uncapped = window_row(&window("month", "usd", 45.0, None, None), NOW);
    assert_eq!(uncapped.fraction, None, "no cap, no bar");
}

#[test]
fn a_reset_in_the_past_is_not_shown() {
    assert_eq!(reset_in(NOW - 1, NOW), None);
    assert_eq!(reset_in(NOW + 30, NOW).as_deref(), Some("1m"));
}

#[test]
fn budgets_show_used_over_limit_with_the_warn_and_limit_colours() {
    let row = |unit, limit, warn, used| budget_row(&budget(unit, limit, warn, used));
    let ok = row("usd", 100.0, Some(80.0), Some(40.0));
    assert_eq!(
        (ok.summary.as_str(), ok.bar),
        ("$40.00 / $100.00", Some((0.4, Tone::Accent)))
    );
    assert_eq!(
        row("usd", 100.0, Some(80.0), Some(82.0)).bar,
        Some((0.82, Tone::Warn))
    );
    assert_eq!(
        row("usd", 100.0, Some(80.0), Some(100.0)).bar,
        Some((1.0, Tone::Error))
    );
    assert_eq!(
        row("usd", 100.0, Some(80.0), Some(130.0)).bar,
        Some((1.0, Tone::Error)),
        "over the limit clamps the bar"
    );
    assert_eq!(
        row("percent", 90.0, None, Some(70.0))
            .bar
            .map(|(_, tone)| tone),
        Some(Tone::Warn),
        "no warn level falls back to 70 percent of the limit"
    );
    assert_eq!(
        row("percent", 90.0, None, Some(20.0))
            .bar
            .map(|(_, tone)| tone),
        Some(Tone::Accent)
    );
}

#[test]
fn an_unreadable_budget_says_unknown_and_draws_no_bar() {
    let unknown = budget_row(&budget("usd", 100.0, Some(80.0), None));
    assert_eq!(unknown.summary, "unknown / $100.00");
    assert_eq!(unknown.bar, None);
    assert_eq!(unknown.scope, "huggingface · month");
    let zero = budget_row(&budget("usd", 100.0, None, Some(0.0)));
    assert_eq!(
        zero.summary, "$0.00 / $100.00",
        "a real zero is not unknown"
    );
    assert_eq!(zero.bar, Some((0.0, Tone::Accent)));
}

#[test]
fn amounts_format_by_unit() {
    assert_eq!(amount("percent", 41.4), "41%");
    assert_eq!(amount("credits", 12.0), "12 credits");
    assert_eq!(amount("tokens", 1_250_000.0), "1.2M tokens");
    assert_eq!(amount("tokens", 4_500.0), "4k tokens");
    assert_eq!(amount("tokens", 900.0), "900 tokens");
}
