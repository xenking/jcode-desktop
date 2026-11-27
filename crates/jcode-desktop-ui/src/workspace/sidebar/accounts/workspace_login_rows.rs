//! One block per saved login in the accounts sidebar.
//!
//! A provider with several logins (three Claude accounts, two ChatGPT seats)
//! used to show one row with the first login's limits. Each login now gets its
//! own block: its name, how it is billed, its own windows, its own banked
//! resets and its own credits. Nothing is summed or borrowed across logins.
use super::*;
use crate::accounts::{Account, Credits, LoginKind, UsageReport};

/// `5-hour window` reads `5h`, `7-day Opus window` reads `7d Opus`.
fn short_limit_name(name: &str) -> String {
    name.trim_end_matches(" window")
        .replace("5-hour", "5h")
        .replace("7-day", "7d")
}

/// Whether the account row lists its logins one by one. Providers without a
/// saved-login identity (API keys, device sign-ins) keep the compact row.
pub(super) fn lists_logins(account: &Account) -> bool {
    account
        .usage_reports
        .iter()
        .any(|report| report.identity.is_some())
}

fn login_name(report: &UsageReport) -> String {
    match &report.identity {
        Some(identity) => identity.display_name.clone(),
        None => report.title(),
    }
}

/// One caption line: a fixed height, never wrapped, and cut with an ellipsis
/// instead of mid-word when the sidebar is narrow.
fn caption(text: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .h(px(12.0))
        .line_height(px(12.0))
        .text_size(px(8.0))
        .text_color(Theme::global().TEXT_DIM)
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(text.into())
}

fn kind_color(kind: LoginKind) -> gpui::Rgba {
    match kind {
        LoginKind::Subscription => Theme::global().TEXT_DIM,
        LoginKind::Credits => Theme::global().ACCENT,
        LoginKind::Metered => Theme::global().WARN,
    }
}

fn limit_cell(
    account_id: &str,
    login: usize,
    index: usize,
    limit: &crate::accounts::UsageLimit,
) -> gpui::Div {
    let used = limit.usage_percent.clamp(0.0, 100.0);
    let reading = crate::account_status::configured_usage_reading(used);
    let label = match &limit.reset_in {
        Some(reset) => format!("{} · {reading} · {reset}", short_limit_name(&limit.name)),
        None => format!("{} · {reading}", short_limit_name(&limit.name)),
    };
    let id = account_id.to_owned();
    div()
        .debug_selector(move || format!("account-{id}-login-{login}-limit-{index}"))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(caption(label))
        .child(usage_bar(
            used / 100.0,
            crate::account_status::usage_color(used, Theme::global()),
        ))
}

fn credits_line(account_id: &str, login: usize, credits: &Credits) -> gpui::Div {
    let id = account_id.to_owned();
    let text = if credits.known {
        format!("credits · {}", credits.summary)
    } else {
        "credits · unknown".to_owned()
    };
    let mut line = div()
        .debug_selector(move || format!("account-{id}-login-{login}-credits"))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(caption(text));
    if let Some(fraction) = credits.used_fraction {
        line = line.child(usage_bar(
            fraction,
            if fraction >= 0.9 {
                Theme::global().ERROR
            } else if fraction >= 0.7 {
                Theme::global().WARN
            } else {
                Theme::global().ACCENT
            },
        ));
    }
    line
}

/// A saved-grant count is not an offer to reset the current session.
fn resets_text(reset: &crate::accounts::BankedReset) -> Option<String> {
    let expiry = reset
        .expires_at
        .as_deref()
        .map(|at| format!(" · exp {}", jcode_base::usage::format_reset_time(at)))
        .unwrap_or_default();
    let imported = reset
        .cached_source
        .as_deref()
        .map(|_| " · imported")
        .unwrap_or_default();
    match (reset.usable_count, reset.available_count) {
        (Some(_), available) if available > 0 => {
            Some(format!("↻ {available} saved{imported}{expiry}"))
        }
        (None, available) if available > 0 => Some(format!(
            "↻ {available} {}{expiry}{imported}",
            if available == 1 { "reset" } else { "resets" }
        )),
        _ => reset.next_available_at.as_deref().map(|at| {
            format!(
                "↻ 0 resets · next in {}",
                jcode_base::usage::format_reset_time(at)
            )
        }),
    }
}

