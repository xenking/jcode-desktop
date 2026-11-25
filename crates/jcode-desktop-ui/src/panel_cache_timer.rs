//! Prompt-cache status for the composer bar: a short retention countdown or
//! the latest completed request's cache hit rate.
use super::live::Live;
use super::*;

#[path = "panel_cache_card.rs"]
pub(super) mod card;

use jcode_desktop_model::prompt_cache::{CacheFacts, SHORT_COUNTDOWN_MAX_SECS};
use std::time::{SystemTime, UNIX_EPOCH};

/// Once the countdown is visible, only the leaf view repaints.
const TICK: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CachePillValue {
    short_countdown: bool,
    retention: Option<CacheFacts>,
    hit_rate: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CacheMetric {
    Countdown(Option<CacheFacts>),
    HitRate(Option<f32>),
}

impl CachePillValue {
    fn metric(self) -> CacheMetric {
        if self.short_countdown || self.retention.is_some_and(|facts| facts.expiring_soon()) {
            CacheMetric::Countdown(self.retention)
        } else {
            CacheMetric::HitRate(self.hit_rate)
        }
    }

    fn next_refresh(self) -> Option<Duration> {
        let facts = self.retention?;
        match self.metric() {
            CacheMetric::Countdown(_) if !facts.is_cold() => {
                let until_change = facts
                    .secs_until_warning()
                    .filter(|&secs| secs != 0)
                    .unwrap_or(facts.remaining_secs);
                Some(TICK.min(Duration::from_secs(until_change)))
            }
            CacheMetric::Countdown(_) => None,
            CacheMetric::HitRate(_) => facts.secs_until_warning().map(Duration::from_secs),
        }
    }
}

impl CacheMetric {
    fn label(self) -> String {
        match self {
            Self::Countdown(Some(facts)) if facts.expiring_soon() => {
                format!("{} left", label(facts.remaining_secs))
            }
            Self::Countdown(Some(facts)) => label(facts.remaining_secs),
            Self::HitRate(Some(percent)) => format!("{percent:.0}%"),
            Self::Countdown(None) | Self::HitRate(None) => "–".into(),
        }
    }
}

/// `47m`, `40s`, `cold`. Minutes past the first minute, so a long window
/// reads as one number instead of a clock face. The word "cache" is the drawn
/// timer (snowflake once cold) in front of it, never text.
pub(crate) fn label(remaining_secs: u64) -> String {
    match remaining_secs {
        0 => "cold".into(),
        secs if secs >= 60 => format!("{}m", secs.div_ceil(60)),
        secs => format!("{secs}s"),
    }
}

/// `1h`, `5m`, `47m`: compact spans for the hover text.
pub(super) fn span(secs: u64) -> String {
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

impl Panel {
    /// The newest usage event reporting a positive cache read or write. Its
    /// timestamp estimates when the request refreshed the provider's window.
    fn last_cache_touch(&self) -> Option<(i64, Option<u64>)> {
        self.response_stats.cache_touch().or_else(|| {
            self.items.iter().rev().find_map(|item| match item {
                Item::ResponseStats(stats) => {
                    let tokens = stats
                        .cache_read_tokens
                        .filter(|tokens| *tokens > 0)
                        .or(stats.cache_creation_tokens.filter(|tokens| *tokens > 0))?;
                    stats
                        .cache_at_ms
                        .or(stats.at_ms)
                        .map(|at| (at, Some(tokens)))
                }
                _ => None,
            })
        })
    }

    fn cache_retention(&self) -> Option<(&str, u64)> {
        let provider = self
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())?;
        let ttl =
            jcode_base::provider::cache_ttl_for_provider_model(provider, self.model.as_deref())?;
        Some((provider, ttl))
    }

    /// Match the popover's latest request route once telemetry is available.
    /// Without it, restored turns and in-flight usage can still seed retention.
    pub(super) fn cache_facts(&self) -> Option<CacheFacts> {
        if self.cache_telemetry.has_requests() {
            let on_route = self
                .cache_telemetry
                .is_on_route(self.provider.as_deref(), self.model.as_deref());
            let live = on_route.then(|| self.response_stats.cache_touch()).flatten();
            return self.cache_telemetry.retention(live);
        }
        let (provider, ttl_secs) = self.cache_retention()?;
        let latest = self.last_cache_touch();
        if let Some(facts) = self.cache_telemetry.keepalive_retention(
            provider,
            self.model.as_deref(),
            latest.map(|(at, _)| at),
        ) {
            return Some(facts);
        }
        let (touched_ms, cached_tokens) = latest?;
        Some(CacheFacts::new(
            ttl_secs,
            jcode_base::provider::cache_ttl_is_estimate(provider),
            touched_ms,
            now_ms(),
            cached_tokens,
        ))
    }

    fn cache_pill_value(&self) -> Option<CachePillValue> {
        let retention = self.cache_facts();
        let ttl = retention.map(|facts| facts.ttl_secs);
        let known_ttl = ttl.or_else(|| self.cache_retention().map(|(_, ttl)| ttl));
        (known_ttl.is_some() || self.cache_telemetry.has_requests()).then(|| CachePillValue {
            short_countdown: known_ttl.is_some_and(|ttl| ttl <= SHORT_COUNTDOWN_MAX_SECS),
            retention,
            hit_rate: self.cache_telemetry.last_hit_rate(),
        })
    }

    /// What this panel's cache pill and hover card read of it.
    fn cache_source(panel: &Entity<Self>) -> CacheSource {
        CacheSource::new(
            Live::of(panel, |panel: &Panel, _| panel.session_id.clone()),
            Live::of(panel, |panel: &Panel, _| panel.cache_view()),
        )
    }

    pub(super) fn render_cache_pill(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let value = self.cache_pill_value()?;
        let source = Self::cache_source(&cx.entity());
        Some(self.swarm_marks.cache_pill(value, source, cx))
    }
}

impl Panel {
    /// Offline screenshot fixture (`JCODE_DESKTOP_SCREENSHOT_BAR=<effort>`):
    /// a cache-capable route, a level, a context reading and a response that
    /// touched the cache 12 minutes ago, so the bar shows every drawn mark.
    pub(crate) fn apply_bar_fixture(&mut self, effort: &str, cx: &mut Context<Self>) {
        self.provider = Some("anthropic".into());
        self.model = Some("claude-opus-5-5".into());
        self.reasoning_effort = Some(effort.into());
        self.context_tokens = Some(116_000);
        // The fixture panel is narrow; the location would push the status
        // marks out of the row, and it is not what this review is about.
        self.working_dir = None;
        // The coordinator is parked on its running workers, so the swarm
        // pill shows the wait mark beside the live count.
        let running: Vec<String> = self
            .swarm_workers
            .iter()
            .filter(|worker| worker.is_live())
            .map(|worker| worker.session_id.clone())
            .collect();
        if !running.is_empty() {
            self.pending_awaits = vec![jcode_sdk::PendingAwait {
                requested_ids: running,
                deadline_unix_ms: u64::MAX,
            }];
        }
        self.items
            .push(Item::ResponseStats(super::response_stats::ResponseStats {
                at_ms: Some(now_ms() - 12 * 60_000),
                cache_read_tokens: Some(18_200),
                ..Default::default()
            }));
        // `JCODE_DESKTOP_SCREENSHOT_EFFORT_MENU=1` also opens the `/effort`
        // picker so its drawn rows can be reviewed offline.
        if std::env::var("JCODE_DESKTOP_SCREENSHOT_EFFORT_MENU").as_deref() == Ok("1") {
            self.open_picker_search(PickerKind::Effort, cx);
        }
        self.apply_speed_picker_fixture(cx);
        cx.notify();
    }
}

/// Tone for the countdown: quiet while the window is open, warm when it is
/// nearly over, faint once it has lapsed.
pub(super) fn tone(facts: &CacheFacts, theme: &Palette) -> gpui::Rgba {
    if facts.is_cold() {
        theme.TEXT_FAINT
    } else if facts.expiring_soon() {
        theme.WARN
    } else {
        theme.TEXT_DIM
    }
}

/// What the pill and its hover card read of the panel that owns them.
#[derive(Clone)]
pub(crate) struct CacheSource {
    session: Live<String>,
    card: Live<card::CacheView>,
}

impl CacheSource {
    pub(super) fn new(session: Live<String>, card: Live<card::CacheView>) -> Self {
        Self { session, card }
    }
}

/// A leaf view that wakes at the warning boundary, then ticks only while
/// displaying a live countdown.
pub(crate) struct CachePill {
    value: CachePillValue,
    source: CacheSource,
    anchor: Instant,
    tick: Option<Task<()>>,
}

impl CachePill {
    pub(crate) fn new(
        value: CachePillValue,
        source: CacheSource,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            value,
            source,
            anchor: cx.background_executor().now(),
            tick: None,
        }
    }

    pub(crate) fn set(&mut self, value: CachePillValue, cx: &mut Context<Self>) {
        if self.value != value {
            self.value = value;
            self.anchor = cx.background_executor().now();
            self.tick = None;
            cx.notify();
        }
    }

    pub(super) fn caption(&self, now: Instant) -> &'static str {
        match self.live(now).metric() {
            CacheMetric::Countdown(_) => "Footer: time left before cache expiry",
            CacheMetric::HitRate(_) => "Footer: % of last prompt read from cache",
        }
    }

    pub(super) fn retention(&self, now: Instant) -> Option<CacheFacts> {
        self.live(now).retention
    }

    fn live(&self, now: Instant) -> CachePillValue {
        let elapsed = now.saturating_duration_since(self.anchor).as_secs();
        CachePillValue {
            retention: self.value.retention.map(|facts| facts.after(elapsed)),
            ..self.value
        }
    }
}

