//! Compact, truthful context and credential quota meters for the session footer.
use super::*;
use crate::accounts::{Account, UsageLimit};

#[derive(Default)]
pub(crate) struct StatusAccounts(pub Vec<Account>);
impl gpui::Global for StatusAccounts {}

/// Compact structured hover for multi-fact sidebar and composer controls.
pub(crate) struct RowsTooltip {
    pub(crate) title: String,
    pub(crate) rows: Vec<String>,
}

impl RowsTooltip {
    pub(crate) fn separated(title: impl Into<String>, text: &str) -> Self {
        Self {
            title: title.into(),
            rows: text
                .lines()
                .flat_map(|line| line.split(" · "))
                .filter(|row| !row.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }
}

impl Render for RowsTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .debug_selector(|| "rows-tooltip".into())
            .px_3()
            .py_2()
            .min_w(px(180.))
            .max_w(px(320.))
            .rounded_xl()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(11.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_color(theme.TEXT)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(self.title.clone()),
            )
            .children(
                self.rows
                    .iter()
                    .map(|row| div().text_color(theme.TEXT_DIM).child(row.clone())),
            )
    }
}

pub(crate) struct MeterTooltip(pub(crate) String);
impl Render for MeterTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("MeterTooltip");
        let theme = Theme::global();
        div()
            .px_3()
            .py_2()
            .max_w(px(380.))
            .rounded_md()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(11.))
            .text_color(theme.TEXT_DIM)
            .child(self.0.clone())
    }
}

/// Subscription quota shown when hovering the composer's method pill.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // hover card of the method pill, which left the composer bar
pub(crate) enum LimitsSummary {
    Limits(Vec<UsageLimit>),
    Unavailable(String),
    Details(Vec<String>),
}

/// Hover card for the method pill: the credential name plus one bar per
/// quota window with its reset time.
#[allow(dead_code)] // see LimitsSummary
#[derive(Clone)]
pub(crate) struct LimitsTooltip {
    pub(crate) method: String,
    pub(crate) summary: LimitsSummary,
    pub(crate) notes: Vec<String>,
}

impl Render for LimitsTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("LimitsTooltip");
        let theme = Theme::global();
        let body: Vec<gpui::AnyElement> = match &self.summary {
            LimitsSummary::Unavailable(reason) => vec![
                div()
                    .max_w(px(320.))
                    .text_color(theme.TEXT_DIM)
                    .child(format!("Usage limits unavailable because {reason}."))
                    .into_any_element(),
            ],
            LimitsSummary::Details(rows) => rows
                .iter()
                .map(|row| {
                    div()
                        .text_color(theme.TEXT_DIM)
                        .child(row.clone())
                        .into_any_element()
                })
                .collect(),
            LimitsSummary::Limits(limits) => limits
                .iter()
                .enumerate()
                .map(|(index, limit)| limit_row(index, limit, theme))
                .collect(),
        };
        div()
            .debug_selector(|| "panel-limits-tooltip".into())
            .px_3()
            .py_2()
            .min_w(px(220.))
            .rounded_xl()
            .bg(theme.HEADER_BG)
            .border_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(11.))
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_color(theme.TEXT)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(self.method.clone()),
            )
            .children(body)
            .children(
                self.notes
                    .iter()
                    .map(|note| div().text_color(theme.TEXT_FAINT).child(note.clone())),
            )
    }
}

#[allow(dead_code)]
fn limit_row(index: usize, limit: &UsageLimit, theme: &Theme) -> gpui::AnyElement {
    let used = limit
        .usage_percent
        .is_finite()
        .then(|| limit.usage_percent.clamp(0., 100.));
    let value = used
        .map(|used| {
            let used = used.round() as u8;
            if jcode_base::config::config().display.usage_display_used() {
                format!("{used}% used")
            } else {
                format!("{}% left", 100 - used)
            }
        })
        .unwrap_or_else(|| "not reported".into());
    let value_selector = format!("panel-limit-value-{index}-{value}");
    div()
        .debug_selector(move || format!("panel-limit-{index}"))
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_color(theme.TEXT_DIM)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(limit.name.clone()),
                )
                .child(
                    div()
                        .debug_selector(move || value_selector.clone())
                        .flex_none()
                        .font_family(theme.FONT_MONO)
                        .child(value),
                ),
        )
        .children(used.map(|used| usage_bar(index, used, theme)))
        .children(limit.reset_in.as_ref().map(|reset| {
            div()
                .text_size(px(10.))
                .text_color(theme.TEXT_FAINT)
                .child(format!("Resets in {reset}"))
        }))
        .into_any_element()
}

/// Shared quota bar for both the panel and sidebar limit cards.
fn usage_bar(index: usize, used: f32, theme: &Theme) -> gpui::Div {
    div()
        .debug_selector(move || format!("panel-limit-bar-{index}"))
        .w_full()
        .h(px(4.))
        .rounded_full()
        .overflow_hidden()
        .bg(theme.INLINE_CODE_BG)
        .child(
            div()
                .h_full()
                .w(relative(used / 100.))
                .rounded_full()
                .bg(usage_color(used, theme)),
        )
}

pub(crate) fn usage_color(used: f32, theme: &Theme) -> gpui::Rgba {
    if used >= 90. {
        theme.ERROR
    } else if used >= 70. {
        theme.WARN
    } else {
        gpui::rgb(0x64c864).into()
    }
}

