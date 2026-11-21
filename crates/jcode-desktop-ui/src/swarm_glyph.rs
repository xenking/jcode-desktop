//! Small status marks: the per-worker glyph (spinner, check, cross, pause) and
//! the composer's activity icon. An animated mark is its own leaf view that
//! notifies only itself on the shared decorative grid, so a spinning icon
//! never rebuilds the sidebar or the transcript around it (see `pulse_text`).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, Entity, IntoElement, Render, Rgba, Transformation, Window, div,
    prelude::*, px, radians, svg,
};

use crate::swarm_view::{Activity, Glyph};
use crate::theme::Theme;

/// 20 Hz, the rate every other decorative pulse uses.
const TICK: Duration = Duration::from_millis(50);
const SPIN: Duration = Duration::from_millis(900);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Spinner,
    Check,
    Cross,
    Stop,
    Pause,
    Idle,
    Thinking,
    Tool,
    Background,
    Waiting,
    /// The swarm: three connected nodes, the upstream sidebar's worker mark.
    Workers,
    /// The prompt-cache countdown while the window is open.
    Timer,
    /// The prompt cache once its window has lapsed.
    Snowflake,
    OpenRig,
}

impl Icon {
    fn data(self) -> &'static [u8] {
        macro_rules! icon {
            ($name:literal) => {
                include_bytes!(concat!("../../../assets/icons/swarm/", $name, ".svg"))
                    as &'static [u8]
            };
        }
        match self {
            Self::Spinner => icon!("spinner"),
            Self::Check => icon!("check"),
            Self::Cross => icon!("cross"),
            Self::Stop => icon!("stop"),
            Self::Pause => icon!("pause"),
            Self::Idle => icon!("idle"),
            Self::Thinking => icon!("thinking"),
            Self::Tool => icon!("tool"),
            Self::Background => include_bytes!("../../../assets/icons/tools/bg.svg"),
            Self::Waiting => icon!("waiting"),
            Self::Workers => icon!("workers"),
            Self::Timer => icon!("timer"),
            Self::Snowflake => icon!("snowflake"),
            Self::OpenRig => icon!("openrig"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Still,
    Spin(Duration),
    Pulse(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub icon: Icon,
    pub color: Rgba,
    pub size: f32,
    pub motion: Motion,
}

impl Mark {
    pub(crate) fn glyph(glyph: Glyph, size: f32) -> Self {
        let theme = Theme::global();
        let (icon, color, motion) = match glyph {
            Glyph::Spinner => (Icon::Spinner, theme.ACCENT, Motion::Spin(SPIN)),
            Glyph::Check => (Icon::Check, theme.OK, Motion::Still),
            Glyph::Cross => (Icon::Cross, theme.ERROR, Motion::Still),
            Glyph::Pause => (Icon::Pause, theme.WARN, Motion::Still),
        };
        Self {
            icon,
            color,
            size,
            motion,
        }
    }

    /// Idle is a quiet dot. Thinking pulses, a running tool turns slowly and
    /// waiting on workers breathes, all subtle enough to sit in the bar.
    pub(crate) fn activity(activity: Activity, size: f32) -> Self {
        let theme = Theme::global();
        let (icon, color, motion) = match activity {
            Activity::Idle => (Icon::Idle, theme.composer_secondary(), Motion::Still),
            Activity::Thinking => (
                Icon::Thinking,
                theme.ACCENT,
                Motion::Pulse(Duration::from_millis(1600)),
            ),
            Activity::RunningTool => (
                Icon::Tool,
                theme.ACCENT,
                Motion::Spin(Duration::from_millis(3200)),
            ),
            Activity::WaitingOnWorkers => (
                Icon::Waiting,
                theme.WARN,
                Motion::Pulse(Duration::from_millis(2200)),
            ),
        };
        Self {
            icon,
            color,
            size,
            motion,
        }
    }

    /// A still mark, for callers that draw outside a `Marks` cache.
    pub(crate) fn still(icon: Icon, color: Rgba, size: f32) -> Self {
        Self {
            icon,
            color,
            size,
            motion: Motion::Still,
        }
    }

    pub(crate) fn render(self, phase: Option<f32>) -> gpui::Svg {
        let mut icon = svg()
            .data(self.icon.data())
            .size(px(self.size))
            .flex_none()
            .text_color(self.color);
        match (self.motion, phase) {
            (Motion::Spin(_), Some(phase)) => {
                icon = icon.with_transformation(Transformation::rotate(radians(
                    phase * std::f32::consts::TAU,
                )));
            }
            (Motion::Pulse(_), Some(phase)) => icon = icon.opacity(pulse_opacity(phase)),
            _ => {}
        }
        icon
    }
}

/// Breathes between 55% and 100%.
pub(crate) fn pulse_opacity(phase: f32) -> f32 {
    0.775 + 0.225 * (phase * std::f32::consts::TAU).cos()
}

/// Phase in `0..1` from a process-wide epoch, so marks sharing a period move
/// in step whenever they were created.
fn phase(period: Duration, cx: &gpui::App) -> f32 {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let now = cx.background_executor().now();
    let epoch = if cfg!(any(test, feature = "test-support")) {
        now
    } else {
        *EPOCH.get_or_init(|| now)
    };
    let period = period.as_nanos().max(1);
    (now.saturating_duration_since(epoch).as_nanos() % period) as f32 / period as f32
}

pub(crate) struct MarkView {
    mark: Mark,
    tick_pending: Rc<Cell<bool>>,
}

impl Render for MarkView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reduce = cx.reduce_motion() || crate::config::get().appearance.reduce_motion;
        let period = match self.mark.motion {
            Motion::Spin(period) | Motion::Pulse(period) => Some(period),
            Motion::Still => None,
        };
        let phase = period.filter(|_| !reduce).map(|period| {
            // One pending tick per render: a mark that stops being drawn
            // stops ticking on its own.
            if !self.tick_pending.replace(true) {
                let pending = self.tick_pending.clone();
                let id = cx.entity_id();
                let weak = cx.entity().downgrade();
                cx.spawn(async move |_, cx| {
                    crate::animation_clock::next_tick(cx.background_executor(), TICK).await;
                    pending.set(false);
                    if weak.upgrade().is_some() {
                        cx.update(|cx| cx.notify(id));
                    }
                })
                .detach();
            }
            phase(period, cx)
        });
        div()
            .flex_none()
            .size(px(self.mark.size))
            .child(self.mark.render(phase))
    }
}

/// One leaf view per key, created on first use.
#[derive(Default)]
pub(crate) struct Marks {
    marks: HashMap<String, Entity<MarkView>>,
    /// The composer's cache countdown. A leaf view for the same reason: its
    /// own tick repaints the label, never the panel around it.
    cache: Option<Entity<crate::panel::cache_timer::CachePill>>,
}

impl Marks {
    /// The mark for `key`. Still marks are plain icons. Animated ones are
    /// leaf views kept here so each owns its own tick.
    pub(crate) fn element<T: 'static>(
        &mut self,
        key: &str,
        mark: Mark,
        cx: &mut Context<T>,
    ) -> AnyElement {
        if mark.motion == Motion::Still {
            return div()
                .flex_none()
                .size(px(mark.size))
                .child(mark.render(None))
                .into_any_element();
        }
        if let Some(view) = self.marks.get(key) {
            if view.read(cx).mark != mark {
                view.update(cx, |view, cx| {
                    view.mark = mark;
                    cx.notify();
                });
            }
            return view.clone().into_any_element();
        }
        let view = cx.new(|_| MarkView {
            mark,
            tick_pending: Rc::default(),
        });
        self.marks.insert(key.to_owned(), view.clone());
        view.into_any_element()
    }

