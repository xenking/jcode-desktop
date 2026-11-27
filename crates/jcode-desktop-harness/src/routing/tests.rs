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

/// The icons the daemon resolves when `claude` has several accounts.
fn with_default_icons(chip: RoutingChip) -> RoutingChip {
    chip.with_icons(|account| Some(icon(account)))
}

#[test]
fn chip_parts_lead_with_the_icon_then_add_the_warning_and_the_pending_move() {
    let team = icon("claude:team");
    let spare = icon("claude:spare");
    let parts = |chip: RoutingChip| {
        let chip = with_default_icons(chip);
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
fn a_lone_account_names_itself_without_an_icon() {
    let chip = chip(&["a"], Some("claude:spare"));
    assert_eq!(chip.account_text(), "claude:team");
    assert_eq!(chip.pending_text().as_deref(), Some("claude:spare"));
    assert_eq!(chip.tooltip()[0], "Running on claude:team");
}

#[test]
fn only_the_accounts_the_lookup_names_get_an_icon() {
    let chip = chip(&[], Some("claude:spare"))
        .with_icons(|account| (account == "claude:spare").then(|| "🚀".to_owned()));
    assert_eq!(chip.account_text(), "claude:team");
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
    let lines =
        with_default_icons(chip(&["budget hf-month 45 of 50"], Some("claude:spare"))).tooltip();
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
fn an_icon_exists_only_where_it_tells_accounts_of_a_provider_apart() {
    let mut view = fixture();
    let team = view.accounts[0].clone();
    // One account of the provider: no icon, whatever the label.
    view.accounts = vec![team.clone()];
    assert_eq!(icon_for(Some(&view), "claude:team"), None);
    assert_eq!(
        icon_for(None, "claude:team"),
        None,
        "nothing known, nothing shown"
    );

    // A second Claude account: both are told apart by the daemon's icons.
    let mut fox = team.clone();
    fox.id = "claude:claude-fox".into();
    fox.icon = Some(icon("claude:claude-fox"));
    view.accounts[0].icon = Some("🚀".into());
    view.accounts.push(fox);
    assert_eq!(icon_for(Some(&view), "claude:team").as_deref(), Some("🚀"));
    assert_eq!(
        icon_for(Some(&view), "claude:claude-fox").as_deref(),
        Some("🦊")
    );
    // An account the view does not route still gets the default beside siblings.
    assert_eq!(
        icon_for(Some(&view), "claude:other"),
        Some(icon("claude:other"))
    );
    // Another provider's lone account stays bare.
    assert_eq!(icon_for(Some(&view), "openai:work"), None);
}
