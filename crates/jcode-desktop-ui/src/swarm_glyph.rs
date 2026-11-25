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

use crate::swarm_view::Glyph;
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
    Ask,
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
            Self::Ask => icon!("ask"),
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
    Arrive(Duration),
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

    pub(crate) fn thinking(size: f32) -> Self {
        Self {
            icon: Icon::Thinking,
            color: Theme::global().ACCENT,
            size,
            motion: Motion::Pulse(Duration::from_millis(1600)),
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
            (Motion::Arrive(_), Some(progress)) => {
                icon = icon.opacity(0.55 + 0.45 * jcode_desktop_motion::ease_out_cubic(progress));
            }
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
    arrived_at: Option<Instant>,
    tick_pending: Rc<Cell<bool>>,
}

impl Render for MarkView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reduce = cx.reduce_motion() || crate::config::get().appearance.reduce_motion;
        let frame = if reduce {
            self.arrived_at = None;
            None
        } else {
            match self.mark.motion {
                Motion::Spin(period) | Motion::Pulse(period) => Some(phase(period, cx)),
                Motion::Arrive(duration) => self.arrived_at.and_then(|start| {
                    let elapsed = cx
                        .background_executor()
                        .now()
                        .saturating_duration_since(start);
                    (elapsed < duration).then(|| elapsed.as_secs_f32() / duration.as_secs_f32())
                }),
                Motion::Still => None,
            }
        };
        // One pending tick per render: a mark that stops being drawn
        // stops ticking on its own.
        if frame.is_some() && !self.tick_pending.replace(true) {
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
        div()
            .flex_none()
            .size(px(self.mark.size))
            .child(self.mark.render(frame))
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
        self.retained_element(key, mark, cx)
    }

    /// Keep the previous mark even when still, so entry transitions only run
    /// on a change of glyph, not on every render or on restored sessions.
    pub(crate) fn retained_element<T: 'static>(
        &mut self,
        key: &str,
        mark: Mark,
        cx: &mut Context<T>,
    ) -> AnyElement {
        if let Some(view) = self.marks.get(key) {
            if view.read(cx).mark != mark {
                view.update(cx, |view, cx| {
                    view.arrived_at = if !matches!(mark.motion, Motion::Arrive(_)) {
                        None
                    } else if view.mark.icon != mark.icon {
                        Some(cx.background_executor().now())
                    } else {
                        view.arrived_at
                    };
                    view.mark = mark;
                    cx.notify();
                });
            }
            return view.clone().into_any_element();
        }
        let view = cx.new(|_| MarkView {
            mark,
            arrived_at: None,
            tick_pending: Rc::default(),
        });
        self.marks.insert(key.to_owned(), view.clone());
        view.into_any_element()
    }

    pub(crate) fn cache_pill<T: 'static>(
        &mut self,
        value: crate::panel::cache_timer::CachePillValue,
        source: crate::panel::cache_timer::CacheSource,
        cx: &mut Context<T>,
    ) -> AnyElement {
        match self.cache.clone() {
            Some(view) => {
                view.update(cx, |view, cx| view.set(value, cx));
                view.into_any_element()
            }
            None => {
                let view = cx.new(|cx| crate::panel::cache_timer::CachePill::new(value, source, cx));
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
        assert_ne!(Mark::thinking(12.0).motion, Motion::Still);
    }

    #[test]
    fn every_icon_is_an_embedded_currentcolor_svg() {
        for icon in [
            Icon::Spinner,
            Icon::Check,
            Icon::Cross,
            Icon::Stop,
            Icon::Pause,
            Icon::Idle,
            Icon::Thinking,
            Icon::Tool,
            Icon::Waiting,
            Icon::Ask,
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
