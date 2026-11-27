//! Account routing state a session panel shows: the account chip. The gate
//! card, the view feed, and the popover live in submodules. GPUI-free so the
//! behaviour is tested without a window.

pub mod feed;
pub mod gate;
pub mod popover;
pub mod rows;

use jcode_session_types::account_icon::{disambiguating_icon, provider_of};
use jcode_session_types::routing::RoutingView;

/// The account serving a session, why, and what the daemon warned about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingChip {
    pub account: String,
    pub reason: String,
    pub warnings: Vec<String>,
    /// The account a warm session moves to at its next cache-cold seam.
    pub pending: Option<String>,
    /// Icons of `account` and `pending`, shown first wherever they are named.
    /// `None` when the account's provider has no other account to tell it from.
    pub icon: Option<String>,
    pub pending_icon: Option<String>,
}

impl RoutingChip {
    pub fn new(
        account: String,
        reason: String,
        warnings: Vec<String>,
        pending: Option<String>,
    ) -> Option<Self> {
        let account = clean(&account);
        let pending = pending.map(|p| clean(&p)).filter(|p| !p.is_empty());
        (!account.is_empty()).then(|| Self {
            icon: None,
            pending_icon: None,
            account,
            reason: clean(&reason),
            warnings: warnings
                .iter()
                .map(|w| clean(w))
                .filter(|w| !w.is_empty())
                .collect(),
            pending,
        })
    }

    /// Give the accounts their icons: `lookup` answers for an account id, or
    /// `None` when the account shows none.
    pub fn with_icons(mut self, lookup: impl Fn(&str) -> Option<String>) -> Self {
        self.icon = lookup(&self.account);
        self.pending_icon = self.pending.as_deref().and_then(lookup);
        self
    }

    /// `🦊 claude:claude-fox`: the account with its icon first.
    pub fn account_text(&self) -> String {
        match &self.icon {
            Some(icon) => format!("{icon} {}", self.account),
            None => self.account.clone(),
        }
    }

    pub fn pending_text(&self) -> Option<String> {
        let pending = self.pending.as_deref()?;
        Some(match &self.pending_icon {
            Some(icon) => format!("{icon} {pending}"),
            None => pending.to_owned(),
        })
    }

    /// `⚠` for one warning, `⚠3` for several, nothing for none.
    pub fn warning_marker(&self) -> Option<String> {
        match self.warnings.len() {
            0 => None,
            1 => Some("⚠".into()),
            count => Some(format!("⚠{count}")),
        }
    }

    /// The hover text, one entry per line.
    pub fn tooltip(&self) -> Vec<String> {
        let mut lines = vec![format!("Running on {}", self.account_text())];
        if !self.reason.is_empty() {
            lines.push(format!("Why: {}", self.reason));
        }
        if let Some(pending) = self.pending_text() {
            lines.push(format!("Moves to {pending} at the next cache-cold turn"));
        }
        lines.extend(self.warnings.iter().map(|warning| format!("⚠ {warning}")));
        lines
    }
}

/// The icon of `account` (`provider:label`) where it tells accounts apart: the
/// daemon's resolved icon for a routing account, else the stable default when
/// the view lists several accounts (or keys) of the provider. `None` for a
/// provider's only account, and while no view has arrived.
pub fn icon_for(view: Option<&RoutingView>, account: &str) -> Option<String> {
    let view = view?;
    if let Some(routed) = view.accounts.iter().find(|a| a.id == account) {
        return routed.icon.clone();
    }
    let provider = provider_of(account);
    let keys = view
        .api_keys
        .iter()
        .flat_map(|provider| provider.keys.iter().map(|key| key.account.as_str()));
    let ids = view.accounts.iter().map(|a| a.id.as_str()).chain(keys);
    let siblings = ids
        .filter(|id| provider_of(id) == provider && *id != account)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        + 1;
    disambiguating_icon(account, None, siblings)
}

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
