//! Wheel and input scrolling of the transcript with momentum.
use super::*;

impl Panel {
    pub(super) fn glide_transcript_wheel(&mut self, pixels: f32, cx: &mut Context<Self>) {
        self.glide_transcript_input(pixels, false, cx);
    }

    pub(super) fn glide_transcript_input(&mut self, pixels: f32, precise: bool, cx: &mut Context<Self>) {
        if !pixels.is_finite() || pixels == 0.0 {
            return;
        }
        if pixels < 0.0
            && self.transcript_list.logical_scroll_top().item_ix == 0
            && self.transcript_list.logical_scroll_top().offset_in_item <= px(0.)
        {
            self.request_older_history();
        }
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_input(pixels, precise);
        if crate::config::get().appearance.reduce_motion || cx.reduce_motion() {
            self.scroll_transcript_direct(-pixels, cx);
            return;
        }
        self.release_startup_preview();
        // `ListState::scroll_by` is a programmatic movement and therefore does
        // not invoke the list's user-scroll callback. Release follow mode here
        // before the first animated frame, or render would pin every step back
        // to the live tail.
        if pixels < 0.0 && self.stick_to_bottom {
            self.stick_to_bottom = false;
            cx.notify();
        }
        if precise {
            self.transcript_wheel_glide.push_input(pixels, true);
        } else {
            self.transcript_wheel_glide.push(pixels);
        }
        self.transcript_wheel_frame
            .get_or_insert_with(|| cx.background_executor().now());
        cx.notify();
    }

    pub(super) fn cancel_transcript_momentum(&mut self) {
        self.transcript_wheel_glide.cancel();
        self.transcript_wheel_frame = None;
    }

    pub(super) fn scroll_transcript_direct(&mut self, delta_y: f32, cx: &mut Context<Self>) {
        self.cancel_transcript_momentum();
        self.release_startup_preview();
        if delta_y > 0.0 && self.stick_to_bottom {
            self.stick_to_bottom = false;
        }
        let current = self.transcript_list.scroll_px_offset_for_scrollbar();
        self.transcript_list
            .set_offset_from_scrollbar(point(current.x, current.y + px(delta_y)));
        if delta_y > 0.0
            && self.transcript_list.logical_scroll_top().item_ix == 0
            && self.transcript_list.logical_scroll_top().offset_in_item <= px(0.)
        {
            self.request_older_history();
        }
        self.resume_transcript_follow_after_scroll(cx);
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_applied(f32::from(
                current.y - self.transcript_list.scroll_px_offset_for_scrollbar().y,
            ));
        cx.notify();
    }

    pub(super) fn schedule_transcript_wheel_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.transcript_wheel_frame.is_none() || self.transcript_wheel_frame_pending {
            return;
        }
        self.transcript_wheel_frame_pending = true;
        let panel = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = panel.update(cx, |panel, cx| {
                panel.transcript_wheel_frame_pending = false;
                let Some(previous) = panel.transcript_wheel_frame else {
                    return;
                };
                let now = cx.background_executor().now();
                let elapsed = now.saturating_duration_since(previous);
                // Do not replay stale momentum after a suspended/hidden window.
                if elapsed > Duration::from_millis(250) {
                    panel.cancel_transcript_momentum();
                    return;
                }
                panel.transcript_wheel_frame = Some(now);
                if !panel.advance_transcript_wheel_by(elapsed, cx) {
                    panel.cancel_transcript_momentum();
                }
            });
        });
    }

    #[cfg(test)]
    pub(super) fn advance_transcript_wheel(&mut self, cx: &mut Context<Self>) -> bool {
        self.advance_transcript_wheel_by(Duration::from_millis(16), cx)
    }

    pub(super) fn advance_transcript_wheel_by(&mut self, elapsed: Duration, cx: &mut Context<Self>) -> bool {
        if elapsed.is_zero() {
            cx.notify();
            return true;
        }
        let Some(step) = self.transcript_wheel_glide.take_step(elapsed) else {
            return false;
        };
        let current = self.transcript_list.scroll_px_offset_for_scrollbar();
        let max = f32::from(self.transcript_list.max_offset_for_scrollbar().y).max(0.0);
        let target = (f32::from(current.y) - step).clamp(-max, 0.0);
        self.transcript_list
            .set_offset_from_scrollbar(point(current.x, px(target)));
        if step < 0.0 && target >= 0.0 {
            self.request_older_history();
        }
        self.resume_transcript_follow_after_scroll(cx);
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_applied(f32::from(current.y) - target);
        // Discard travel into an edge instead of accumulating invisible debt.
        if (step < 0.0 && target >= 0.0) || (step > 0.0 && target <= -max) {
            self.transcript_wheel_glide.cancel();
        }
        cx.notify();
        self.transcript_wheel_glide.is_gliding()
    }
}
