//! The locally saved Jcode account and the sidebar's privacy switch for it.
//!
//! The lowest layer of the account surfaces: the account menu, the sign-in
//! flow and the sidebar pill all read it, and it reads none of them.
use super::*;
use jcode_base::provider_catalog::load_env_value_from_env_or_config;
use jcode_base::subscription_catalog as subscription;
use jcode_desktop_model::{fs_probe, probe_cache::ProbeCache};
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

pub(super) fn set_show_email(enabled: bool) {
    show_email_flag().store(enabled, Ordering::Relaxed);
}

/// Offline screenshot fixture: `JCODE_DESKTOP_SCREENSHOT_ACCOUNT_MENU=1`.
pub(super) fn fixture_enabled() -> bool {
    harness::screenshot_mode()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_ACCOUNT_MENU").as_deref() == Ok("1")
}

/// The locally saved Jcode account. Only non-secret metadata is kept here.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct JcodeAccount {
    pub signed_in: bool,
    pub email: Option<String>,
    pub plan: Option<&'static str>,
}

/// Re-read the account file at most every few seconds, so sign-in and
/// sign-out from the CLI show up without hitting the disk every frame.
const STALE: Duration = Duration::from_secs(3);

thread_local! {
    static CACHE: RefCell<ProbeCache<(), JcodeAccount>> =
        RefCell::new(ProbeCache::new(STALE, 1));
}

impl JcodeAccount {
    fn load() -> Self {
        fs_probe::note();
        if fixture_enabled() {
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

    /// Read the account file once, at startup, so the first frame is right.
    pub(super) fn prime() {
        CACHE.with_borrow_mut(|cache| cache.fill((), Self::load()));
    }

    /// The last known account. Render never reads the disk: a stale answer is
    /// refreshed on the background executor and the windows redraw if it changed.
    pub(super) fn current(cx: &gpui::App) -> Self {
        let account = CACHE.with_borrow_mut(|cache| cache.get(&()).cloned());
        crate::background_probe::drain_local(cx, &CACHE, |_| fs_probe::off_main(Self::load));
        account.unwrap_or(Self {
            signed_in: false,
            email: None,
            plan: None,
        })
    }

    /// Read the account again now, e.g. after signing out from the account
    /// menu. Runs in an event handler, never in render.
    pub(super) fn invalidate() {
        Self::prime();
    }

    pub(super) fn title(&self, show_email: bool) -> String {
        match (&self.email, self.signed_in) {
            (Some(email), true) if show_email => email.clone(),
            (_, true) => "Jcode account".into(),
            _ => "Sign in to Jcode".into(),
        }
    }

    pub(super) fn detail(&self) -> String {
        match (self.signed_in, self.plan) {
            (true, Some(plan)) => format!("{plan} plan"),
            (true, None) => "Signed in".into(),
            (false, _) => "Not signed in".into(),
        }
    }

    pub(super) fn initial(&self, show_email: bool) -> String {
        self.email
            .as_deref()
            .filter(|_| self.signed_in && show_email)
            .and_then(|email| email.chars().next())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "J".into())
    }
}
