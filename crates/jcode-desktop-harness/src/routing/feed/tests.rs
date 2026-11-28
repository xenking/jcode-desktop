use super::*;

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
