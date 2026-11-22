//! The composer pill's prompt-cache hover card.
use super::{cache_miss::CacheMissNotice, cache_timer, usage, *};
use jcode_desktop_model::prompt_cache::{Tokens, Usage};
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub(super) struct CacheTelemetry {
    usage: Usage,
    route: Option<(String, String)>,
    touches: BTreeMap<(String, String), (i64, u64)>,
    misses: Misses,
}

impl CacheTelemetry {
    pub(super) fn observe(&mut self, event: &ApiEvent) {
        self.observe_at(event, cache_timer::now_ms());
    }

    fn observe_at(&mut self, event: &ApiEvent, at_ms: i64) {
        if let ApiEvent::RequestUsage {
            request_id,
            provider,
            model,
            input,
            cache_read_input,
            cache_creation_input,
            ..
        } = event
            && self.usage.record(
                request_id,
                tokens(provider, *input, *cache_read_input, *cache_creation_input),
            )
        {
            let route = (provider.clone(), model.clone());
            if let Some(tokens) = cache_read_input
                .filter(|read| *read > 0)
                .or(cache_creation_input.filter(|written| *written > 0))
            {
                self.touches.insert(route.clone(), (at_ms, tokens));
            }
            self.route = Some(route);
        }
    }

    pub(super) fn has_requests(&self) -> bool {
        self.usage.count() != 0
    }

    pub(super) fn last_hit_rate(&self) -> Option<f32> {
        self.usage.last.and_then(Tokens::percent)
    }

    pub(super) fn miss(&mut self, notice: &CacheMissNotice) {
        self.misses.record(notice);
    }

    fn retention(&self) -> Option<cache_timer::CacheFacts> {
        let route = self.route.as_ref()?;
        let (provider, model) = route;
        let (at_ms, tokens) = *self.touches.get(route)?;
        Some(cache_timer::facts(
            jcode_base::provider::cache_ttl_for_provider_model(provider, Some(model))?,
            jcode_base::provider::cache_ttl_is_estimate(provider),
            at_ms,
            cache_timer::now_ms(),
            Some(tokens),
        ))
    }
}

impl Panel {
    pub(crate) fn apply_cache_card_fixture(&mut self, cx: &mut Context<Self>) {
        let provider = self.provider.clone().unwrap_or_else(|| "anthropic".into());
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| "claude-opus-5-5".into());
        let session_id = self.session_id.clone();
        for (i, (read, written, uncached, miss)) in
            std::iter::repeat_n((23_500, 310, 330, None), 210)
                .chain([
                    (19_000, 74_000, 5_000, Some((74_000, "expired", false))),
                    (23_000, 32_000, 4_500, Some((32_000, "model_switch", false))),
                    (
                        32_000,
                        306_000,
                        8_600,
                        Some((306_000, "prefix_changed", true)),
                    ),
                    (92_000, 12_000, 7_000, None),
                ])
                .enumerate()
        {
            self.apply(
                &ApiEvent::RequestUsage {
                    session_id: session_id.clone(),
                    request_id: format!("cache-fixture-{i}"),
                    provider: provider.clone(),
                    model: model.clone(),
                    input: Some(uncached),
                    output: Some(95),
                    cache_read_input: Some(read),
                    cache_creation_input: Some(written),
                },
                cx,
            );
            if let Some((missed_tokens, reason, harness_caused)) = miss {
                self.apply(
                    &ApiEvent::KvCacheMiss {
                        session_id: session_id.clone(),
                        reason: reason.into(),
                        harness_caused,
                        missed_tokens,
                        read_tokens: read,
                        expected_tokens: read + missed_tokens,
                        documented_cause: None,
                        message: format!(
                            "{} prompt tokens re-billed ({})",
                            amount(Some(missed_tokens)),
                            reason.replace('_', " ")
                        ),
                    },
                    cx,
                );
            }
        }
        if let Some(touch) = self.cache_telemetry.touches.get_mut(&(provider, model)) {
            touch.0 = cache_timer::now_ms() - 12 * 60_000;
        }
    }
}

#[derive(Default)]
struct Misses {
    count: u64,
    tokens: u64,
    recent: VecDeque<CacheMissNotice>,
}

impl Misses {
    fn record(&mut self, notice: &CacheMissNotice) {
        self.count = self.count.saturating_add(1);
        self.tokens = self.tokens.saturating_add(notice.missed_tokens);
        self.recent.push_front(notice.clone());
        self.recent.truncate(3);
    }
}

fn tokens(provider: &str, input: Option<u64>, read: Option<u64>, write: Option<u64>) -> Tokens {
    let prompt = input.and_then(|input| {
        let effective = jcode_base::compaction::effective_context_tokens_from_usage(
            provider, input, read, write,
        );
        // The shared context estimate treats unknown writes as zero. A cache
        // ratio cannot do that when writes belong in the denominator.
        let provider = provider.to_ascii_lowercase();
        let split = effective > input
            || (!(provider.contains("openai") || provider.contains("codex"))
                && (provider.contains("anthropic") || provider.contains("claude")));
        (!split || write.is_some()).then_some(effective)
    });
    Tokens::new(prompt, read, write)
}

