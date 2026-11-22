//! Prompt-cache status for the composer bar: a short retention countdown or
//! the latest completed request's cache hit rate.
use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Repaint rate while the window is open. Minute granularity means a coarse
/// tick is enough; the pill is a leaf view, so this never rebuilds the panel.
const TICK: Duration = Duration::from_secs(15);

fn shows_countdown(ttl_secs: Option<u64>) -> bool {
    ttl_secs.is_some_and(|secs| secs <= 20 * 60)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CachePillValue {
    Countdown(Option<CacheFacts>),
    HitRate(Option<f32>),
}

impl CachePillValue {
    fn label(self) -> String {
        match self {
            Self::Countdown(Some(facts)) => label(facts.remaining_secs),
            Self::HitRate(Some(percent)) => format!("{percent:.0}%"),
            Self::Countdown(None) | Self::HitRate(None) => "–".into(),
        }
    }
}

/// A provider's retention window and how long is left of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CacheFacts {
    pub ttl_secs: u64,
    pub remaining_secs: u64,
    /// How long ago the last request touched the cache.
    pub written_ago_secs: u64,
    /// The provider documents this as a retention estimate, not an expiry.
    pub is_estimate: bool,
    pub cached_tokens: Option<u64>,
}

impl CacheFacts {
    /// How long before expiry the pill turns warm. Scales with the TTL the way
    /// the TUI's warning window does, so a 1-hour cache warns for minutes and a
    /// 5-minute cache for a minute.
    pub(super) fn warn_window_secs(&self) -> u64 {
        (self.ttl_secs / 10).clamp(60, 600)
    }

    pub(super) fn is_cold(&self) -> bool {
        self.remaining_secs == 0
    }

