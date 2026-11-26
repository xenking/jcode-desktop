use super::feed::fixture;
use super::*;

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