pub(super) struct CacheTooltip {
    pub(super) panel: gpui::WeakEntity<Panel>,
    pub(super) pill: gpui::WeakEntity<cache_timer::CachePill>,
}

impl Render for CacheTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(panel) = self.panel.upgrade() else {
            return div();
        };
        let panel = panel.read(cx);
        let telemetry = &panel.cache_telemetry;
        let (provider, model) = telemetry.route.as_ref().map_or(
            (panel.provider.as_deref(), panel.model.as_deref()),
            |(provider, model)| (Some(provider.as_str()), Some(model.as_str())),
        );
        let title = [Some("Prompt cache"), provider, model]
            .into_iter()
            .flatten()
            .map(one_line)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let theme = Theme::global();
        usage::meter_card(theme)
            .debug_selector(|| "panel-cache-tooltip".into())
            .w(px(312.))
            .child(
                div()
                    .truncate()
                    .text_color(theme.TEXT)
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .children(self.pill.upgrade().map(|pill| {
                usage::meter_detail(pill.read(cx).caption().into(), theme)
                    .debug_selector(|| "panel-cache-footer-metric".into())
            }))
            .child(usage_row(
                "session",
                "Session",
                telemetry.usage.session,
                telemetry.usage.count(),
                theme,
            ))
            .child(usage_row(
                "last-request",
                "Last request",
                telemetry.usage.last,
                usize::from(telemetry.has_requests()),
                theme,
            ))
            .child(retention_row(
                if telemetry.has_requests() {
                    telemetry.retention()
                } else {
                    panel.cache_facts()
                },
                theme,
            ))
            .child(render_misses(&telemetry.misses, theme))
            .child(usage::meter_detail(
                "Requests and misses observed in this panel".into(),
                theme,
            ))
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

const NOT_REPORTED: &str = "not reported";

fn meter_ids(id: &str, value: &str) -> usage::MeterIds {
    usage::MeterIds {
        row: format!("panel-cache-{id}"),
        value: format!("panel-cache-{id}-value-{value}"),
        bar: format!("panel-cache-{id}-bar"),
    }
}

fn usage_row(
    id: &str,
    name: &str,
    tokens: Option<Tokens>,
    count: usize,
    theme: &Theme,
) -> gpui::Div {
    let pct = tokens.and_then(Tokens::percent);
    let value = pct.map_or_else(|| NOT_REPORTED.into(), |pct| format!("{pct:.0}% cached"));
    let details = tokens.map(|tokens| {
        let (remainder, label) = if tokens.written.is_none() {
            (tokens.not_read(), "not read")
        } else {
            (tokens.uncached(), "uncached")
        };
        format!(
            "{} read · {} written · {} {label} · {count} {}",
            amount(tokens.read),
            amount(tokens.written),
            amount(remainder),
            if count == 1 { "request" } else { "requests" },
        )
    });
    usage::meter_row(
        meter_ids(id, &value),
        name.into(),
        value,
        pct.map(|pct| (pct, cache_color(pct, theme))),
        details,
        theme,
    )
}

fn amount(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "–".into();
    };
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1e6)
    } else if value >= 1_000 {
        format!("{:.0}K", value as f64 / 1e3)
    } else {
        value.to_string()
    }
}

fn cache_color(percent: f32, theme: &Theme) -> gpui::Rgba {
    // Thresholds match the TUI's kv_cache_optimal_color, with Desktop theme tones.
    match percent.round() as u8 {
        0..=24 => theme.ERROR,
        25..=59 => theme.WARN,
        60..=84 => gpui::rgb(0x6e9dd8).into(),
        _ => gpui::rgb(0x58a574).into(),
    }
}

fn retention_row(facts: Option<cache_timer::CacheFacts>, theme: &Theme) -> gpui::Div {
    let (value, bar, detail) = facts.map_or_else(
        || (NOT_REPORTED.into(), None, None),
        |facts| {
            let estimate = if facts.is_estimate {
                " · estimate"
            } else {
                ""
            };
            let (value, detail) = if facts.is_cold() {
                let expiry =
                    cache_timer::span(facts.written_ago_secs.saturating_sub(facts.ttl_secs));
                ("cold".into(), format!("Expired {expiry} ago{estimate}"))
            } else {
                (
                    format!(
                        "{} left of {}",
                        cache_timer::label(facts.remaining_secs),
                        cache_timer::span(facts.ttl_secs)
                    ),
                    format!(
                        "Last use {} ago{estimate}",
                        cache_timer::span(facts.written_ago_secs)
                    ),
                )
            };
            let percent = facts.remaining_secs as f32 * 100.0 / facts.ttl_secs.max(1) as f32;
            let color = if facts.expiring_soon() {
                theme.WARN
            } else {
                cache_color(100.0, theme)
            };
            (value, Some((percent, color)), Some(detail))
        },
    );
    usage::meter_row(
        meter_ids("retention", &value),
        "Retention".into(),
        value,
        bar,
        detail,
        theme,
    )
}

