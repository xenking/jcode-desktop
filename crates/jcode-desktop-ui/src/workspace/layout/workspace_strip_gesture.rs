//! The sticky axis lock for two-finger strip gestures.
use super::*;

/// Which axis a touchpad gesture has committed to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum GestureAxis {
    #[default]
    Undecided,
    Horizontal,
    Vertical,
}

/// The sticky axis lock for two-finger strip gestures. A gesture that starts
/// horizontally owns the strip: it pans, and once enough vertical travel
/// accumulates it breaks the stickiness and hops strips. A gesture that
/// starts vertically belongs to whatever is under the pointer and never pans.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StripGesture {
    pub(super) axis: GestureAxis,
    pub(super) travel_x: f32,
    pub(super) travel_y: f32,
    /// Vertical pull accumulated while horizontally locked. Positive pulls
    /// toward the strip below (natural: fingers up reveal what is beneath).
    pub(super) pull: f32,
    /// Remains latched through Ended and momentum until a fresh gesture.
    pub(super) switched: bool,
}

/// Where one scroll delta goes, decided by the sticky axis lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Routed {
    /// The event belongs to whatever is under the pointer (a transcript).
    ToPanel,
    /// The strip takes it: pan by `dx`, hop `switch` strips, and when
    /// `exclusive` the event must not also reach the transcript.
    Strip {
        dx: f32,
        switch: i8,
        exclusive: bool,
    },
}

impl StripGesture {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin_phase(&mut self, phase: gpui::TouchPhase) -> bool {
        use gpui::TouchPhase::{Cancelled, Ended, Started};
        if phase == Started {
            self.reset();
        }
        if matches!(phase, Ended | Cancelled) {
            // Some platforms send momentum as Moved after fingers lift.
            // Clear the axis/pull, but do not re-arm a completed swipe.
            let switched = self.switched || phase == Cancelled;
            self.reset();
            self.switched = switched;
            return false;
        }
        true
    }

    /// Drop any accumulated vertical pull; the rubber band snaps home.
    pub(super) fn clear_pull(&mut self) {
        self.pull = 0.0;
    }

    /// How far the vertical pull has travelled toward the strip break, in
    /// `-1.0..=1.0`. Negative pulls upward, positive pulls downward.
    pub(super) fn break_progress(&self) -> f32 {
        (self.pull / STRIP_BREAK).clamp(-1.0, 1.0)
    }

    /// True while a vertical pull is accumulating under a vertical lock.
    pub(super) fn is_vertical_pull(&self) -> bool {
        self.axis == GestureAxis::Vertical && self.pull != 0.0
    }

    /// The resisted pull the gesture reticle leans into while a horizontal
    /// pan is locked, bounded to a third of the viewport height.
    pub(super) fn reticle_lean(&self, viewport_h: f32) -> f32 {
        if self.axis == GestureAxis::Horizontal {
            (self.pull * PULL_RESISTANCE).clamp(-viewport_h / 3.0, viewport_h / 3.0)
        } else {
            0.0
        }
    }

    pub(super) fn pull_vertical(&mut self, dy: f32) -> i8 {
        if self.switched {
            return 0;
        }
        self.pull -= dy;
        if self.pull.abs() < STRIP_BREAK {
            return 0;
        }
        let direction = if self.pull > 0.0 { 1 } else { -1 };
        self.pull = 0.0;
        self.switched = true;
        direction
    }

    /// Empty panels and empty rows share the same latch as axis breakouts.
    pub(super) fn feed_vertical(&mut self, dy: f32, phase: gpui::TouchPhase) -> i8 {
        if !self.begin_phase(phase) {
            return 0;
        }
        if self.axis != GestureAxis::Vertical {
            self.pull = 0.0;
            self.axis = GestureAxis::Vertical;
        }
        self.pull_vertical(dy)
    }

    /// Advance the state machine by one precise (touchpad) delta.
    pub(super) fn feed(&mut self, dx: f32, dy: f32, phase: gpui::TouchPhase) -> Routed {
        let owned_by_strip = self.switched || self.axis == GestureAxis::Horizontal;
        if !self.begin_phase(phase) {
            if !owned_by_strip {
                return Routed::ToPanel;
            }
            return Routed::Strip {
                dx: 0.0,
                switch: 0,
                exclusive: true,
            };
        }
        if self.switched {
            // Do not leak the tail of a workspace swipe into the destination
            // transcript. Horizontal panning can still continue normally.
            return Routed::Strip {
                dx,
                switch: 0,
                exclusive: true,
            };
        }
        match self.axis {
            GestureAxis::Undecided => {
                self.travel_x += dx.abs();
                self.travel_y += dy.abs();
                if self.travel_x >= AXIS_LOCK || self.travel_y >= AXIS_LOCK {
                    self.axis = if self.travel_x >= self.travel_y {
                        GestureAxis::Horizontal
                    } else {
                        GestureAxis::Vertical
                    };
                }
                match self.axis {
                    GestureAxis::Vertical => Routed::ToPanel,
                    // Pan horizontal motion immediately, but keep the event
                    // shared until the lock resolves so the first few small
                    // vertical pixels still reach the transcript.
                    axis => Routed::Strip {
                        dx,
                        switch: 0,
                        exclusive: axis == GestureAxis::Horizontal,
                    },
                }
            }
            GestureAxis::Vertical => Routed::ToPanel,
            GestureAxis::Horizontal => {
                let switch = self.pull_vertical(dy);
                Routed::Strip {
                    dx,
                    switch,
                    exclusive: true,
                }
            }
        }
    }
}
