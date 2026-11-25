//! The Jcode account pill pinned to the bottom of the sidebar.
//!
//! It names the signed-in Jcode account (not an AI provider login) with its
//! plan. Clicking it opens the in-app account menu, or sign-in when signed out.
use super::jcode_account::{JcodeAccount, set_show_email, show_email};
use super::account_sign_in::account_button;
use super::*;

impl Workspace {
    pub(super) fn render_sidebar_account(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // The avatar is the app's single update control. `render_sidebar` has
        // already started the release check for this frame.
        let offer = version::offered_update();
        let theme = Theme::global();
        let account = JcodeAccount::current(cx);
        let signed_in = account.signed_in;
        let show_email = show_email();
        let ink = if signed_in {
            theme.TEXT
        } else {
            theme.TEXT_DIM
        };
        let menu_open = signed_in && self.account_menu.open;
        // The block spells out whom it belongs to only on hover now.
        let account_tooltip = format!("{} · {}", account.title(show_email), account.detail());

        div()
            .id("sidebar-account-area")
            .flex_none()
            .px_2()
            .pt_1()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .when(menu_open, |el| {
                el.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_account_menu(cx)))
                    .child(self.render_account_menu(cx))
            })
            .child(
                div()
                    .id("sidebar-account")
                    .debug_selector(|| "sidebar-account".into())
                    .h(px(40.0))
                    .pl(px(8.0))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if menu_open {
                        theme.HEADER_BG
                    } else {
                        theme.INLINE_CODE_BG
                    })
                    .hover(|el| el.bg(theme.HEADER_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if signed_in {
                                this.toggle_account_menu(cx);
                                return;
                            }
                            // Email sign-in is paused, so reuse the Jcode
                            // provider login the Accounts page already runs.
                            let source = this
                                .slots
                                .get(this.active)
                                .filter(|slot| !slot.closing)
                                .map(|slot| slot.panel.entity_id());
                            match source {
                                Some(source) => this.open_accounts(
                                    &OpenAccounts {
                                        source,
                                        login_command: Some("/login jcode".into()),
                                    },
                                    window,
                                    cx,
                                ),
                                None => this.open_account_sign_in(window, cx),
                            }
                        }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "sidebar-account-avatar".into())
                            .relative()
                            .size(px(26.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if signed_in {
                                theme.ACCENT_DIM
                            } else {
                                theme.PANEL_BG
                            })
                            .text_size(px(11.0))
                            .text_color(ink)
                            .child(account.initial(show_email))
                            // Update indication lives on the avatar and nowhere
                            // else. No update means a plain avatar with no
                            // extra text, exactly as before.
                            .when_some(offer, |el, offer| {
                                el.child(
                                    div()
                                        .id("sidebar-update-badge")
                                        .debug_selector(|| "sidebar-update-badge".into())
                                        .absolute()
                                        .top(px(-3.0))
                                        .right(px(-3.0))
                                        .size(px(13.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_full()
                                        .bg(theme.ACCENT)
                                        .text_color(theme.BG)
                                        .text_size(px(9.0))
                                        .line_height(px(9.0))
                                        .cursor_pointer()
                                        .hover(|el| el.opacity(0.85))
                                        .tooltip(
                                            sidebar_atoms::tab_tooltip(format!(
                                                "{}\nClick to {}",
                                                offer.label,
                                                offer.action.to_lowercase()
                                            ))
                                            .builder(),
                                        )
                                        // The badge acts on the press and keeps
                                        // the click from opening the account
                                        // popup, so a restart is never a
                                        // side effect of a mis-aimed click.
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(|_, _, window, cx| {
                                                cx.stop_propagation();
                                                window.prevent_default();
                                                Self::check_and_apply_update(cx);
                                                cx.notify();
                                            }),
                                        )
                                        .child("↑"),
                                )
                            }),
                    )
                    // The avatar plus one icon and figure per provider. The
                    // account and its plan live in the hover, so the block is
                    // usage at a glance instead of repeated identity.
                    .children(self.render_sidebar_quota(cx))
                    .child(div().flex_1())
                    .tooltip(move |_, cx| {
                        cx.new(|_| crate::panel::meter::MeterTooltip(account_tooltip.clone()))
                            .into()
                    }),
            )
            .into_any_element()
    }

    /// Provider quota icons for the account block: one icon plus figure per
    /// configured credential, red at exhaustion, each with the same hover card
    /// it had in the composer bar. Clicking one opens the account picker.
    fn render_sidebar_quota(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let Some(accounts) = cx.try_global::<crate::panel::usage::StatusAccounts>() else {
            return Vec::new();
        };
        let chips = crate::panel::usage::quota_chips(&accounts.0);
        if chips.is_empty() {
            return Vec::new();
        }
        let theme = Theme::global();
        let source = self
            .slots
            .get(self.active)
            .filter(|slot| !slot.closing)
            .map(|slot| slot.panel.entity_id());
        {
            chips
                .into_iter()
                .map(|chip| {
                    let id = format!("sidebar-quota-{}", chip.logo);
                    let selector = id.clone();
                    let account = accounts
                        .0
                        .iter()
                        .find(|account| account.id == chip.logo && account.status == "available")
                        .expect("quota chip has a matching account");
                    let tooltip = crate::panel::usage::quota_card(account, &chip);
                    let color = match chip.percent {
                        Some(percent) if percent >= 70. => {
                            crate::panel::meter::usage_color(percent, theme)
                        }
                        _ => theme.TEXT_DIM,
                    };
                    let logo = crate::accounts::logo(chip.logo).map(|bytes| {
                        gpui::svg()
                            .data(bytes)
                            .size(px(12.))
                            .flex_none()
                            .text_color(color)
                    });
                    div()
                        .id(gpui::SharedString::from(id))
                        .debug_selector(move || selector.clone())
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .h(px(22.))
                        .px(px(5.0))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.HEADER_BG))
                        .text_size(px(10.5))
                        .font_family(theme.FONT_MONO)
                        .text_color(color)
                        .when(chip.cached, |el| el.opacity(0.6))
                        .tooltip(move |_, cx| cx.new(|_| tooltip.clone()).into())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            match source {
                                Some(source) => this.open_accounts(
                                    &OpenAccounts {
                                        source,
                                        login_command: None,
                                    },
                                    window,
                                    cx,
                                ),
                                None => {
                                    this.status = "Open a session to switch provider".into();
                                    cx.notify();
                                }
                            }
                        }))
                        .children(logo)
                        .child(chip.label())
                        .into_any_element()
                })
                .collect()
        }
    }

    /// Settings row: reveal the account email in the sidebar. Off by default.
    pub(super) fn render_account_email_setting(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let enabled = show_email();
        div()
            .id("settings-show-account-email")
            .debug_selector(|| "settings-show-account-email".into())
            .flex_none()
            .px_2()
            .py_2()
            .rounded_sm()
            .flex()
            .justify_between()
            .gap_2()
            .cursor_pointer()
            .bg(theme.PANEL_BG)
            .hover(|el| el.bg(theme.TOOL_BG))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    let enabled = !show_email();
                    if let Err(error) = crate::config::persist_show_account_email(enabled) {
                        this.status = format!("Could not save account email setting: {error}");
                    }
                    set_show_email(enabled);
                    cx.notify();
                }),
            )
            .child("Show account email in sidebar")
            .child(
                div()
                    .text_color(if enabled {
                        theme.ACCENT
                    } else {
                        theme.TEXT_DIM
                    })
                    .child(if enabled { "On" } else { "Off" }),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn render_account_settings(&self, cx: &mut Context<Self>) -> gpui::Div {
        let connected = self.account_sign_in.is_connected();
        if !self.account_sign_in.offers_email() && !connected {
            return div();
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .py_3()
            .child(div().text_size(px(12.0)).child("Jcode account"))
            .child(
                div()
                    .text_color(Theme::global().TEXT_DIM)
                    .child(if connected {
                        "Account saved on this computer"
                    } else {
                        "Optional · Separate from your AI providers"
                    }),
            )
            .child(
                account_button(
                    "settings-account-sign-in",
                    if connected {
                        "Manage account"
                    } else {
                        "Sign in with email"
                    },
                    false,
                    false,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if connected {
                        if !this.account_menu.open {
                            this.toggle_account_menu(cx);
                        }
                    } else {
                        this.open_account_sign_in(window, cx);
                    }
                })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::JcodeAccount;

    /// The user's layout: one block at the bottom of the sidebar with the
    /// avatar on the left and the provider figures to its right. The account
    /// name and plan are hover text now, so the block never repeats identity
    /// over the usage it exists to show.
    #[gpui::test]
    fn the_quota_icons_sit_inside_the_account_block(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut accounts = crate::accounts::parse(
                r#"{"providers":[{"id":"claude","status":"available"},{"id":"openai","status":"available"}]}"#,
            )
            .unwrap();
            crate::accounts::merge_usage_for_tests(
                &mut accounts,
                r#"{"providers":[{"provider_name":"Anthropic (Claude)","limits":[{"name":"7-day window","usage_percent":41.0,"reset_in":"1d 11h"}],"extra_info":[],"error":null}]}"#,
            );
            cx.set_global(crate::panel::usage::StatusAccounts(accounts));
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx)
        });
        let _ = &workspace;
        vcx.run_until_parked();
        let block = vcx
            .debug_bounds("sidebar-account")
            .expect("the account block paints");
        let chip = vcx
            .debug_bounds("sidebar-quota-claude")
            .expect("Claude's figure sits in the block");
        assert!(
            chip.left() >= block.left()
                && chip.right() <= block.right()
                && chip.top() >= block.top()
                && chip.bottom() <= block.bottom(),
            "the quota icon lives inside the account pill: {chip:?} {block:?}"
        );
        let avatar = vcx.debug_bounds("sidebar-account-avatar").unwrap();
        assert!(
            avatar.right() <= chip.left(),
            "the avatar leads, then the provider figures: {avatar:?} {chip:?}"
        );
        assert!(
            vcx.debug_bounds("sidebar-account-name").is_none(),
            "the block no longer spells the account name inline"
        );
        assert!(vcx.debug_bounds("sidebar-account-detail").is_none());
        vcx.update(|window, cx| window.simulate_mouse_move(chip.center(), cx));
        vcx.run_until_parked();
        vcx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-limits-tooltip").is_some());
        assert!(vcx.debug_bounds("panel-limit-0").is_some());
        assert!(vcx.debug_bounds("panel-limit-bar-0").is_some());
        assert!(
            vcx.debug_bounds("sidebar-quota").is_none(),
            "no separate quota row above the block"
        );
    }

    #[test]
    fn signed_in_account_names_email_and_plan() {
        let account = JcodeAccount {
            signed_in: true,
            email: Some("ada@example.com".into()),
            plan: Some("Pro"),
        };
        assert_eq!(account.title(true), "ada@example.com");
        assert_eq!(account.detail(), "Pro plan");
        assert_eq!(account.initial(true), "A");
    }

    #[test]
    fn hidden_email_never_reaches_the_pill() {
        let account = JcodeAccount {
            signed_in: true,
            email: Some("ada@example.com".into()),
            plan: Some("Pro"),
        };
        assert_eq!(account.title(false), "Jcode account");
        assert_eq!(account.initial(false), "J");
        assert!(
            !crate::config::DesktopConfig::default()
                .workspace
                .show_account_email
        );
    }

    #[test]
    fn signed_out_account_invites_sign_in() {
        let account = JcodeAccount {
            signed_in: false,
            email: Some("stale@example.com".into()),
            plan: None,
        };
        assert_eq!(account.title(true), "Sign in to Jcode");
        assert_eq!(account.detail(), "Not signed in");
        assert_eq!(account.initial(true), "J");
    }
}
