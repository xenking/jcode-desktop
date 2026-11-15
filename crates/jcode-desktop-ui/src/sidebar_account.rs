//! The Jcode account pill pinned to the bottom of the sidebar.
//!
//! It names the signed-in Jcode account (not an AI provider login) with its
//! plan. Clicking it opens the in-app account menu, or sign-in when signed out.
use super::*;
use jcode_base::provider_catalog::load_env_value_from_env_or_config;
use jcode_base::subscription_catalog as subscription;
use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the sidebar may show the account email. Off by default so a
/// streamed or shared screen never reveals it.
fn show_email_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(crate::config::get().workspace.show_account_email))
}

pub(super) fn show_email() -> bool {
    show_email_flag().load(Ordering::Relaxed)
}

/// The locally saved Jcode account. Only non-secret metadata is kept here.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct JcodeAccount {
    pub signed_in: bool,
    pub email: Option<String>,
    pub plan: Option<&'static str>,
}

thread_local! {
    static CACHE: RefCell<Option<(Instant, JcodeAccount)>> = const { RefCell::new(None) };
}

impl JcodeAccount {
    fn load() -> Self {
        if account_menu::fixture_enabled() {
            return Self {
                signed_in: true,
                email: Some("ada@example.com".into()),
                plan: Some("Pro"),
            };
        }
        let value = |key| {
            load_env_value_from_env_or_config(key, subscription::JCODE_ENV_FILE)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        Self {
            signed_in: subscription::has_credentials(),
            email: value(subscription::JCODE_ACCOUNT_EMAIL_ENV),
            plan: subscription::cached_tier().map(subscription::JcodeTier::display_name),
        }
    }

    /// Re-read the account file at most every few seconds, so sign-in and
    /// sign-out from the CLI show up without hitting the disk every frame.
    pub(super) fn current() -> Self {
        const STALE: Duration = Duration::from_secs(3);
        CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            match &*cache {
                Some((at, account)) if at.elapsed() < STALE => account.clone(),
                _ => {
                    let account = Self::load();
                    *cache = Some((Instant::now(), account.clone()));
                    account
                }
            }
        })
    }

    /// Forget the cached account so the next frame re-reads it, e.g. after
    /// signing out from the account menu.
    pub(super) fn invalidate() {
        CACHE.with(|cache| *cache.borrow_mut() = None);
    }

    fn title(&self, show_email: bool) -> String {
        match (&self.email, self.signed_in) {
            (Some(email), true) if show_email => email.clone(),
            (_, true) => "Jcode account".into(),
            _ => "Sign in to Jcode".into(),
        }
    }

    fn detail(&self) -> String {
        match (self.signed_in, self.plan) {
            (true, Some(plan)) => format!("{plan} plan"),
            (true, None) => "Signed in".into(),
            (false, _) => "Not signed in".into(),
        }
    }

    fn initial(&self, show_email: bool) -> String {
        self.email
            .as_deref()
            .filter(|_| self.signed_in && show_email)
            .and_then(|email| email.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "J".into())
    }
}

impl Workspace {
    pub(super) fn render_sidebar_account(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // The avatar is the app's single update control. `render_sidebar` has
        // already started the release check for this frame.
        let offer = version::offered_update();
        let theme = Theme::global();
        let account = JcodeAccount::current();
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
                                        .tooltip(move |_, cx| {
                                            cx.new(|_| {
                                                live_tabs::TabTooltip(
                                                    format!(
                                                        "{}\nClick to {}",
                                                        offer.label,
                                                        offer.action.to_lowercase()
                                                    )
                                                    .into(),
                                                )
                                            })
                                            .into()
                                        })
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
                        cx.new(|_| crate::panel::usage::MeterTooltip(account_tooltip.clone()))
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
            chips.into_iter().map(|chip| {
                    let id = format!("sidebar-quota-{}", chip.logo);
                    let selector = id.clone();
                    let tooltip = chip.tooltip();
                    let color = match chip.percent {
                        Some(percent) if percent >= 70. => {
                            crate::panel::usage::usage_color(percent, theme)
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
                        .tooltip(move |_, cx| {
                            cx.new(|_| crate::panel::usage::MeterTooltip(tooltip.clone()))
                                .into()
                        })
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
                    show_email_flag().store(enabled, Ordering::Relaxed);
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