fn resets_line(account_id: &str, login: usize, report: &UsageReport) -> Option<gpui::Div> {
    let reset = report.banked_reset.as_ref()?;
    let text = resets_text(reset)?;
    let id = account_id.to_owned();
    let mut lines = div()
        .debug_selector(move || format!("account-{id}-login-{login}-resets"))
        .flex()
        .flex_col()
        .child(caption(text));
    if let Some(reason) = reset.inactive_reason() {
        lines = lines.child(caption(reason));
    }
    Some(lines)
}

/// Credits of a provider that has no saved-login identities (Cursor): its one
/// credential's balance or spend, or `None` when it reports none.
pub(super) fn provider_credits(account: &Account) -> Option<gpui::Div> {
    let credits = account
        .usage_reports
        .iter()
        .find_map(|report| report.credits.as_ref())?;
    Some(credits_line(&account.id, 0, credits).mt(px(3.0)))
}

/// The block for every login of `account`, or `None` for a provider without
/// saved-login identities.
///
/// `icon_for` names the icon of a login by its label. `reset_pill` renders the redeem pill for a login's banked reset, or `None`
/// when that reset is not offerable. The pill sits inside the login's block, so
/// it never floats below another login's.
pub(super) fn login_blocks(
    account: &Account,
    icon_for: &dyn Fn(&str) -> Option<String>,
    reset_pill: &mut dyn FnMut(&crate::accounts::BankedReset) -> Option<gpui::AnyElement>,
) -> Option<gpui::Div> {
    if !lists_logins(account) {
        return None;
    }
    let mut blocks = div().mt(px(4.0)).flex().flex_col().gap(px(8.0));
    for (login, report) in account.usage_reports.iter().enumerate() {
        let id = account.id.clone();
        let name = login_name(report);
        let active = report
            .identity
            .as_ref()
            .is_some_and(|identity| identity.active);
        let kind = report.identity.as_ref().map(|identity| identity.kind);
        let icon = report
            .identity
            .as_ref()
            .and_then(|identity| icon_for(&identity.label));
        let mut block = div()
            .debug_selector({
                let id = id.clone();
                move || format!("account-{id}-login-{login}")
            })
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(14.0))
                    .line_height(px(14.0))
                    .children(icon.map(|icon| {
                        div()
                            .debug_selector({
                                let id = id.clone();
                                move || format!("account-{id}-login-{login}-icon")
                            })
                            .flex_none()
                            .text_size(px(10.0))
                            .child(icon)
                    }))
                    .child(
                        div()
                            .debug_selector({
                                let id = id.clone();
                                move || format!("account-{id}-login-{login}-name")
                            })
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(10.0))
                            .text_color(Theme::global().TEXT)
                            .child(if active { format!("{name} ✦") } else { name }),
                    ),
            );
        if let Some(kind) = kind {
            block = block.child(
                div()
                    .debug_selector({
                        let id = id.clone();
                        move || format!("account-{id}-login-{login}-kind")
                    })
                    .h(px(11.0))
                    .line_height(px(11.0))
                    .text_size(px(8.0))
                    .text_color(kind_color(kind))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(kind.label()),
            );
        }
        // Two windows per row, like the single-login row.
        let limits: Vec<_> = report.limits.iter().take(2).collect();
        if !limits.is_empty() {
            let mut row = div().flex().gap(px(6.0));
            for (index, limit) in limits.into_iter().enumerate() {
                row = row.child(limit_cell(&id, login, index, limit));
            }
            block = block.child(row);
        }
        if let Some(credits) = &report.credits {
            block = block.child(credits_line(&id, login, credits));
        }
        if let Some(resets) = resets_line(&id, login, report) {
            block = block.child(resets);
        }
        if let Some(pill) = report.banked_reset.as_ref().and_then(&mut *reset_pill) {
            block = block.child(div().mt(px(1.0)).flex().child(pill));
        }
        blocks = blocks.child(block);
    }
    Some(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREE_CLAUDE_LOGINS: &str = r#"{"providers":[
      {"provider_name":"Anthropic - team · Max 20x ✦","limits":[
         {"name":"5-hour window","usage_percent":41.0,"reset_in":"2h"},
         {"name":"7-day window","usage_percent":59.0,"reset_in":"3d"}],
       "extra_info":[["Account label","claude-otter"]],
       "banked_reset":{"provider":"claude","account_label":"claude-otter","available_count":2,"usable_count":1,
                       "expires_at":"2099-01-01T00:00:00Z","limit_reached":false},
       "account":{"provider":"claude","label":"claude-otter","name":"team","display_name":"team · Max 20x","plan":"Max 20x","kind":"subscription","active":true}},
      {"provider_name":"Anthropic - m***x@example.com · Max","limits":[
         {"name":"5-hour window","usage_percent":12.0,"reset_in":"4h"},
         {"name":"7-day window","usage_percent":22.0,"reset_in":"5d"}],
       "extra_info":[["Account label","claude-fox"]],
       "banked_reset":{"provider":"claude","account_label":"claude-fox","available_count":1,"usable_count":1,
                       "expires_at":"2099-02-01T00:00:00Z","limit_reached":false},
       "account":{"provider":"claude","label":"claude-fox","display_name":"m***x@example.com · Max","plan":"Max","kind":"subscription","active":false}},
      {"provider_name":"Anthropic - ent · Enterprise","limits":[],
       "extra_info":[["Account label","claude-panda"]],
       "credits":{"state":"unknown","unit":"usd","unlimited":false,"summary":"unknown"},
       "account":{"provider":"claude","label":"claude-panda","name":"ent","display_name":"ent · Enterprise","plan":"Enterprise","kind":"credits","active":false}}
    ]}"#;

    /// Debug selectors are `'static`. Tests build a few, so leaking is fine.
    fn selector(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    #[gpui::test]
    fn every_login_of_a_provider_gets_its_own_named_block(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            w.sidebar_view = SidebarView::Accounts;
            let mut accounts = accounts::parse(
                r#"{"providers":[{"id":"claude","display_name":"Claude","status":"available","auth_kind":"OAuth"}]}"#,
            )
            .unwrap();
            accounts::merge_usage_for_tests(&mut accounts, THREE_CLAUDE_LOGINS);
            w.accounts = accounts;
            cx.notify();
        });
        vcx.run_until_parked();

        let names: Vec<_> = (0..3)
            .map(|login| {
                vcx.debug_bounds(selector(format!("account-claude-login-{login}-name")))
                    .unwrap_or_else(|| panic!("login {login} has no name row"))
            })
            .collect();
        assert!(names[0].bottom() <= names[1].top(), "{names:?}");
        assert!(names[1].bottom() <= names[2].top(), "{names:?}");
        // Each login keeps its own windows, resets and billing kind.
        for login in [0, 1] {
            for part in ["limit-0", "limit-1", "resets", "kind"] {
                assert!(
                    vcx.debug_bounds(selector(format!("account-claude-login-{login}-{part}")))
                        .is_some(),
                    "login {login} lacks {part}"
                );
            }
        }
        // A credits login without a readable balance says so and shows no bars.
        assert!(vcx.debug_bounds("account-claude-login-2-credits").is_some());
        assert!(vcx.debug_bounds("account-claude-login-2-limit-0").is_none());
        let row = vcx.debug_bounds("account-claude").unwrap();
        let last = vcx.debug_bounds("account-claude-login-2").unwrap();
        assert!(last.bottom() <= row.bottom(), "logins must not be clipped");
        // The provider row no longer borrows the first login's windows.
        assert!(vcx.debug_bounds("account-claude-limit-0").is_none());
    }

    #[gpui::test]
    fn every_login_leads_with_its_account_icon_on_one_line(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            w.sidebar_view = SidebarView::Accounts;
            let mut accounts = accounts::parse(
                r#"{"providers":[{"id":"claude","display_name":"Claude","status":"available","auth_kind":"OAuth"}]}"#,
            )
            .unwrap();
            accounts::merge_usage_for_tests(&mut accounts, THREE_CLAUDE_LOGINS);
            w.accounts = accounts;
            let mut view = crate::routing::feed::fixture();
            view.accounts[0].id = "claude:claude-fox".into();
            view.accounts[0].icon = Some("🚀".into());
            w.set_test_routing_view(view);
            cx.notify();
        });
        vcx.run_until_parked();
        for login in 0..3 {
            let icon = vcx
                .debug_bounds(selector(format!("account-claude-login-{login}-icon")))
                .unwrap_or_else(|| panic!("login {login} has no icon"));
            let name = vcx
                .debug_bounds(selector(format!("account-claude-login-{login}-name")))
                .unwrap();
            assert!(icon.right() <= name.left(), "icon first: {icon:?} {name:?}");
            assert!(icon.top() < name.bottom() && name.top() < icon.bottom(), "one line");
        }
        workspace.read_with(vcx, |w, _| {
            assert_eq!(
                w.account_icon("claude", "claude-fox").as_deref(),
                Some("🚀")
            );
            assert_eq!(
                w.account_icon("claude", "claude-otter").as_deref(),
                Some("🦦")
            );
        });
    }

    #[gpui::test]
    fn each_logins_reset_pill_sits_inside_its_own_block(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            w.sidebar_view = SidebarView::Accounts;
            let mut accounts = accounts::parse(
                r#"{"providers":[{"id":"claude","display_name":"Claude","status":"available","auth_kind":"OAuth"}]}"#,
            )
            .unwrap();
            let reached = THREE_CLAUDE_LOGINS.replace("\"limit_reached\":false", "\"session_reset_available\":true,\"limit_reached\":true");
            accounts::merge_usage_for_tests(&mut accounts, &reached);
            w.accounts = accounts;
            cx.notify();
        });
        vcx.run_until_parked();
        for login in [0, 1] {
            let pill = vcx
                .debug_bounds(selector(format!("account-claude-reset-{login}")))
                .unwrap_or_else(|| panic!("login {login} has no reset pill"));
            let block = vcx
                .debug_bounds(selector(format!("account-claude-login-{login}")))
                .unwrap();
            assert!(
                pill.top() >= block.top() && pill.bottom() <= block.bottom(),
                "pill {pill:?} outside block {block:?}"
            );
        }
    }

    #[test]
    fn imported_saved_resets_do_not_imply_a_session_offer() {
        let mut accounts = accounts::parse(
            r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"}]}"#,
        )
        .unwrap();
        accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[{
            "provider_name":"Anthropic - otter", "limits":[],
            "extra_info":[["Account label","claude-otter"]],
            "banked_reset":{"provider":"claude","account_label":"claude-otter",
                "available_count":1,"usable_count":1,"cached_source":"imported",
                "session_reset_available":false,"limit_reached":false,
                "grants":[{"status":"redeemed"},{"status":"available"}]}
        }]}"#,
        );
        let reset = accounts[0].usage_reports[0].banked_reset.as_ref().unwrap();
        assert_eq!(resets_text(reset).as_deref(), Some("↻ 1 saved · imported"));
        assert_eq!(reset.inactive_reason(), Some("Session reset unavailable"));
        assert!(!reset.offerable());
        assert_eq!(reset.account_label.as_deref(), Some("claude-otter"));
    }

    #[test]
    fn unreadable_credits_are_unknown_and_never_a_bar() {
        let accounts = {
            let mut accounts = accounts::parse(
                r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"}]}"#,
            )
            .unwrap();
            accounts::merge_usage_for_tests(&mut accounts, THREE_CLAUDE_LOGINS);
            accounts
        };
        let ent = &accounts[0].usage_reports[2];
        let credits = ent.credits.as_ref().expect("credits");
        assert!(!credits.known);
        assert_eq!(credits.used_fraction, None);
        assert_eq!(
            ent.identity.as_ref().map(|identity| identity.kind),
            Some(LoginKind::Credits)
        );
        assert_eq!(
            accounts[0].usage_reports[0]
                .identity
                .as_ref()
                .map(|identity| identity.display_name.as_str()),
            Some("team · Max 20x")
        );
    }
}
