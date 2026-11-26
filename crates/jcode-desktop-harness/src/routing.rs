//! Account routing state a session panel shows: the account chip. The gate
//! card, the view feed, and the popover live in submodules. GPUI-free so the
//! behaviour is tested without a window.

pub mod feed;
pub mod gate;
pub mod popover;
pub mod rows;

use jcode_session_types::account_icon::account_icon;
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
    pub icon: String,
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
            icon: account_icon(&account, None),
            pending_icon: pending.as_deref().map(|p| account_icon(p, None)),
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

    /// Swap in configured icons: `lookup` answers for an account id, or `None`
    /// to keep the stable default.
    pub fn with_icons(mut self, lookup: impl Fn(&str) -> Option<String>) -> Self {
        if let Some(icon) = lookup(&self.account) {
            self.icon = icon;
        }
        if let Some(pending) = &self.pending
            && let Some(icon) = lookup(pending)
        {
            self.pending_icon = Some(icon);
        }
        self
    }

    /// `🦊 claude:claude-fox`: the account with its icon first.
    pub fn account_text(&self) -> String {
        format!("{} {}", self.icon, self.account)
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

/// The icon of `account` (`provider:label`): the daemon's resolved icon for a
/// routing account, else the stable default for its label.
pub fn icon_for(view: Option<&RoutingView>, account: &str) -> String {
    let configured = view
        .and_then(|view| view.accounts.iter().find(|a| a.id == account))
        .and_then(|a| a.icon.as_deref());
    account_icon(account, configured)
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
