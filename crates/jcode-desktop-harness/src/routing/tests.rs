use super::feed::fixture;
use super::*;

fn chip(warnings: &[&str]) -> RoutingChip {
    RoutingChip::new(
        "claude:team".into(),
        "claude:team ranks first".into(),
        warnings.iter().map(|w| w.to_string()).collect(),
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
fn the_chip_names_the_account_by_its_name_with_the_icon_first() {
    assert_eq!(chip(&[]).account_text(), "team");
    let iconed = chip(&[]).with_icons(|account| Some(icon(account)));
    assert_eq!(
        iconed.account_text(),
        format!("{} team", icon("claude:team"))
    );
}

#[test]
fn the_chip_is_an_alert_only_after_a_move_or_with_a_warning() {
    assert!(!chip(&[]).is_alert());
    assert!(chip(&["budget hf 90 of 100"]).is_alert());
    let mut moved = chip(&[]);
    moved.moved_from = Some("claude:other".into());
    assert!(moved.is_alert());
    assert_eq!(
        chip(&["budget hf 90 of 100"]).warning_marker().as_deref(),
        Some("⚠")
    );
    assert_eq!(
        chip(&["a", "b", "c"]).warning_marker().as_deref(),
        Some("⚠3")
    );
}

#[test]
fn chip_needs_an_account_and_strips_control_characters() {
    assert_eq!(RoutingChip::new("  ".into(), "r".into(), vec![]), None);
    let chip = RoutingChip::new("a\nb".into(), "why\t\u{7}".into(), vec!["\n".into()]).unwrap();
    assert_eq!(chip.account, "a b");
    assert_eq!(chip.reason, "why");
    assert!(chip.warnings.is_empty(), "blank warnings are dropped");
}

#[test]
fn ids_in_a_reason_are_said_as_names() {
    let mut moved = chip(&[]);
    moved.moved_from = Some("claude:claude-otter".into());
    moved.reason =
        "claude:claude-otter cannot serve (claude:claude-otter 5h 99%), moving to claude:team"
            .into();
    assert_eq!(
        moved.lines()[1],
        "Claude-otter cannot serve (claude-otter 5h 99%), moving to team"
    );
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