/// Footer pill order. Every credential with a reported figure gets one, and a
/// credential with nothing to report gets none: a bare logo pill would read as
/// a broken meter.
const FOOTER_QUOTA_PROVIDERS: [(&str, &str); 5] = [
    ("claude", "claude"),
    ("openai", "openai"),
    ("opencode-go", "opencode-go"),
    ("opencode", "opencode"),
    ("openrouter", "openrouter"),
];

/// One compact footer chip per credential: its logo and the figure the
/// provider actually reports. Quota windows give a percentage; a key without a
/// quota API falls back to its real balance or the spend Jcode recorded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuotaChip {
    /// Logo id for `accounts::logo`.
    pub(crate) logo: &'static str,
    /// `23%` for a quota window, `$12.34` for a balance or spend figure.
    pub(crate) value: String,
    /// Tightest reported window, for the colour ramp. `None` on money chips.
    pub(crate) percent: Option<f32>,
    pub(crate) cached: bool,
    /// One hover-card line per reported fact.
    pub(crate) detail: Vec<String>,
}

/// Build the original limits card from the exact login represented by the chip.
pub(super) fn banked_reset_label(reset: &crate::accounts::BankedReset) -> String {
    if let Some(usable) = reset.usable_count {
        let expiry = reset
            .expires_at
            .as_deref()
            .map(|at| format!(" · expires in {}", jcode_base::usage::format_reset_time(at)))
            .unwrap_or_default();
        let source = reset
            .cached_source
            .as_deref()
            .map(|s| format!(" · cached {s}"))
            .unwrap_or_default();
        return format!(
            "{usable} usable / {} banked{expiry}{source}",
            reset.available_count
        );
    }
    if reset.provider == crate::accounts::ResetProvider::Claude
        && let Some(available) = reset.session_reset_available
    {
        if available {
            return "1 session reset available".into();
        }
        let next = reset
            .next_available_at
            .as_deref()
            .map(|at| format!(" · next in {}", jcode_base::usage::format_reset_time(at)))
            .unwrap_or_default();
        return format!("0 session resets available{next}");
    }
    match reset.available_count {
        1 => "1 reset banked".into(),
        n if n > 1 => format!("{n} resets banked"),
        _ => match reset.next_available_at.as_deref() {
            Some(at) => format!(
                "0 banked resets · next in {}",
                jcode_base::usage::format_reset_time(at)
            ),
            None => "0 banked resets".into(),
        },
    }
}

pub(crate) fn quota_card(account: &Account, chip: &QuotaChip) -> LimitsTooltip {
    let report = account.active_report();
    let method = serving_account_label(
        Some(&account.id),
        Some(&account.auth_kind),
        report.and_then(|r| r.account_label.as_deref()),
    );
    let limits = account.active_limits().unwrap_or_default();
    let summary = if limits.iter().any(|limit| limit.usage_percent.is_finite()) {
        LimitsSummary::Limits(limits.to_vec())
    } else {
        LimitsSummary::Details(chip.detail.clone())
    };
    let mut notes = Vec::new();
    if let Some(reset) = report
        .and_then(|r| r.banked_reset.as_ref())
        .filter(|r| r.available_count > 0)
    {
        notes.push(banked_reset_label(reset));
    }
    if chip.cached {
        notes.push("Cached usage".into());
    }
    LimitsTooltip {
        method,
        summary,
        notes,
    }
}

impl QuotaChip {
    pub(crate) fn label(&self) -> String {
        self.value.clone()
    }
}