    /// Warm but close enough to expiry to warn.
    pub(super) fn expiring_soon(&self) -> bool {
        !self.is_estimate && !self.is_cold() && self.remaining_secs <= self.warn_window_secs()
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

/// Build the facts from the newest cache touch. Split out from the panel so
/// the arithmetic is testable without a transcript.
pub(crate) fn facts(
    ttl_secs: u64,
    is_estimate: bool,
    touched_ms: i64,
    now_ms: i64,
    cached_tokens: Option<u64>,
) -> CacheFacts {
    let written_ago_secs = now_ms.saturating_sub(touched_ms).max(0) as u64 / 1000;
    CacheFacts {
        ttl_secs,
        remaining_secs: ttl_secs.saturating_sub(written_ago_secs),
        written_ago_secs,
        is_estimate,
        cached_tokens,
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

    /// The latest reported cache touch, including restored turns, estimates expiry.
    pub(super) fn cache_facts(&self) -> Option<CacheFacts> {
        let (provider, ttl_secs) = self.cache_retention()?;
        self.countdown_facts(provider, ttl_secs)
    }

    fn countdown_facts(&self, provider: &str, ttl_secs: u64) -> Option<CacheFacts> {
        let (touched_ms, cached_tokens) = self.last_cache_touch()?;
        Some(facts(
            ttl_secs,
            jcode_base::provider::cache_ttl_is_estimate(provider),
            touched_ms,
            now_ms(),
            cached_tokens,
        ))
    }

    fn cache_pill_value(&self) -> Option<CachePillValue> {
        let retention = self.cache_retention();
        if let Some((provider, ttl)) = retention
            && shows_countdown(Some(ttl))
        {
            return Some(CachePillValue::Countdown(
                self.countdown_facts(provider, ttl),
            ));
        }
        (retention.is_some() || self.cache_telemetry.has_requests())
            .then(|| CachePillValue::HitRate(self.cache_telemetry.last_hit_rate()))
    }

    pub(super) fn render_cache_pill(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let value = self.cache_pill_value()?;
        Some(
            self.swarm_marks
                .cache_pill(value, cx.entity().downgrade(), cx),
        )
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
            .filter(|worker| worker.state.is_live())
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
        cx.notify();
    }
}

/// Tone for the countdown: quiet while the window is open, warm when it is
/// nearly over, faint once it has lapsed.
pub(super) fn tone(facts: &CacheFacts, theme: &Theme) -> gpui::Rgba {
    if facts.is_cold() {
        theme.TEXT_FAINT
    } else if facts.expiring_soon() {
        theme.WARN
    } else {
        theme.TEXT_DIM
    }
}

/// The countdown as a leaf view: one 15s tick while the window is open, none
/// once it has lapsed, so an idle panel is not repainted for a static label.
pub(crate) struct CachePill {
    value: CachePillValue,
    panel: gpui::WeakEntity<Panel>,
    anchor: Instant,
    tick_pending: Rc<Cell<bool>>,
}

impl CachePill {
    pub(crate) fn new(value: CachePillValue, panel: gpui::WeakEntity<Panel>) -> Self {
        Self {
            value,
            panel,
            anchor: Instant::now(),
            tick_pending: Rc::default(),
        }
    }

    pub(crate) fn set(&mut self, value: CachePillValue, cx: &mut Context<Self>) {
        if self.value != value {
            self.value = value;
            self.anchor = Instant::now();
            cx.notify();
        }
    }

    pub(super) fn caption(&self) -> &'static str {
        match self.value {
            CachePillValue::Countdown(_) => "Footer: time left before cache expiry",
            CachePillValue::HitRate(_) => "Footer: % of last prompt read from cache",
        }
    }

    fn live(&self) -> CachePillValue {
        let CachePillValue::Countdown(Some(facts)) = self.value else {
            return self.value;
        };
        let elapsed = self.anchor.elapsed().as_secs();
        CachePillValue::Countdown(Some(CacheFacts {
            remaining_secs: facts.remaining_secs.saturating_sub(elapsed),
            written_ago_secs: facts.written_ago_secs.saturating_add(elapsed),
            ..facts
        }))
    }
}

impl Render for CachePill {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("CachePill");
        let value = self.live();
        if matches!(value, CachePillValue::Countdown(Some(facts)) if !facts.is_cold())
            && !self.tick_pending.replace(true)
        {
            let pending = self.tick_pending.clone();
            let id = cx.entity_id();
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(TICK).await;
                pending.set(false);
                if weak.upgrade().is_some() {
                    cx.update(|cx| cx.notify(id));
                }
            })
            .detach();
        }
        let theme = Theme::global();
        let panel = self.panel.clone();
        let pill = cx.entity().downgrade();
        use crate::swarm_glyph::Icon;
        let (icon, ink) = match value {
            CachePillValue::Countdown(Some(facts)) => (
                if facts.is_cold() {
                    Icon::Snowflake
                } else {
                    Icon::Timer
                },
                tone(&facts, theme),
            ),
            CachePillValue::HitRate(Some(_)) => (Icon::Timer, theme.TEXT_DIM),
            _ => (Icon::Timer, theme.TEXT_FAINT),
        };
        super::composer::composer_pill("panel-cache")
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
                        crate::swarm_glyph::Mark::still(icon, ink, super::composer::BAR_ICON)
                            .render(None),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "panel-cache-label".into())
                    .child(value.label()),
            )
            .children(crate::fixture_control_targets::marker(
                || "hover:panel-cache".into(),
                || {
                    panel
                        .upgrade()
                        .map(|panel| panel.read(cx).session_id.clone())
                },
            ))
            .tooltip(move |_, cx| {
                cx.new(|cx| {
                    if let Some(panel) = panel.upgrade() {
                        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                    }
                    if let Some(pill) = pill.upgrade() {
                        cx.observe(&pill, |_, _, cx| cx.notify()).detach();
                    }
                    super::cache_card::CacheTooltip {
                        panel: panel.clone(),
                        pill: pill.clone(),
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

    #[test]
    fn only_known_retention_through_twenty_minutes_shows_a_countdown() {
        assert!(shows_countdown(Some(1199)));
        assert!(shows_countdown(Some(1200)));
        assert!(!shows_countdown(Some(1201)));
        assert!(!shows_countdown(None));
    }

    #[gpui::test]
    fn hit_rate_uses_the_last_completed_request_even_if_zero_or_missing(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.apply_bar_fixture("high", cx);
            assert_eq!(
                panel.cache_pill_value(),
                Some(CachePillValue::HitRate(None))
            );
            panel.apply_cache_card_fixture(cx);
            assert_eq!(panel.cache_pill_value().unwrap().label(), "83%");

            panel.provider = Some("openai".into());
            assert_eq!(panel.cache_pill_value().unwrap().label(), "83%");
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
                assert_eq!(panel.cache_pill_value().unwrap().label(), expected);
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
        let warm = facts(
            3600,
            false,
            1_000_000,
            1_000_000 + 47 * 60_000,
            Some(18_200),
        );
        assert_eq!(warm.remaining_secs, 3600 - 47 * 60);
        assert_eq!(warm.written_ago_secs, 47 * 60);
        assert!(!warm.is_cold());
        assert!(!warm.expiring_soon(), "47m left is not a warning");

        let cold = facts(300, false, 0, 600_000, None);
        assert!(cold.is_cold());
        assert_eq!(label(cold.remaining_secs), "cold");
    }

    #[test]
    fn the_warn_window_scales_with_the_retention() {
        let short = facts(300, false, 0, 0, None);
        assert_eq!(short.warn_window_secs(), 60);
        let long = facts(3600, false, 0, 0, None);
        assert_eq!(long.warn_window_secs(), 360);
        let at_five_minutes = facts(3600, false, 0, 55 * 60_000, None);
        assert!(at_five_minutes.expiring_soon());
    }

    /// An estimated retention must never claim a hard warning: the provider
    /// may evict earlier or later than the number.
    #[test]
    fn estimates_do_not_warn() {
        let estimate = facts(600, true, 0, 9 * 60_000, None);
        assert!(estimate.remaining_secs > 0);
        assert!(!estimate.expiring_soon());
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
            vcx.debug_bounds("panel-cache-label").is_some(),
            "a known retention with a cache touch earns a countdown"
        );
        // The word "cache" is a drawn timer, the same size as the other bar
        // marks, in front of a bare `3m`.
        let pill = vcx.debug_bounds("panel-cache").expect("pill");
        let icon = vcx
            .debug_bounds("panel-cache-icon-timer")
            .expect("timer icon");
        let text = vcx.debug_bounds("panel-cache-label").unwrap();
        assert!(icon.left() >= pill.left() && icon.right() <= text.left());
        assert_eq!(icon.size.width, px(super::super::composer::BAR_ICON));
        let shown = panel.read_with(vcx, |panel, _| panel.cache_pill_value().unwrap().label());
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
        assert!(vcx.debug_bounds("panel-cache-label").is_none());
        assert!(
            panel
                .read_with(vcx, |panel, _| panel.cache_facts())
                .is_none()
        );
    }
}