impl Render for CachePill {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("CachePill");
        let value = self.live(cx.background_executor().now());
        if self.tick.is_none()
            && let Some(delay) = value.next_refresh()
        {
            self.tick = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |pill, cx| {
                    pill.tick = None;
                    cx.notify();
                });
            }));
        }
        let metric = value.metric();
        let theme = Theme::global();
        let source = self.source.clone();
        let reading = Live::of(&cx.entity(), |pill: &CachePill, cx| {
            let now = cx.background_executor().now();
            card::PillReading {
                caption: pill.caption(now),
                facts: pill.retention(now),
            }
        });
        use crate::swarm_glyph::Icon;
        let (icon, ink) = match metric {
            CacheMetric::Countdown(Some(facts)) => (
                if facts.is_cold() {
                    Icon::Snowflake
                } else {
                    Icon::Timer
                },
                tone(&facts, theme),
            ),
            CacheMetric::HitRate(Some(_)) => (Icon::Timer, theme.TEXT_DIM),
            _ => (Icon::Timer, theme.TEXT_FAINT),
        };
        let label = metric.label();
        super::chrome::composer_pill("panel-cache")
            .relative()
            .flex_none()
            .px_2()
            .gap_1()
            .text_color(ink)
            .child(
                div()
                    .debug_selector(move || format!("panel-cache-icon-{icon:?}").to_lowercase())
                    .flex_none()
                    .child(
                        crate::swarm_glyph::Mark::still(icon, ink, super::chrome::BAR_ICON)
                            .render(None),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| format!("panel-cache-label-{label}"))
                    .child(label),
            )
            .children(crate::fixture_control_targets::marker(
                || "hover:panel-cache".into(),
                || source.session.get(cx),
            ))
            .tooltip(move |_, cx| {
                cx.new(|cx| {
                    source.card.follow(cx);
                    reading.follow(cx);
                    card::CacheTooltip {
                        view: source.card.clone(),
                        pill: reading.clone(),
                    }
                })
                .into()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn live_model_requests_refresh_cache_before_the_turn_finishes(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "cache-live".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.provider = Some("anthropic".into());
            panel.items.push(Item::ResponseStats(
                super::super::response_stats::ResponseStats {
                    at_ms: Some(now_ms() - 37 * 60_000),
                    cache_read_tokens: Some(1000),
                    ..Default::default()
                },
            ));
            panel.apply(
                &ApiEvent::TokenUsage {
                    session_id: "cache-live".into(),
                    input: 100,
                    output: 10,
                    cache_read_input: Some(1000),
                    cache_creation_input: None,
                    effective_budget: None,
                },
                cx,
            );
            let facts = panel.cache_facts().expect("cache facts");
            assert!(
                facts.written_ago_secs < 5,
                "live usage must refresh timer: {facts:?}"
            );
        });
    }

    #[test]
    fn countdown_reads_in_minutes_and_says_when_it_is_cold() {
        assert_eq!(label(47 * 60 + 12), "48m");
        assert_eq!(label(60), "1m");
        assert_eq!(label(59), "59s");
        assert_eq!(label(0), "cold");
    }

    #[gpui::test]
    fn one_hour_cache_warns_at_twelve_minutes_even_with_a_high_hit_rate(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, _| {
            panel.provider = Some("anthropic".into());
            panel.model = Some("claude-opus-5-5".into());
            assert_eq!(panel.cache_retention().unwrap().1, 3600);
            panel.cache_telemetry.observe_at(
                &ApiEvent::RequestUsage {
                    session_id: panel.session_id.clone(),
                    request_id: "warm-claude".into(),
                    provider: "anthropic".into(),
                    model: "claude-opus-5-5".into(),
                    input: Some(2000),
                    output: None,
                    cache_read_input: Some(98_000),
                    cache_creation_input: Some(0),
                },
                now_ms() - 48 * 60_000,
            );
            assert_eq!(panel.cache_telemetry.last_hit_rate(), Some(98.0));
            assert_eq!(
                panel.cache_pill_value().unwrap().metric().label(),
                "12m left"
            );
            let facts = panel.cache_facts().unwrap();
            assert_eq!(facts.written_ago_secs, 48 * 60);
            assert_eq!(tone(&facts, Theme::global()), Theme::global().WARN);
        });
    }

    #[gpui::test]
    fn cache_pill_repaints_at_warning_and_expiry_without_input(cx: &mut gpui::TestAppContext) {
        for (ttl, remaining, before, warning, after) in [
            (
                3600,
                1200,
                "panel-cache-label-98%",
                "panel-cache-label-20m left",
                "panel-cache-label-98%",
            ),
            (
                300,
                61,
                "panel-cache-label-2m",
                "panel-cache-label-1m left",
                "panel-cache-label-cold",
            ),
        ] {
            let (_, vcx) = cx.add_window_view(|_, cx| {
                let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
                let value = CachePillValue {
                    short_countdown: ttl == 300,
                    retention: Some(CacheFacts::new(
                        ttl,
                        false,
                        0,
                        (ttl - remaining) as i64 * 1000,
                        None,
                    )),
                    hit_rate: Some(98.0),
                };
                CachePill::new(value, Panel::cache_source(&panel), cx)
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds(before).is_some(), "missing {before}");
            vcx.executor().advance_clock(Duration::from_secs(1));
            vcx.run_until_parked();
            assert!(vcx.debug_bounds(warning).is_some(), "missing {warning}");
            vcx.executor().advance_clock(Duration::from_secs(remaining));
            vcx.run_until_parked();
            assert!(vcx.debug_bounds(after).is_some(), "missing {after}");
        }
    }

    #[gpui::test]
    fn hit_rate_uses_the_last_completed_request_even_if_zero_or_missing(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.apply_bar_fixture("high", cx);
            assert_eq!(
                panel.cache_pill_value().map(CachePillValue::metric),
                Some(CacheMetric::HitRate(None))
            );
            panel.apply_cache_card_fixture(false, cx);
            assert_eq!(panel.cache_pill_value().unwrap().metric().label(), "83%");

            panel.provider = Some("openai".into());
            assert_eq!(panel.cache_pill_value().unwrap().metric().label(), "83%");
            for (request_id, read, expected) in [
                ("openai-warm", Some(60), "60%"),
                ("openai-cold", Some(0), "0%"),
                ("openai-unreported", None, "–"),
            ] {
                panel.apply(
                    &ApiEvent::RequestUsage {
                        session_id: panel.session_id.clone(),
                        request_id: request_id.into(),
                        provider: "openai".into(),
                        model: "gpt-5.6-sol".into(),
                        input: Some(100),
                        output: None,
                        cache_read_input: read,
                        cache_creation_input: None,
                    },
                    cx,
                );
                assert_eq!(panel.cache_pill_value().unwrap().metric().label(), expected);
            }
        });
    }

    /// No pill state spells out "cache": the drawn icon carries the word.
    #[test]
    fn the_label_never_spells_cache() {
        for secs in [0, 1, 59, 60, 300, 3600] {
            assert!(!label(secs).contains("cache"), "{}", label(secs));
        }
    }

    #[test]
    fn facts_count_down_from_the_last_touch() {
        let warm = CacheFacts::new(
            3600,
            false,
            1_000_000,
            1_000_000 + 47 * 60_000,
            Some(18_200),
        );
        assert_eq!(warm.remaining_secs, 3600 - 47 * 60);
        assert_eq!(warm.written_ago_secs, 47 * 60);
        assert!(!warm.is_cold());
        assert!(
            warm.expiring_soon(),
            "47m since last use leaves 13m and warns"
        );

        let cold = CacheFacts::new(300, false, 0, 600_000, None);
        assert!(cold.is_cold());
        assert_eq!(label(cold.remaining_secs), "cold");
    }

    #[test]
    fn retention_duration_formats_hours_minutes_and_seconds() {
        assert_eq!(span(3600), "1h");
        assert_eq!(span(300), "5m");
        assert_eq!(span(20), "20s");
    }

    /// The bar shows the countdown for a provider whose retention is known,
    /// built from the newest settled response's cache tokens.
    #[gpui::test]
    fn the_bar_shows_the_countdown_for_a_cache_capable_provider(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("cache-bar", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        panel.update(vcx, |panel, cx| {
            panel.provider = Some("gemini".into());
            panel.model = Some("gemini-3.1-pro".into());
            panel.items.push(Item::ResponseStats(
                super::super::response_stats::ResponseStats {
                    at_ms: Some(now_ms() - 2 * 60_000),
                    cache_read_tokens: Some(18_200),
                    ..Default::default()
                },
            ));
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("panel-cache-label-3m").is_some(),
            "a known retention with a cache touch earns a countdown"
        );
        // The word "cache" is a drawn timer, the same size as the other bar
        // marks, in front of a bare `3m`.
        let pill = vcx.debug_bounds("panel-cache").expect("pill");
        let icon = vcx
            .debug_bounds("panel-cache-icon-timer")
            .expect("timer icon");
        let text = vcx.debug_bounds("panel-cache-label-3m").unwrap();
        assert!(icon.left() >= pill.left() && icon.right() <= text.left());
        assert_eq!(icon.size.width, px(super::super::chrome::BAR_ICON));
        let shown = panel.read_with(vcx, |panel, _| {
            panel.cache_pill_value().unwrap().metric().label()
        });
        assert_eq!(shown, "3m");
        let facts = panel
            .read_with(vcx, |panel, _| panel.cache_facts())
            .expect("facts");
        assert!(facts.remaining_secs > 0, "{facts:?}");
        assert_eq!(facts.written_ago_secs, 2 * 60, "{facts:?}");
        assert_eq!(facts.cached_tokens, Some(18_200));

        // A provider with no documented retention shows nothing rather than an
        // invented window.
        panel.update(vcx, |panel, cx| {
            panel.provider = Some("cursor".into());
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-cache").is_none());
        assert!(
            panel
                .read_with(vcx, |panel, _| panel.cache_facts())
                .is_none()
        );
    }
}