/// First dollar amount in a usage line, trimmed to cents.
fn first_usd(text: &str) -> Option<String> {
    let start = text.find('$')?;
    let amount: String = text[start + 1..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = amount.parse().ok()?;
    Some(format!("${value:.2}"))
}

pub(crate) fn quota_chips(accounts: &[Account]) -> Vec<QuotaChip> {
    let mut chips = Vec::new();
    for (id, logo) in FOOTER_QUOTA_PROVIDERS {
        let Some(account) = accounts
            .iter()
            .find(|account| account.id == id && account.status == "available")
        else {
            continue;
        };
        let report = account.active_report();
        let cached = report.is_some_and(|report| {
            report
                .extra_info
                .iter()
                .any(|(key, value)| key == "Last updated" && value.contains("cached;"))
        });
        let windows: Vec<(String, f32, Option<String>)> = account
            .active_limits()
            .unwrap_or_default()
            .iter()
            .filter(|limit| limit.usage_percent.is_finite())
            .map(|limit| {
                (
                    limit.name.clone(),
                    limit.usage_percent.clamp(0., 100.),
                    limit.reset_in.clone(),
                )
            })
            .collect();
        if let Some(percent) = windows
            .iter()
            .map(|(_, percent, _)| *percent)
            .reduce(f32::max)
        {
            let detail = windows
                .iter()
                .map(|(window, percent, reset)| {
                    let reset = reset
                        .as_deref()
                        .map(|reset| format!(", resets in {reset}"))
                        .unwrap_or_default();
                    format!("{window}: {percent:.0}% used{reset}")
                })
                .collect();
            chips.push(QuotaChip {
                logo,
                value: format!("{percent:.0}%"),
                percent: Some(percent),
                cached,
                detail,
            });
            continue;
        }
        let info = report
            .map(|report| report.extra_info.as_slice())
            .unwrap_or(&[]);
        let field = |key: &str| {
            info.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        let (value, detail) = if let Some(balance) = field("Balance").and_then(first_usd) {
            (
                balance,
                vec![format!("Balance: {}", field("Balance").unwrap_or(""))],
            )
        } else if let Some(spend) = field("Local spend (this machine)").and_then(first_usd) {
            (
                spend.clone(),
                vec![format!("Spend today: {spend} (Jcode's own accounting)")],
            )
        } else if let Some(today) = field("Today").and_then(first_usd) {
            (today.clone(), vec![format!("Spend today: {today}")])
        } else {
            continue;
        };
        chips.push(QuotaChip {
            logo,
            value,
            percent: None,
            cached,
            detail,
        });
    }
    chips
}

impl Panel {
    #[allow(dead_code)] // moved to the sidebar's quota row; kept as the bar's fallback renderer
    pub(super) fn render_provider_usage_footer(&self, cx: &App) -> Option<gpui::Div> {
        if crate::harness::remote_host(&self.session_id).is_some() {
            return None;
        }
        let accounts = &cx.try_global::<StatusAccounts>()?.0;
        let chips = quota_chips(accounts);
        if chips.is_empty() {
            return None;
        }
        let theme = Theme::global();
        Some(
            div()
                .debug_selector(|| "panel-provider-usage".into())
                .flex()
                .flex_none()
                .items_center()
                .gap_1p5()
                .whitespace_nowrap()
                .text_size(px(10.5))
                .font_family(theme.FONT_MONO)
                .children(chips.into_iter().map(|chip| {
                    let id = format!("panel-quota-{}", chip.logo);
                    let selector = id.clone();
                    let account = accounts
                        .iter()
                        .find(|account| account.id == chip.logo && account.status == "available")
                        .expect("quota chip has a matching account");
                    let tip = quota_card(account, &chip);
                    let color = match chip.percent {
                        Some(percent) if percent >= 70. => usage_color(percent, theme),
                        _ => theme.TEXT_DIM,
                    };
                    div()
                        .id(SharedString::from(id))
                        .debug_selector(move || selector.clone())
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .h(px(22.))
                        .px_1()
                        .text_color(color)
                        .when(chip.cached, |el| el.opacity(0.6))
                        .tooltip(move |_, cx| cx.new(|_| tip.clone()).into())
                        .children(crate::accounts::logo(chip.logo).map(|bytes| {
                            gpui::svg()
                                .data(bytes)
                                .size(px(11.))
                                .flex_none()
                                .text_color(color)
                        }))
                        .child(chip.label())
                })),
        )
    }
}

/// Filled annulus segment from 12 o'clock, clockwise, `fraction` of a turn.
fn ring_path(
    center: gpui::Point<gpui::Pixels>,
    outer: f32,
    inner: f32,
    fraction: f32,
) -> Option<gpui::Path<gpui::Pixels>> {
    let fraction = fraction.clamp(0., 1.);
    if fraction <= 0. {
        return None;
    }
    let at = |r: f32, t: f32| {
        let angle = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        center + gpui::point(px(r * angle.cos()), px(r * angle.sin()))
    };
    let mut builder = gpui::PathBuilder::fill();
    if fraction >= 0.999 {
        // Two half arcs per circle: a single full-turn arc is degenerate.
        builder.move_to(at(outer, 0.));
        builder.arc_to(
            gpui::point(px(outer), px(outer)),
            px(0.),
            false,
            true,
            at(outer, 0.5),
        );
        builder.arc_to(
            gpui::point(px(outer), px(outer)),
            px(0.),
            false,
            true,
            at(outer, 0.),
        );
        builder.close();
        builder.move_to(at(inner, 0.));
        builder.arc_to(
            gpui::point(px(inner), px(inner)),
            px(0.),
            false,
            false,
            at(inner, 0.5),
        );
        builder.arc_to(
            gpui::point(px(inner), px(inner)),
            px(0.),
            false,
            false,
            at(inner, 0.),
        );
        builder.close();
    } else {
        let large = fraction > 0.5;
        builder.move_to(at(outer, 0.));
        builder.arc_to(
            gpui::point(px(outer), px(outer)),
            px(0.),
            large,
            true,
            at(outer, fraction),
        );
        builder.line_to(at(inner, fraction));
        builder.arc_to(
            gpui::point(px(inner), px(inner)),
            px(0.),
            large,
            false,
            at(inner, 0.),
        );
        builder.close();
    }
    builder.build().ok()
}

/// Circular context gauge: a faint full track with a colored used arc.
fn context_ring(used: Option<f32>) -> gpui::AnyElement {
    const SIZE: f32 = super::composer::BAR_ICON;
    let theme = Theme::global();
    let used = used.filter(|p| p.is_finite()).map(|p| p.clamp(0., 100.));
    div()
        .debug_selector(|| "panel-context-ring".into())
        .flex_none()
        .size(px(SIZE))
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let center = bounds.center();
                    let outer = SIZE / 2.;
                    let inner = outer - 2.6;
                    if let Some(track) = ring_path(center, outer, inner, 1.) {
                        window.paint_path(track, theme.INLINE_CODE_BG);
                    }
                    if let Some(used) = used
                        && let Some(arc) = ring_path(center, outer, inner, used / 100.)
                    {
                        window.paint_path(arc, usage_color(used, theme));
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// USD with cents above a dollar and more precision for small sessions.
fn format_cost(usd: f64) -> String {
    if usd >= 1. {
        format!("${usd:.2}")
    } else if usd >= 0.01 {
        format!("${usd:.3}")
    } else if usd > 0. {
        "<$0.01".into()
    } else {
        "$0.00".into()
    }
}

/// Activity-ledger source key for a per-token (API key) route, or `None` for
/// subscription routes that show quota limits instead.
fn metered_source_key(provider: Option<&str>, auth: Option<&str>) -> Option<String> {
    let provider = provider?.trim().to_ascii_lowercase();
    let api_key = auth == Some("api key");
    Some(match provider.as_str() {
        "anthropic" | "claude" if api_key => "claude:api-key".into(),
        "anthropic-api" | "claude-api" => "claude:api-key".into(),
        "openai" if api_key => "openai:api-key".into(),
        "openai-api" => "openai:api-key".into(),
        "gemini" if api_key => "gemini".into(),
        "gemini-api" => "gemini".into(),
        "openrouter" | "bedrock" | "azure" | "azure-openai" => provider,
        other => {
            let id = other.strip_prefix("openai-compatible:").unwrap_or(other);
            if !api_key && !other.starts_with("openai-compatible:") {
                return None;
            }
            if jcode_base::model_pricing::models_dev_provider_id(id).is_some() {
                id.to_string()
            } else {
                format!("openai-compatible:{id}")
            }
        }
    })
}

/// Never substitute another credential's limits (e.g. ChatGPT for an API key).
fn active_account<'a>(
    accounts: &'a [Account],
    provider: Option<&str>,
    auth: Option<&str>,
) -> Option<&'a Account> {
    let provider = provider?;
    // Runtime identity arrives asynchronously. Ambiguous auth is not OAuth.
    if auth.is_none()
        && matches!(
            provider.trim().to_ascii_lowercase().as_str(),
            "openai" | "anthropic" | "claude" | "gemini"
        )
    {
        return None;
    }
    let id = crate::accounts::credential_id(provider, auth);
    accounts.iter().find(|account| account.id == id)
}

#[cfg(test)]
fn active_limits<'a>(
    accounts: &'a [Account],
    provider: Option<&str>,
    auth: Option<&str>,
) -> Option<&'a [UsageLimit]> {
    active_account(accounts, provider, auth).and_then(Account::active_limits)
}

