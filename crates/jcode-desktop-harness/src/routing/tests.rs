use super::*;
use RoutingGateChoice as C;
use RoutingGateOption as O;

fn chip(warnings: &[&str], pending: Option<&str>) -> RoutingChip {
    RoutingChip::new(
        "claude:team".into(),
        "priority 1".into(),
        warnings.iter().map(|w| w.to_string()).collect(),
        pending.map(str::to_string),
    )
    .unwrap()
}

fn icon(account: &str) -> String {
    jcode_session_types::account_icon::account_icon(account, None)
}

#[test]
fn chip_parts_lead_with_the_icon_then_add_the_warning_and_the_pending_move() {
    let team = icon("claude:team");
    let spare = icon("claude:spare");
    let parts = |chip: RoutingChip| {
        (
            chip.account_text(),
            chip.warning_marker(),
            chip.pending_text(),
        )
    };
    assert_eq!(
        parts(chip(&[], None)),
        (format!("{team} claude:team"), None, None)
    );
    assert_eq!(
        parts(chip(&["budget hf 90 of 100"], None)).1.as_deref(),
        Some("⚠")
    );
    assert_eq!(parts(chip(&["a", "b", "c"], None)).1.as_deref(), Some("⚠3"));
    assert_eq!(
        parts(chip(&["a"], Some("claude:spare"))).2,
        Some(format!("{spare} claude:spare"))
    );
}

#[test]
fn a_configured_icon_replaces_the_default_in_the_account_and_the_pending_move() {
    let chip = chip(&[], Some("claude:spare"))
        .with_icons(|account| (account == "claude:spare").then(|| "🚀".to_owned()));
    assert_eq!(
        chip.account_text(),
        format!("{} claude:team", icon("claude:team"))
    );
    assert_eq!(chip.pending_text().as_deref(), Some("🚀 claude:spare"));
}

#[test]
fn chip_needs_an_account_and_strips_control_characters() {
    assert_eq!(
        RoutingChip::new("  ".into(), "r".into(), vec![], None),
        None
    );
    let chip = RoutingChip::new(
        "a\nb".into(),
        "why\t\u{7}".into(),
        vec!["\n".into()],
        Some("".into()),
    )
    .unwrap();
    assert_eq!(chip.account, "a b");
    assert_eq!(chip.reason, "why");
    assert!(chip.warnings.is_empty(), "blank warnings are dropped");
    assert_eq!(chip.pending, None);
}

#[test]
fn tooltip_explains_the_account_the_move_and_each_warning() {
    let lines = chip(&["budget hf-month 45 of 50"], Some("claude:spare")).tooltip();
    assert_eq!(
        lines[0],
        format!("Running on {} claude:team", icon("claude:team"))
    );
    assert_eq!(lines[1], "Why: priority 1");
    assert_eq!(
        lines[2],
        format!(
            "Moves to {} claude:spare at the next cache-cold turn",
            icon("claude:spare")
        )
    );
    assert_eq!(lines[3], "⚠ budget hf-month 45 of 50");
}

fn card() -> GateCard {
    GateCard::new(
        "g1".into(),
        "claude:team reached its 5h limit".into(),
        vec![
            O::WaitUntil {
                resets_at: 1_900_000_000,
            },
            O::Switch {
                account: "claude:spare".into(),
                detail: "40% used".into(),
            },
            O::Ack {
                account: "claude:team".into(),
            },
        ],
    )
}

#[test]
fn keys_answer_like_the_terminal_and_click_answers_its_row() {
    let mut card = card();
    assert_eq!(card.view().cursor, 1, "focus starts on the first switch");
    assert_eq!(
        card.press(GateKey::Enter),
        Some(C::Switch {
            account: "claude:spare".into()
        })
    );
    assert_eq!(card.press(GateKey::Digit(1)), Some(C::Wait));
    assert_eq!(card.press(GateKey::Esc), Some(C::Cancel));
    assert_eq!(card.press(GateKey::Down), None);
    assert_eq!(card.view().cursor, 2);
    assert_eq!(card.choose_row(3), Some(C::Cancel));
    assert_eq!(card.view().cursor, 3);
    assert_eq!(card.choose_row(4), None, "no such row");
}

#[test]
fn nothing_is_answerable_while_an_answer_is_in_flight_and_a_failure_reopens_it() {
    let mut card = card();
    card.mark_submitting(7);
    assert!(card.view().submitting);
    assert_eq!(card.press(GateKey::Enter), None);
    assert_eq!(card.choose_row(0), None);
    card.answer_failed("No routing gate g1 is waiting for this session".into());
    assert!(!card.view().submitting);
    assert_eq!(
        card.error.as_deref(),
        Some("No routing gate g1 is waiting for this session")
    );
    assert!(card.press(GateKey::Enter).is_some());
    card.mark_submitting(8);
    assert_eq!(card.error, None, "a new attempt clears the old error");
}

fn view(mode: &str) -> RoutingView {
    RoutingView {
        mode: mode.into(),
        ..RoutingView::default()
    }
}

#[test]
fn a_failed_fetch_keeps_the_last_good_view_and_reports_nothing() {
    let mut publisher = Publisher::default();
    assert_eq!(publisher.observe(Ok(view("auto"))), Some(view("auto")));
    assert_eq!(publisher.observe(Err(FetchError::Unavailable)), None);
    assert_eq!(publisher.observe(Err(FetchError::Unsupported)), None);
    assert_eq!(
        publisher.observe(Ok(view("auto"))),
        None,
        "an unchanged view is not republished"
    );
    assert_eq!(publisher.observe(Ok(view("shadow"))), Some(view("shadow")));
}

#[test]
fn a_runtime_without_routing_is_asked_rarely() {
    assert_eq!(Publisher::next_poll(&Ok(view("off"))), POLL);
    assert_eq!(Publisher::next_poll(&Err(FetchError::Unavailable)), POLL);
    assert_eq!(
        Publisher::next_poll(&Err(FetchError::Unsupported)),
        UNSUPPORTED_POLL
    );
    assert!(UNSUPPORTED_POLL > POLL);
}

#[test]
fn the_screenshot_fixture_covers_keys_budgets_and_an_unknown_meter() {
    let fixture = fixture();
    assert!(
        fixture.api_keys[0]
            .keys
            .iter()
            .any(|key| key.active && key.label.is_some())
    );
    assert!(fixture.budgets.iter().any(|budget| budget.used.is_none()));
    assert!(fixture.budgets.iter().any(|budget| budget.unit == "usd"));
}

#[test]
fn a_configured_icon_wins_and_the_default_follows_the_label() {
    let mut view = fixture();
    view.accounts[0].icon = Some("🚀".into());
    assert_eq!(icon_for(Some(&view), "claude:team"), "🚀");
    assert_eq!(icon_for(Some(&view), "claude:claude-fox"), "🦊");
    assert_eq!(
        icon_for(None, "claude:team"),
        icon_for(Some(&fixture()), "claude:team")
    );
}
