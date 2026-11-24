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

#[test]
fn chip_label_is_the_account_alone_until_something_is_pending_or_warned() {
    assert_eq!(chip(&[], None).label(), "claude:team");
    assert_eq!(
        chip(&["budget hf 90 of 100"], None).label(),
        "claude:team ⚠"
    );
    assert_eq!(chip(&["a", "b", "c"], None).label(), "claude:team ⚠3");
    assert_eq!(
        chip(&["a"], Some("claude:spare")).label(),
        "claude:team ⚠ → claude:spare"
    );
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
    assert_eq!(lines[0], "Running on claude:team");
    assert_eq!(lines[1], "Why: priority 1");
    assert_eq!(
        lines[2],
        "Moves to claude:spare at the next cache-cold turn"
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
    assert_eq!(card.model.cursor, 1, "focus starts on the first switch");
    assert_eq!(
        card.press(GateKey::Enter),
        Some(C::Switch {
            account: "claude:spare".into()
        })
    );
    assert_eq!(card.press(GateKey::Digit(1)), Some(C::Wait));
    assert_eq!(card.press(GateKey::Esc), Some(C::Cancel));
    assert_eq!(card.press(GateKey::Down), None);
    assert_eq!(card.model.cursor, 2);
    assert_eq!(card.choose_row(3), Some(C::Cancel));
    assert_eq!(card.model.cursor, 3);
    assert_eq!(card.choose_row(4), None, "no such row");
}

#[test]
fn nothing_is_answerable_while_an_answer_is_in_flight_and_a_failure_reopens_it() {
    let mut card = card();
    card.mark_submitting(7);
    assert!(card.submitting());
    assert_eq!(card.press(GateKey::Enter), None);
    assert_eq!(card.choose_row(0), None);
    card.answer_failed("No routing gate g1 is waiting for this session".into());
    assert!(!card.submitting());
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
