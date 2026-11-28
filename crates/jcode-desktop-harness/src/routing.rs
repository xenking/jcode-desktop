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
    /// The account the session left when the daemon moved it, until the user
    /// has looked at the popover.
    pub moved_from: Option<String>,
    /// Icon of `account`, shown first wherever it is named. `None` when the
    /// account's provider has no other account to tell it from.
    pub icon: Option<String>,
}

/// An account as people say it: the name they gave the login, without the
/// provider prefix of its id.
pub fn account_name(account: &str) -> &str {
    account.split_once(':').map_or(account, |(_, name)| name)
}

/// `text` with every `provider:name` id in it said as `name`.
fn named(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let id = word.trim_matches(|c| matches!(c, '(' | ')' | ',' | '.' | ';' | ':'));
            match id.split_once(':') {
                Some((provider, name))
                    if !provider.is_empty()
                        && !name.is_empty()
                        && !provider.contains(|c: char| !c.is_ascii_lowercase() && c != '-') =>
                {
                    word.replacen(id, name, 1)
                }
                _ => word.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl RoutingChip {
    pub fn new(account: String, reason: String, warnings: Vec<String>) -> Option<Self> {
        let account = clean(&account);
        (!account.is_empty()).then(|| Self {
            icon: None,
            account,
            reason: clean(&reason),
            warnings: warnings
                .iter()
                .map(|w| clean(w))
                .filter(|w| !w.is_empty())
                .collect(),
            moved_from: None,
        })
    }

    /// Give the account its icon: `lookup` answers for an account id, or
    /// `None` when the account shows none.
    pub fn with_icons(mut self, lookup: impl Fn(&str) -> Option<String>) -> Self {
        self.icon = lookup(&self.account);
        self
    }

    /// `🦊 claude-fox`: the account's name with its icon first.
    pub fn account_text(&self) -> String {
        let name = account_name(&self.account);
        match &self.icon {
            Some(icon) => format!("{icon} {name}"),
            None => name.to_owned(),
        }
    }

    /// The chip is worth showing only when something needs looking at: the
    /// daemon moved the session, or warns about the account.
    pub fn is_alert(&self) -> bool {
        self.moved_from.is_some() || !self.warnings.is_empty()
    }

    /// `⚠` for one warning, `⚠3` for several, nothing for none.
    pub fn warning_marker(&self) -> Option<String> {
        match self.warnings.len() {
            0 => None,
            1 => Some("⚠".into()),
            count => Some(format!("⚠{count}")),
        }
    }

    /// The popover's facts: the account, then at most one line that says why
    /// it is worth a look: the move that happened, else the first warning.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Running on {}", self.account_text())];
        if let Some(from) = &self.moved_from {
            let from = account_name(from);
            let why = named(&self.reason);
            lines.push(if why.is_empty() {
                format!("Moved from {from}")
            } else if why.contains(from) {
                // The daemon's reason already says who left.
                let mut chars = why.chars();
                chars.next().map_or(String::new(), |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            } else {
                format!("Moved from {from}: {why}")
            });
        } else if let Some(warning) = self.warnings.first() {
            lines.push(format!("⚠ {}", named(warning)));
        }
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