impl Panel {
    /// Estimated session spend on a per-token route, summed from completed
    /// response stats (live and restored). `None` on subscription routes or
    /// when the model cannot be priced.
    pub(super) fn session_api_cost(&self) -> Option<(f64, usize)> {
        let source = metered_source_key(self.provider.as_deref(), self.auth_method.as_deref())?;
        let model = self.model.as_deref()?;
        // Probe once so unpriced routes fall back to limits, not "$0.00".
        jcode_base::provider::pricing::metered_usage_cost_usd(&source, model, 0, 0, 0, 0)?;
        let mut total = 0.;
        let mut turns = 0;
        for item in &self.items {
            let Item::ResponseStats(stats) = item else {
                continue;
            };
            let cost = jcode_base::provider::pricing::metered_usage_cost_usd(
                &source,
                model,
                stats.input_tokens.unwrap_or(0),
                stats.output_tokens.unwrap_or(0),
                stats.cache_read_tokens.unwrap_or(0),
                stats.cache_creation_tokens.unwrap_or(0),
            )?;
            total += cost;
            turns += 1;
        }
        Some((total, turns))
    }

    /// Context window ring plus percentage. Sits in the composer's bottom row
    /// right after the credential method pill.
    pub(super) fn render_context_meter(&self) -> Option<gpui::AnyElement> {
        if self.model.is_none() && self.provider.is_none() {
            return None;
        }
        let theme = Theme::global();
        let window = self.model.as_deref().and_then(context_window_for_model);
        // Fill the ring toward the compaction trigger, not the raw window, so
        // the user sees when the session will fold (e.g. 500k on a 1M window).
        let window = self
            .model
            .as_deref()
            .zip(window)
            .and_then(|(model, window)| {
                super::compaction_threshold_for_model(self.provider.as_deref(), model, window)
            })
            .or(window);
        let used = self.context_tokens;
        let percent = used
            .zip(window)
            .map(|(used, window)| (used as f64 / window as f64 * 100.) as f32);
        // Unknown usage shows only the empty ring, never a placeholder dash.
        let label = percent.map(|percent| format!("{:.0}%", percent.min(100.)));
        let detail = context_usage_label(self.model.as_deref(), self.context_tokens)
            .map(|label| format!("Context window: {label}. Model capacity is an estimate. Usage reflects the latest reported request."))
            .unwrap_or_else(|| "Context usage is not reported yet.".into());
        Some(
            div()
                .id("panel-context-meter")
                .debug_selector(|| "panel-context-meter".into())
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .h(px(22.))
                .px_1()
                .text_size(px(10.5))
                .font_family(theme.FONT_MONO)
                .text_color(theme.TEXT_DIM)
                .tooltip(move |_, cx| cx.new(|_| MeterTooltip(detail.clone())).into())
                .child(context_ring(percent))
                .children(label)
                .into_any_element(),
        )
    }