    /// The cache countdown pill, updated in place when a newer turn refreshes the
    /// window so its countdown anchor moves with the facts.
    pub(crate) fn cache_pill<T: 'static>(
        &mut self,
        facts: Option<crate::panel::cache_timer::CacheFacts>,
        panel: gpui::WeakEntity<crate::panel::Panel>,
        cx: &mut Context<T>,
    ) -> AnyElement {
        match self.cache.clone() {
            Some(view) => {
                view.update(cx, |view, cx| view.set(facts, cx));
                view.into_any_element()
            }
            None => {
                let view = cx.new(|_| crate::panel::cache_timer::CachePill::new(facts, panel));
                self.cache = Some(view.clone());
                view.into_any_element()
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn retain(&mut self, mut live: impl FnMut(&str) -> bool) {
        self.marks.retain(|key, _| live(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_stays_subtle_and_cyclic() {
        assert!((pulse_opacity(0.0) - 1.0).abs() < 1e-4);
        assert!((pulse_opacity(0.5) - 0.55).abs() < 1e-4);
        assert!((pulse_opacity(1.0) - pulse_opacity(0.0)).abs() < 1e-4);
    }

    #[test]
    fn only_running_glyphs_and_busy_activities_animate() {
        for (glyph, animated) in [
            (Glyph::Spinner, true),
            (Glyph::Check, false),
            (Glyph::Cross, false),
            (Glyph::Pause, false),
        ] {
            assert_eq!(
                Mark::glyph(glyph, 10.0).motion != Motion::Still,
                animated,
                "{glyph:?}"
            );
        }
        for (activity, animated) in [
            (Activity::Idle, false),
            (Activity::Thinking, true),
            (Activity::RunningTool, true),
            (Activity::WaitingOnWorkers, true),
        ] {
            assert_eq!(
                Mark::activity(activity, 12.0).motion != Motion::Still,
                animated,
                "{activity:?}"
            );
        }
    }

    #[test]
    fn every_icon_is_an_embedded_currentcolor_svg() {
        for icon in [
            Icon::Spinner,
            Icon::Check,
            Icon::Cross,
            Icon::Pause,
            Icon::Idle,
            Icon::Thinking,
            Icon::Tool,
            Icon::Waiting,
            Icon::Workers,
            Icon::Timer,
            Icon::Snowflake,
        ] {
            let svg = std::str::from_utf8(icon.data()).unwrap();
            assert!(svg.starts_with("<svg "), "{icon:?}");
            assert!(svg.contains("stroke=\"currentColor\""), "{icon:?}");
        }
    }
}
