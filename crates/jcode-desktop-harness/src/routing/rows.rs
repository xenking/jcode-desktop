//! What the accounts sidebar shows of the routing view: labelled API keys with
//! their usage windows, and budgets with used against limit.
//!
//! Bars follow the accounts rows: red from 90 percent, amber from 70, accent
//! below. A meter the runtime could not read has no bar and says `unknown`,
//! never zero.

use jcode_session_types::routing::{
    RoutingBudgetView, RoutingKeyView, RoutingView, RoutingWindowView,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Warn,
    Error,
}

/// A bar's colour for a used fraction, shared by windows and budgets.
fn tone_for(fraction: f32) -> Tone {
    if fraction >= 0.9 {
        Tone::Error
    } else if fraction >= 0.7 {
        Tone::Warn
    } else {
        Tone::Accent
    }
}

/// One line of text with an optional bar under it.
#[derive(Debug, Clone, PartialEq)]
pub struct BarRow {
    pub label: String,
    /// Used fraction in 0..=1. `None` draws no bar.
    pub fraction: Option<f32>,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyRow {
    /// The key's account icon, shown before the title.
    pub icon: String,
    /// `provider` for the unlabelled key, `provider · label` otherwise.
    pub title: String,
    pub active: bool,
    pub windows: Vec<BarRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetRow {
    pub name: String,
    /// Where it applies, such as `huggingface · month`.
    pub scope: String,
    /// `$82.00 / $100.00`, or `unknown / $100.00` when unreadable.
    pub summary: String,
    pub bar: Option<(f32, Tone)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidebarRouting {
    pub keys: Vec<KeyRow>,
    pub budgets: Vec<BudgetRow>,
}

impl SidebarRouting {
    pub fn from_view(view: &RoutingView, now_unix: i64) -> Self {
        let keys = view
            .api_keys
            .iter()
            .flat_map(|provider| {
                provider
                    .keys
                    .iter()
                    .filter(|key| key.label.is_some() || !key.windows.is_empty())
                    .map(|key| key_row(view, &provider.provider, key, now_unix))
            })
            .collect();
        let budgets = view.budgets.iter().map(budget_row).collect();
        Self { keys, budgets }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.budgets.is_empty()
    }
}

fn key_row(view: &RoutingView, provider: &str, key: &RoutingKeyView, now_unix: i64) -> KeyRow {
    let id = match &key.label {
        Some(label) => format!("{provider}:{label}"),
        None => provider.to_string(),
    };
    KeyRow {
        icon: super::icon_for(Some(view), &id),
        title: match &key.label {
            Some(label) => format!("{provider} · {label}"),
            None => provider.to_string(),
        },
        active: key.active,
        windows: key
            .windows
            .iter()
            .map(|window| window_row(window, now_unix))
            .collect(),
    }
}

fn window_row(window: &RoutingWindowView, now_unix: i64) -> BarRow {
    let name = match &window.model_class {
        Some(class) => format!("{class} {}", window.window),
        None => window.window.clone(),
    };
    let amount = match (window.unit.as_str(), window.limit) {
        ("percent", _) => format!("{:.0}%", window.used.clamp(0.0, 100.0)),
        (unit, Some(limit)) => format!("{} / {}", amount(unit, window.used), amount(unit, limit)),
        (unit, None) => amount(unit, window.used),
    };
    let label = match window.resets_at.and_then(|at| reset_in(at, now_unix)) {
        Some(reset) => format!("{name} · {amount} · {reset}"),
        None => format!("{name} · {amount}"),
    };
    let fraction = match (window.unit.as_str(), window.limit) {
        ("percent", _) => Some((window.used / 100.0) as f32),
        (_, Some(limit)) if limit > 0.0 => Some((window.used / limit) as f32),
        _ => None,
    }
    .map(|fraction| fraction.clamp(0.0, 1.0));
    BarRow {
        label,
        tone: fraction.map_or(Tone::Accent, tone_for),
        fraction,
    }
}

fn budget_row(budget: &RoutingBudgetView) -> BudgetRow {
    let window = match budget.window.as_str() {
        "calendar-month" => "month",
        other => other,
    };
    let limit = amount(&budget.unit, budget.limit);
    let (summary, bar) = match budget.used {
        None => (format!("unknown / {limit}"), None),
        Some(used) => {
            let fraction = (budget.limit > 0.0).then(|| (used / budget.limit) as f32);
            let tone = if used >= budget.limit {
                Tone::Error
            } else if budget.warn.is_some_and(|warn| used >= warn) {
                Tone::Warn
            } else if budget.warn.is_none() {
                fraction.map_or(Tone::Accent, |fraction| {
                    if fraction >= 0.7 {
                        Tone::Warn
                    } else {
                        Tone::Accent
                    }
                })
            } else {
                Tone::Accent
            };
            (
                format!("{} / {limit}", amount(&budget.unit, used)),
                fraction.map(|fraction| (fraction.clamp(0.0, 1.0), tone)),
            )
        }
    };
    BudgetRow {
        name: budget.name.clone(),
        scope: format!("{} · {window}", budget.scope),
        summary,
        bar,
    }
}

/// `41%`, `$82.00`, `12 credits`, `1.2M tokens`.
fn amount(unit: &str, value: f64) -> String {
    match unit {
        "percent" => format!("{value:.0}%"),
        "usd" => format!("${value:.2}"),
        "credits" => format!("{value:.0} credits"),
        "tokens" if value >= 1_000_000.0 => format!("{:.1}M tokens", value / 1_000_000.0),
        "tokens" if value >= 1_000.0 => format!("{:.0}k tokens", value / 1_000.0),
        "tokens" => format!("{value:.0} tokens"),
        _ => format!("{value:.0}"),
    }
}

/// Time until a reset, such as `2h 56m` or `3d 4h`. `None` once it has passed.
fn reset_in(resets_at: i64, now_unix: i64) -> Option<String> {
    let secs = resets_at - now_unix;
    if secs <= 0 {
        return None;
    }
    let (days, hours, minutes) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    Some(if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{}m", minutes.max(1))
    })
}

#[cfg(test)]
mod tests;