    #[allow(dead_code)] // session spend no longer sits in the bar
    pub(super) fn render_usage_meters(&self) -> Option<gpui::Div> {
        if self.model.is_none() && self.provider.is_none() {
            return None;
        }
        let theme = Theme::global();
        let mut row = div()
            .debug_selector(|| "panel-usage".into())
            .flex()
            .min_w_0()
            .items_center()
            .gap_2()
            .flex_nowrap()
            .overflow_hidden();
        // Subscription quota lives in the method pill's hover card. Only
        // per-token spend stays inline.
        if let Some((cost, turns)) = self.session_api_cost() {
            let detail = format!(
                "{}: estimated API spend for this session, priced from {turns} reported response{} at list rates. Not a bill.",
                serving_account_label(
                    self.provider.as_deref(),
                    self.auth_method.as_deref(),
                    self.account_label.as_deref()
                ),
                if turns == 1 { "" } else { "s" },
            );
            row = row.child(
                div()
                    .id("panel-api-cost")
                    .debug_selector(|| "panel-api-cost".into())
                    .flex_none()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .text_color(theme.TEXT_DIM)
                    .tooltip(move |_, cx| cx.new(|_| MeterTooltip(detail.clone())).into())
                    .child(format_cost(cost)),
            );
        }
        Some(row)
    }

    /// Quota for the active subscription credential, for the method pill's
    /// hover card. `None` on per-token routes or when nothing is known.
    #[allow(dead_code)]
    pub(super) fn limits_summary(&self, cx: &App) -> Option<LimitsSummary> {
        if self.model.is_none() && self.provider.is_none() {
            return None;
        }
        let metered =
            metered_source_key(self.provider.as_deref(), self.auth_method.as_deref()).is_some();
        // Local account snapshots cannot describe credentials on a remote host.
        let account = if crate::harness::remote_host(&self.session_id).is_none() {
            cx.try_global::<StatusAccounts>().and_then(|snapshot| {
                active_account(
                    &snapshot.0,
                    self.provider.as_deref(),
                    self.auth_method.as_deref(),
                )
            })
        } else {
            None
        };
        let limits = account.and_then(Account::active_limits);
        // An API-key route that reports quota windows (OpenCode Zen / Go)
        // gives real percentages, so its hover card is not empty just because
        // the route is also priced per token.
        if let Some(limits) = limits.filter(|limits| !limits.is_empty()) {
            return Some(LimitsSummary::Limits(limits.to_vec()));
        }
        // Per-token routes without quota have no limits to show.
        if metered {
            return None;
        }
        // A subscription login whose quota fetch failed says so rather than
        // silently omitting its meters.
        account
            .and_then(Account::active_report)
            .and_then(crate::accounts::UsageReport::usage_error)
            .map(|reason| LimitsSummary::Unavailable(reason.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn quota_tooltip_uses_shared_bar_and_tui_labels(cx: &mut gpui::TestAppContext) {
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"}]}"#,
        )
        .unwrap();
        accounts[0].limits = vec![UsageLimit {
            name: "Weekly".into(),
            usage_percent: 41.,
            reset_in: Some("2h".into()),
        }];
        let chip = quota_chips(&accounts).remove(0);
        let card = quota_card(&accounts[0], &chip);
        let (_, vcx) = cx.add_window_view(|_, _| card);
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-limit-bar-0").is_some());
        let label = if jcode_base::config::config().display.usage_display_used() {
            "panel-limit-value-0-41% used"
        } else {
            "panel-limit-value-0-59% left"
        };
        assert!(vcx.debug_bounds(label).is_some());
        let theme = Theme::global();
        assert_eq!(usage_color(69., theme), gpui::rgb(0x64c864).into());
        assert_eq!(usage_color(70., theme), theme.WARN);
        assert_eq!(usage_color(89., theme), theme.WARN);
        assert_eq!(usage_color(90., theme), theme.ERROR);
    }

    #[test]
    fn quota_card_tracks_the_active_login_and_banked_resets() {
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"}]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[{"provider_name":"Anthropic (Claude)","limits":[{"name":"7-day window","usage_percent":41.0,"reset_in":"1d 11h"}],"extra_info":[["Account label","claude-otter"]],"banked_reset":{"provider":"claude","account_label":"claude-otter","available_count":2,"usable_count":1,"cached_source":"OMP","expires_at":"2099-01-01T00:00:00Z","limit_reached":false}}]}"#,
        );
        let chip = quota_chips(&accounts).remove(0);
        let card = quota_card(&accounts[0], &chip);
        assert_eq!(card.method, "Anthropic · OAuth · claude-otter");
        assert!(
            card.notes.iter().any(|note| note.contains("1 usable / 2")
                && note.contains("expires")
                && note.contains("cached OMP")),
            "{:?}",
            card.notes
        );
        let LimitsSummary::Limits(limits) = card.summary else {
            panic!("quota windows missing")
        };
        assert_eq!(limits[0].name, "7-day window");
        assert_eq!(limits[0].usage_percent, 41.0);
        assert_eq!(limits[0].reset_in.as_deref(), Some("1d 11h"));
    }

    #[test]
    fn footer_chips_show_tightest_window_and_tooltip_lists_all() {
        let mut accounts = crate::accounts::parse(r#"{"providers":[{"id":"claude","status":"available"},{"id":"openai","status":"available"}]}"#).unwrap();
        accounts[0].limits = vec![
            UsageLimit {
                name: "5-hour window".into(),
                usage_percent: 0.,
                reset_in: Some("4h".into()),
            },
            UsageLimit {
                name: "7-day window".into(),
                usage_percent: 28.,
                reset_in: Some("1d 18h".into()),
            },
        ];
        accounts[1].limits.push(UsageLimit {
            name: "7-day window".into(),
            usage_percent: 99.,
            reset_in: Some("7h".into()),
        });
        let chips = quota_chips(&accounts);
        let labels: Vec<_> = chips.iter().map(|c| (c.logo, c.label())).collect();
        assert_eq!(
            labels,
            vec![("claude", "28%".into()), ("openai", "99%".into())]
        );
        let LimitsSummary::Limits(windows) = quota_card(&accounts[0], &chips[0]).summary else {
            panic!("missing quota windows")
        };
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].reset_in.as_deref(), Some("4h"));
        assert_eq!(windows[1].reset_in.as_deref(), Some("1d 18h"));
        // Unreported windows are never fabricated.
        accounts[1].limits[0].usage_percent = f32::NAN;
        assert_eq!(quota_chips(&accounts).len(), 1);
    }

