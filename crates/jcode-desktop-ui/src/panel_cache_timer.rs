//! Prompt-cache countdown for the composer bar.
//!
//! Providers keep the prompt cache for a fixed retention window that restarts
//! every time a request reads or writes it. The TUI already warns with
//! `⏳ cache 3m` when that window is nearly over; the desktop bar shows the
//! same countdown all the time, dimmed once the window has lapsed, so stepping
//! away for a coffee and paying to re-warm a long prompt is visible before it
//! happens rather than after.
//!
//! The facts come from the transcript: the newest settled response carries the
//! provider-reported cache tokens and the moment the turn finished, which is
//! the last cache touch. The retention window and its estimate flag come from
//! `jcode-base`, the same table the TUI reads, so both clients count down the
//! same number.
use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Repaint rate while the window is open. Minute granularity means a coarse
/// tick is enough; the pill is a leaf view, so this never rebuilds the panel.
const TICK: Duration = Duration::from_secs(15);

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
fn span(secs: u64) -> String {
    if secs >= 3600 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

fn tokens_text(tokens: Option<u64>) -> String {
    match tokens {
        Some(tokens) if tokens >= 1_000_000 => {
            format!(" · {:.1}M cached tokens", tokens as f64 / 1e6)
        }
        Some(tokens) if tokens >= 1_000 => format!(" · {}K cached tokens", tokens / 1_000),
        Some(tokens) => format!(" · {tokens} cached tokens"),
        None => String::new(),
    }
}

/// Hover copy: the window, when it was last touched and what it holds.
pub(super) fn tooltip(facts: &CacheFacts) -> String {
    let retention = if facts.is_estimate {
        format!("{} retention (estimate)", span(facts.ttl_secs))
    } else {
        format!("{} retention", span(facts.ttl_secs))
    };
    let state = if facts.is_cold() {
        format!("expired {} ago", span(facts.written_ago_secs))
    } else {
        format!(
            "{} left · last write {} ago",
            span(facts.remaining_secs),
            span(facts.written_ago_secs)
        )
    };
    format!(
        "Prompt cache: {retention} · {state}{}",
        tokens_text(facts.cached_tokens)
    )
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
    /// The newest response that reported cache tokens, with the moment it
    /// settled. That request is what refreshed the provider's window.
    fn last_cache_touch(&self) -> Option<(i64, Option<u64>)> {
        self.items.iter().rev().find_map(|item| match item {
            Item::ResponseStats(stats)
                if stats.cache_read_tokens.is_some() || stats.cache_creation_tokens.is_some() =>
            {
                stats
                    .at_ms
                    .map(|at| (at, stats.cache_read_tokens.or(stats.cache_creation_tokens)))
            }
            _ => None,
        })
    }

    /// Facts for this session's cache countdown, or nothing when the provider
    /// has no known retention window or no turn has touched the cache yet.
    pub(super) fn cache_facts(&self) -> Option<CacheFacts> {
        let provider = self
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())?;
        let ttl_secs =
            jcode_base::provider::cache_ttl_for_provider_model(provider, self.model.as_deref())?;
        let (touched_ms, cached_tokens) = self.last_cache_touch()?;
        Some(facts(
            ttl_secs,
            jcode_base::provider::cache_ttl_is_estimate(provider),
            touched_ms,
            now_ms(),
            cached_tokens,
        ))
    }

    /// The cache countdown pill (timer mark + `47m`), dimmed once the window lapses and warm when it is
    /// nearly over. A leaf view so its tick never rebuilds the composer.
    pub(super) fn render_cache_pill(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let facts = self.cache_facts()?;
        Some(self.swarm_marks.cache_pill(facts, cx))
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
    facts: CacheFacts,
    anchor: Instant,
    tick_pending: Rc<Cell<bool>>,
}

impl CachePill {
    pub(crate) fn new(facts: CacheFacts) -> Self {
        Self {
            facts,
            anchor: Instant::now(),
            tick_pending: Rc::default(),
        }
    }

    pub(crate) fn set(&mut self, facts: CacheFacts, cx: &mut Context<Self>) {
        if self.facts != facts {
            self.facts = facts;
            self.anchor = Instant::now();
            cx.notify();
        }
    }

    /// Facts with the seconds that passed since they were taken subtracted.
    fn live(&self) -> CacheFacts {
        let elapsed = self.anchor.elapsed().as_secs();
        CacheFacts {
            remaining_secs: self.facts.remaining_secs.saturating_sub(elapsed),
            written_ago_secs: self.facts.written_ago_secs.saturating_add(elapsed),
            ..self.facts
        }
    }
}

impl Render for CachePill {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("CachePill");
        let facts = self.live();
        if !facts.is_cold() && !self.tick_pending.replace(true) {
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
        let hover = tooltip(&facts);
        let ink = tone(&facts, theme);
        let icon = if facts.is_cold() {
            crate::swarm_glyph::Icon::Snowflake
        } else {
            crate::swarm_glyph::Icon::Timer
        };
        super::composer::composer_pill("panel-cache")
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
                    .child(label(facts.remaining_secs)),
            )
            .tooltip(move |_, cx| cx.new(|_| super::usage::MeterTooltip(hover.clone())).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_reads_in_minutes_and_says_when_it_is_cold() {
        assert_eq!(label(47 * 60 + 12), "48m");
        assert_eq!(label(60), "1m");
        assert_eq!(label(59), "59s");
        assert_eq!(label(0), "cold");
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
    fn tooltip_names_the_window_and_the_last_write() {
        let warm = facts(3600, false, 0, 12 * 60_000, Some(18_200));
        let hover = tooltip(&warm);
        assert!(hover.contains("1h retention"), "{hover}");
        assert!(hover.contains("48m left"), "{hover}");
        assert!(hover.contains("last write 12m ago"), "{hover}");
        assert!(hover.contains("18K cached tokens"), "{hover}");

        let cold = facts(300, true, 0, 600_000, None);
        let hover = tooltip(&cold);
        assert!(hover.contains("5m retention (estimate)"), "{hover}");
        assert!(hover.contains("expired 10m ago"), "{hover}");
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
            panel.provider = Some("anthropic".into());
            panel.model = Some("claude-opus-5-5".into());
            panel.items.push(Item::ResponseStats(
                super::super::response_stats::ResponseStats {
                    at_ms: Some(now_ms() - 12 * 60_000),
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
        // marks, in front of a bare `48m`.
        let pill = vcx.debug_bounds("panel-cache").expect("pill");
        let icon = vcx
            .debug_bounds("panel-cache-icon-timer")
            .expect("timer icon");
        let text = vcx.debug_bounds("panel-cache-label").unwrap();
        assert!(icon.left() >= pill.left() && icon.right() <= text.left());
        assert_eq!(icon.size.width, px(super::super::composer::BAR_ICON));
        let shown = panel
            .read_with(vcx, |panel, _| panel.cache_facts())
            .map(|facts| label(facts.remaining_secs))
            .unwrap();
        assert!(!shown.contains("cache"), "{shown}");
        assert_eq!(shown, "48m");
        let facts = panel
            .read_with(vcx, |panel, _| panel.cache_facts())
            .expect("facts");
        assert!(facts.remaining_secs > 0, "{facts:?}");
        assert_eq!(facts.written_ago_secs, 12 * 60, "{facts:?}");
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