fn render_misses(misses: &Misses, theme: &Theme) -> gpui::Div {
    let (summary, color) = if misses.count == 0 {
        ("no misses".into(), cache_color(100.0, theme))
    } else {
        let noun = if misses.count == 1 { "miss" } else { "misses" };
        (
            format!(
                "{} {noun} · {} re-billed",
                misses.count,
                amount(Some(misses.tokens))
            ),
            theme.TEXT,
        )
    };
    div()
        .debug_selector(|| "panel-cache-misses".into())
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.TEXT_DIM)
                        .child("Misses"),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(color)
                        .font_family(theme.FONT_MONO)
                        .child(summary),
                ),
        )
        .children(misses.recent.iter().enumerate().map(|(index, miss)| {
            let reason = miss.reason.replace('_', " ");
            usage::meter_detail(
                format!(
                    "{} · {}",
                    amount(Some(miss.missed_tokens)),
                    one_line(&reason)
                ),
                theme,
            )
            .debug_selector(move || format!("panel-cache-miss-{index}"))
            .text_color(if miss.alarming() {
                theme.WARN
            } else {
                theme.TEXT_DIM
            })
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_accounting_produces_the_same_normalized_ratio() {
        for (provider, input) in [("anthropic", 20), ("openai", 100)] {
            let totals = tokens(provider, Some(input), Some(70), Some(10));
            assert_eq!(totals.percent(), Some(70.0));
            assert_eq!(totals.uncached(), Some(20));
        }
    }

    #[test]
    fn missing_writes_only_hide_ratios_whose_denominator_needs_them() {
        let split = tokens("anthropic", Some(20), Some(70), None);
        assert_eq!(split.percent(), None);
        assert_eq!(split.read, Some(70));
        assert_eq!(tokens("anthropic", Some(20), Some(0), None).percent(), None);
        let inclusive = tokens("openai", Some(100), Some(70), None);
        assert_eq!(inclusive.percent(), Some(70.0));
        assert_eq!(inclusive.not_read(), Some(30));
        assert_eq!(inclusive.uncached(), None);
    }

    #[test]
    fn misses_keep_all_counts_but_only_the_three_newest_attributions() {
        let mut misses = Misses::default();
        assert_eq!(misses.count, 0);
        for n in 1..=4 {
            misses.record(&CacheMissNotice {
                reason: "prefix_changed".into(),
                harness_caused: true,
                documented_cause: (n != 4).then(|| "reload".into()),
                missed_tokens: n * 1000,
                message: String::new(),
            });
        }
        assert_eq!(misses.count, 4);
        assert_eq!(misses.tokens, 10_000);
        assert_eq!(
            misses
                .recent
                .iter()
                .map(|miss| miss.missed_tokens)
                .collect::<Vec<_>>(),
            [4000, 3000, 2000]
        );
        assert!(misses.recent[0].alarming());
        assert!(!misses.recent[1].alarming());
    }

    #[gpui::test]
    fn composer_cache_pill_hover_paints_the_completed_request_breakdown(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("cache-card", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        panel.update(vcx, |panel, cx| {
            panel.apply_bar_fixture("high", cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-cache-tooltip").is_none());
        let position = vcx
            .debug_bounds("panel-cache")
            .expect("cache pill")
            .center();
        vcx.simulate_mouse_move(position, None, Default::default());
        vcx.run_until_parked();
        vcx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        vcx.run_until_parked();
        for selector in [
            "panel-cache-session-value-not reported",
            "panel-cache-last-request-value-not reported",
        ] {
            assert!(
                vcx.debug_bounds(selector).is_some(),
                "restored usage is not request telemetry: {selector}"
            );
        }
        assert!(vcx.debug_bounds("panel-cache-session-bar").is_none());

        panel.update(vcx, |panel, cx| panel.apply_cache_card_fixture(cx));
        vcx.run_until_parked();
        for selector in [
            "panel-cache-tooltip",
            "panel-cache-footer-metric",
            "panel-cache-session-value-90% cached",
            "panel-cache-session-bar",
            "panel-cache-last-request-value-83% cached",
            "panel-cache-last-request-bar",
            "panel-cache-retention-bar",
            "panel-cache-misses",
            "panel-cache-miss-0",
            "panel-cache-miss-1",
            "panel-cache-miss-2",
        ] {
            assert!(vcx.debug_bounds(selector).is_some(), "missing {selector}");
        }
    }
}