    #[gpui::test]
    fn context_meter_uses_prompt_occupancy_not_billing_totals(cx: &mut gpui::TestAppContext) {
        let (_, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("usage-session", cx);
            workspace.test_panel(0).unwrap().update(cx, |panel, cx| {
                for (provider, input, read, write, expected) in [
                    ("openai", 100_000, Some(28_000), None, 100_000),
                    ("anthropic", 10_000, Some(80_000), Some(5_000), 95_000),
                    ("openai", 5_000, None, None, 5_000),
                ] {
                    panel.provider = Some(provider.into());
                    panel.apply(
                        &ApiEvent::TokenUsage {
                            session_id: "usage-session".into(),
                            input,
                            output: 8_000,
                            cache_read_input: read,
                            cache_creation_input: write,
                            effective_budget: None,
                        },
                        cx,
                    );
                    assert_eq!(panel.context_tokens, Some(expected));
                }
            });
            workspace
        });
        vcx.run_until_parked();
    }

    #[test]
    fn footer_chips_show_opencode_go_windows_and_money_when_no_quota() {
        let mut accounts = crate::accounts::parse(
            r#"{"providers":[
                {"id":"opencode-go","status":"available","auth_kind":"API key"},
                {"id":"openrouter","status":"available","auth_kind":"API key"},
                {"id":"deepseek","status":"available","auth_kind":"API key"}
            ]}"#,
        )
        .unwrap();
        crate::accounts::merge_usage_for_tests(
            &mut accounts,
            r#"{"providers":[
                {"provider_name":"OpenCode Go (API key)","limits":[
                    {"name":"Rolling window","usage_percent":23.0,"reset_in":"4h"},
                    {"name":"Weekly window","usage_percent":9.0,"reset_in":"1d"},
                    {"name":"Monthly window","usage_percent":4.0,"reset_in":"30d"}
                ],"extra_info":[],"error":null},
                {"provider_name":"OpenRouter","limits":[],
                 "extra_info":[["Balance","$12.34 / $20.00"],["Today","$0.42"]],"error":null}
            ]}"#,
        );
        let chips = quota_chips(&accounts);
        let labels: Vec<(&'static str, String)> =
            chips.iter().map(|chip| (chip.logo, chip.label())).collect();
        assert_eq!(
            labels,
            vec![
                ("opencode-go", "23%".to_string()),
                ("openrouter", "$12.34".to_string())
            ]
        );
        let LimitsSummary::Limits(windows) = quota_card(&accounts[0], &chips[0]).summary else {
            panic!("missing quota windows")
        };
        assert_eq!(windows[0].name, "Rolling window");
        assert_eq!(windows[0].usage_percent, 23.0);
        assert_eq!(windows[2].name, "Monthly window");
        assert_eq!(windows[2].reset_in.as_deref(), Some("30d"));
        // A provider whose usage was never reported paints no bare logo pill.
        assert!(chips.iter().all(|chip| chip.logo != "deepseek"));
    }

    #[gpui::test]
    fn opencode_go_quota_pill_paints_in_the_composer(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut accounts = crate::accounts::parse(
                r#"{"providers":[{"id":"opencode-go","status":"available","auth_kind":"API key","method":"API key (`OPENCODE_GO_API_KEY`)"}]}"#,
            )
            .unwrap();
            crate::accounts::merge_usage_for_tests(
                &mut accounts,
                r#"{"providers":[{"provider_name":"OpenCode Go (API key)","limits":[
                    {"name":"Rolling window","usage_percent":23.0,"reset_in":"4h"},
                    {"name":"Weekly window","usage_percent":9.0,"reset_in":"1d"}
                ],"extra_info":[],"error":null}]}"#,
            );
            cx.set_global(StatusAccounts(accounts));
        });
        let (_workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("opencode-go-session", cx);
            workspace.test_panel(0).unwrap().update(cx, |panel, _| {
                panel.provider = Some("opencode-go".into());
                panel.auth_method = Some("api key".into());
                panel.model = Some("kimi-k2.5".into());
            });
            workspace
        });
        vcx.run_until_parked();
        // Quota moved to the sidebar's bottom block; the composer bar paints
        // no provider figure at all now.
        assert!(vcx.debug_bounds("panel-quota-opencode-go").is_none());
        let chips = vcx
            .update(|_, cx| quota_chips(&cx.try_global::<StatusAccounts>().expect("accounts").0));
        let chip = chips
            .iter()
            .find(|chip| chip.logo == "opencode-go")
            .expect("OpenCode Go reports a quota window");
        assert_eq!(chip.value, "23%");
    }

    #[test]
    fn active_limits_match_credentials_not_model_family() {
        let mut accounts = crate::accounts::parse(r#"{"providers":[{"id":"openai","status":"available"},{"id":"openai-api","status":"available"},{"id":"openrouter","status":"available"}]}"#).unwrap();
        accounts[0].limits.push(UsageLimit {
            name: "5 hour".into(),
            usage_percent: 25.,
            reset_in: Some("2h".into()),
        });
        assert_eq!(
            active_limits(&accounts, Some("openai"), Some("oauth"))
                .unwrap()
                .len(),
            1
        );
        assert!(
            active_limits(&accounts, Some("openai"), Some("api key"))
                .unwrap()
                .is_empty()
        );
        assert!(
            active_limits(&accounts, Some("openrouter"), Some("api key"))
                .unwrap()
                .is_empty()
        );
        assert!(active_limits(&accounts, Some("openai"), None).is_none());
        assert!(active_limits(&accounts, None, None).is_none());
    }

    #[gpui::test]
    fn status_rows_wrap_with_bounded_height_and_visible_controls(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("fixed-status-row", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [1440., 640., 400., 320.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            for populated in [false, true] {
                for status in [
                    "idle",
                    "running_tools",
                    "disconnected with a long status message",
                ] {
                    panel.update(vcx, |panel, cx| {
                        panel.model = populated.then(|| "a-very-long-model-name".repeat(4));
                        panel.provider = populated.then(|| "openai".into());
                        panel.auth_method = populated.then(|| "oauth".into());
                        panel.working_dir = Some("/a/very/long/working/directory".repeat(4));
                        panel.reasoning_effort = Some("high".into());
                        panel.status = status.into();
                        cx.notify();
                    });
                    vcx.run_until_parked();
                    let row = vcx.debug_bounds("panel-meta").expect("status row");
                    // Three bounded groups may wrap, but long metadata/status
                    // text must never create unbounded footer height.
                    assert!(
                        row.size.height >= px(22.) && row.size.height <= px(90.),
                        "width={width}, status={status}: {row:?}"
                    );
                    for selector in ["panel-build", "panel-status", "panel-usage"] {
                        if let Some(child) = vcx.debug_bounds(selector) {
                            assert!(child.top() >= row.top(), "{selector}: {child:?}");
                            assert!(child.bottom() <= row.bottom(), "{selector}: {child:?}");
                        }
                    }
                }
            }
        }
    }

    #[gpui::test]
    fn footer_meters_paint_and_follow_credential_switches(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut accounts = crate::accounts::parse(r#"{"providers":[{"id":"openai","status":"available"},{"id":"openai-api","status":"available"}]}"#).unwrap();
            accounts[0].limits = vec![
                UsageLimit { name: "5-hour window".into(), usage_percent: 25., reset_in: Some("2h".into()) },
                UsageLimit { name: "7-day window".into(), usage_percent: 75., reset_in: Some("3d".into()) },
            ];
            cx.set_global(StatusAccounts(accounts));
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("usage-session", cx);
            let panel = workspace.test_panel(0).unwrap();
            panel.update(cx, |panel, _| {
                panel.provider = Some("openai".into());
                panel.auth_method = Some("oauth".into());
                panel.model = Some("gpt-5.6-sol".into());
                panel.context_tokens = Some(100_000);
            });
            workspace
        });
        vcx.run_until_parked();
        let bounds = vcx.debug_bounds("panel-context-meter").unwrap();
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        // Quota bars live in the method pill's hover card, not the pill row.
        assert!(vcx.debug_bounds("panel-limit-0").is_none());
        let summary = workspace.update(vcx, |workspace, cx| {
            workspace.test_panel(0).unwrap().read(cx).limits_summary(cx)
        });
        let Some(LimitsSummary::Limits(limits)) = summary else {
            panic!("expected limits, got {summary:?}");
        };
        assert_eq!(limits.len(), 2);
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(640.), px(480.)));
        vcx.run_until_parked();
        let context = vcx.debug_bounds("panel-context-meter").unwrap();
        // Quota moved to the sidebar's bottom block; the bar keeps the ring
        // and no provider figure.
        assert!(vcx.debug_bounds("panel-provider-usage").is_none());
        assert!(vcx.debug_bounds("panel-login").is_none());
        assert!(vcx.debug_bounds("panel-model").unwrap().size.width > px(20.));
        assert!(context.size.width > px(0.));
        workspace.update(vcx, |workspace, cx| {
            workspace.test_panel(0).unwrap().update(cx, |panel, cx| {
                panel.auth_method = Some("api key".into());
                // No catalog (static or cached) prices this id.
                panel.model = Some("unpriced-desktop-test-model".into());
                cx.notify();
            });
        });
        vcx.run_until_parked();
        assert_eq!(
            workspace.update(vcx, |workspace, cx| {
                workspace.test_panel(0).unwrap().read(cx).limits_summary(cx)
            }),
            None
        );
        // Unpriced API routes stay honest: no fake $0 and no "Limits" label.
        assert!(vcx.debug_bounds("panel-limits-unavailable").is_none());
        assert!(vcx.debug_bounds("panel-api-cost").is_none());
        // Priced API routes show session spend instead of quota limits.
        workspace.update(vcx, |workspace, cx| {
            workspace.test_panel(0).unwrap().update(cx, |panel, cx| {
                panel.provider = Some("anthropic".into());
                panel.model = Some("claude-sonnet-4-5".into());
                *panel.items = vec![Item::ResponseStats(response_stats::ResponseStats {
                    input_tokens: Some(1_000_000),
                    output_tokens: Some(100_000),
                    ..Default::default()
                })];
                assert_eq!(panel.session_api_cost(), Some((4.5, 1)));
                cx.notify();
            });
        });
        vcx.run_until_parked();
        // Session spend reads in the accounts page; the bar keeps identity and
        // session state only.
        assert!(vcx.debug_bounds("panel-api-cost").is_none());
        assert!(vcx.debug_bounds("panel-limits-unavailable").is_none());
        assert!(vcx.debug_bounds("panel-context-ring").is_some());
    }

    #[gpui::test]
    fn claude_oauth_limits_paint_with_daemon_display_name(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut accounts = crate::accounts::parse(
                r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"},{"id":"anthropic-api","status":"available","auth_kind":"API key"}]}"#,
            )
            .unwrap();
            crate::accounts::merge_usage_for_tests(
                &mut accounts,
                r#"{"providers":[{"provider_name":"Anthropic (Claude) (j***5@gmail.com)","limits":[{"name":"5-hour window","usage_percent":1.0,"reset_in":"3h 56m"},{"name":"7-day window","usage_percent":15.0,"reset_in":"1d 13h"}],"extra_info":[],"error":null}]}"#,
            );
            cx.set_global(StatusAccounts(accounts));
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("claude-session", cx);
            workspace.test_panel(0).unwrap().update(cx, |panel, _| {
                // The daemon reports `Claude`, not the canonical `claude` id.
                panel.provider = Some("Claude".into());
                panel.auth_method = Some("oauth".into());
                panel.model = Some("claude-opus-4-5".into());
            });
            workspace
        });
        vcx.run_until_parked();
        let summary = workspace.update(vcx, |workspace, cx| {
            workspace.test_panel(0).unwrap().read(cx).limits_summary(cx)
        });
        assert!(matches!(summary, Some(LimitsSummary::Limits(ref limits)) if limits.len() == 2));
    }

    #[gpui::test]
    fn throttled_claude_oauth_quota_says_limits_are_unavailable(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let mut accounts = crate::accounts::parse(
                r#"{"providers":[{"id":"claude","status":"available","auth_kind":"OAuth"}]}"#,
            )
            .unwrap();
            crate::accounts::merge_usage_for_tests(
                &mut accounts,
                r#"{"providers":[{"provider_name":"Anthropic (Claude) (j***5@gmail.com)","limits":[],"error":"Usage API error (429 Too Many Requests): {}"}]}"#,
            );
            cx.set_global(StatusAccounts(accounts));
        });
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("claude-session", cx);
            workspace.test_panel(0).unwrap().update(cx, |panel, _| {
                panel.provider = Some("anthropic".into());
                panel.auth_method = Some("oauth".into());
                panel.model = Some("claude-opus-4-5".into());
            });
            workspace
        });
        vcx.run_until_parked();
        let summary = workspace.update(vcx, |workspace, cx| {
            workspace.test_panel(0).unwrap().read(cx).limits_summary(cx)
        });
        assert!(
            matches!(summary, Some(LimitsSummary::Unavailable(_))),
            "{summary:?}"
        );
    }

    #[test]
    fn metered_source_keys_only_for_per_token_routes() {
        assert_eq!(
            metered_source_key(Some("anthropic"), Some("api key")).as_deref(),
            Some("claude:api-key")
        );
        assert_eq!(
            metered_source_key(Some("openai"), Some("api key")).as_deref(),
            Some("openai:api-key")
        );
        assert_eq!(
            metered_source_key(Some("openrouter"), None).as_deref(),
            Some("openrouter")
        );
        assert_eq!(metered_source_key(Some("anthropic"), Some("oauth")), None);
        assert_eq!(metered_source_key(Some("openai"), Some("oauth")), None);
        assert_eq!(metered_source_key(Some("copilot"), None), None);
        assert_eq!(metered_source_key(None, Some("api key")), None);
    }

    #[test]
    fn cost_labels_and_ring_paths_are_bounded() {
        assert_eq!(format_cost(0.), "$0.00");
        assert_eq!(format_cost(0.004), "<$0.01");
        assert_eq!(format_cost(0.123), "$0.123");
        assert_eq!(format_cost(12.345), "$12.35");
        let center = gpui::point(px(10.), px(10.));
        assert!(ring_path(center, 6., 3., 0.).is_none());
        for fraction in [0.01, 0.25, 0.5, 0.75, 0.999, 1.0, 2.0] {
            assert!(ring_path(center, 6., 3., fraction).is_some(), "{fraction}");
        }
    }
}
