//! Which panel is active, where the camera looks, and how focus and panels move.
use super::*;

impl Workspace {
    /// Focus the slot at `index`, remembering the outgoing panel so
    /// `FocusPrevious` (niri's Alt+Tab) can return to it.
    pub(super) fn set_active(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.slots.len() {
            return;
        }
        // Preserve the most recent chat when moving through several utility
        // panels, rather than relying on only the immediately previous slot.
        if let Some(slot) = self.slots.get(self.active)
            && slot.panel.read_chat(cx).is_some_and(Panel::supports_voice)
        {
            self.last_voice_chat = Some(slot.panel.entity_id());
        }
        if self.slots[index]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::supports_voice)
        {
            self.last_voice_chat = Some(self.slots[index].panel.entity_id());
        }
        let outgoing = self
            .slots
            .get(self.active)
            .filter(|_| self.active != index)
            .map(|slot| slot.panel.entity_id());
        if let Some(outgoing) = outgoing {
            self.previous = Some(outgoing);
        }
        if self.last_canvas_width.is_some()
            && (self.active != index || self.active_row != self.slots[index].row)
        {
            self.begin_map_navigation(self.slots[index].row);
        }
        self.compact_sidebar_open = false;
        self.active = index;
        self.active_row = self.slots[index].row;
        self.row_focus[self.active_row] = Some(self.slots[index].panel.entity_id());
        self.retarget_camera();
        cx.notify();
    }

    pub(super) fn slot_width(&self, index: usize, viewport: f32) -> f32 {
        let fraction = self.slots[index].width_fraction;
        Self::width_for_fraction(fraction, viewport)
    }

    pub(super) fn width_for_fraction(fraction: f32, viewport: f32) -> f32 {
        // Narrow windows show one readable conversation without overwriting the
        // user's width presets. Widening restores the original arrangement.
        let fraction = if viewport < responsive::SINGLE_PANEL_BREAKPOINT {
            1.0
        } else {
            fraction
        };
        ((viewport - STRUT * 2.0) * fraction - GAP)
            .max(320.0_f32.min((viewport - STRUT * 2.0).max(1.0)))
    }

    pub(super) fn slot_left(&self, index: usize, viewport: f32) -> f32 {
        let mut x = STRUT;
        for i in self.row_indices(self.slots[index].row) {
            if i == index {
                break;
            }
            x += self.slot_width(i, viewport) + GAP;
        }
        x
    }

    pub(super) fn row_indices(&self, row: usize) -> impl Iterator<Item = usize> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| (slot.row == row).then_some(index))
    }

    pub(super) fn active_position_in_row(&self) -> usize {
        self.row_indices(self.active_row)
            .position(|index| index == self.active)
            .unwrap_or(0)
    }

    pub(super) fn select_row(&mut self, row: usize, preferred_position: usize) {
        self.begin_map_navigation(row.min(STRIP_COUNT - 1));
        self.active_row = row.min(STRIP_COUNT - 1);
        let remembered = self.row_focus[self.active_row].and_then(|entity_id| {
            self.slots
                .iter()
                .position(|slot| slot.row == self.active_row && slot.panel.entity_id() == entity_id)
        });
        let selected = remembered.or_else(|| {
            self.row_indices(self.active_row)
                .nth(preferred_position)
                .or_else(|| self.row_indices(self.active_row).last())
        });
        if let Some(index) = selected {
            if let Some(outgoing) = self
                .slots
                .get(self.active)
                .map(|slot| slot.panel.entity_id())
                && self.active != index
            {
                self.previous = Some(outgoing);
            }
            self.active = index;
            self.row_focus[self.active_row] = Some(self.slots[index].panel.entity_id());
        }
        self.retarget_camera();
    }

    pub(super) fn retarget_camera(&mut self) {
        // Camera target is resolved during render when the viewport width is
        // known; setting a sentinel forces recomputation.
        self.camera_dirty[self.active_row] = true;
    }

    /// Resolve the strip's scroll offset. This follows niri's
    /// `center-focused-column "never"`: the camera only scrolls far enough to
    /// bring the focused panel fully on screen, keeping it at the left or right
    /// edge rather than centering it.
    pub(super) fn resolve_camera_target(&mut self, viewport: f32) {
        self.camera_dirty[self.active_row] = false;
        // Target the settled layout, not the fading slot's old width. Closing
        // the left panel should pull its successor into place, not pan right
        // toward it and then reverse once the closing slot is removed.
        let surviving: Vec<_> = self
            .row_indices(self.active_row)
            .filter(|&index| !self.slots[index].closing)
            .collect();
        if surviving.is_empty() {
            self.camera_target[self.active_row] = 0.0;
            return;
        }
        let active = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
            .map(|_| self.active)
            .unwrap_or(surviving[0]);
        let left = STRUT
            + surviving
                .iter()
                .take_while(|&&index| index != active)
                .map(|&index| self.slot_width(index, viewport) + GAP)
                .sum::<f32>();
        let width = self.slot_width(active, viewport);
        let total = surviving
            .iter()
            .map(|&index| self.slot_width(index, viewport) + GAP)
            .sum::<f32>()
            + STRUT * 2.0;
        let current = self.camera_target[self.active_row];
        let target = scroll_into_view(current, left, width, viewport);
        let max_scroll = (total - viewport).max(-GAP);
        let target = target.clamp(-STRUT, max_scroll.max(-STRUT));
        let row = self.active_row;
        if (target - self.camera_target[row]).abs() > 0.01 {
            self.camera_from[row] = self.camera_x[row];
            self.camera_started[row] = Some(Instant::now());
            self.camera_target[row] = target;
        }
    }

    /// Record that the user reached an outcome by its keyboard shortcut.
    pub(super) fn learned(&mut self, skill_id: &str, cx: &mut Context<Self>) {
        self.coach.used_shortcut(skill_id, learning::now());
        self.after_coach_update(cx);
    }

    /// Onboarding is complete after the user has successfully exercised every
    /// control it presents. `practiced` intentionally accepts prompted use: the
    /// full coach can continue building recall after the tutorial is complete.
    pub(super) fn onboarding_complete(&self) -> bool {
        ONBOARDING_SKILLS
            .iter()
            .all(|skill| self.coach.trace(skill).practiced())
    }

    /// Record that the user reached the same outcome the long way, which is the
    /// evidence that they do not know (or have forgotten) the shortcut.
    pub(super) fn missed(&mut self, skill_id: &str, cx: &mut Context<Self>) {
        self.coach.used_slow_path(skill_id, learning::now());
        self.after_coach_update(cx);
    }

    /// Reveal or retire the hint toast and persist the model when it changed.
    pub(super) fn after_coach_update(&mut self, cx: &mut Context<Self>) {
        let now = learning::now();
        let hint = crate::config::coaching_hints()
            .then(|| self.coach.active_hint(now))
            .flatten();
        let visible = hint.is_some();
        if let Some(hint) = hint {
            self.coach_display_hint = Some(hint);
        }
        self.coach_progress
            .set(if visible { 1.0 } else { 0.0 }, Instant::now());
        if visible {
            self.coach_expiry_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(COACH_EXPIRY_WAKE).await;
                let _ = this.update(cx, |workspace, cx| workspace.after_coach_update(cx));
            }));
        } else {
            self.coach_expiry_task = None;
        }
        if self.coach.take_dirty()
            && let Some(persistence) = &self.learning_persistence
        {
            persistence.save(&self.coach);
        }
        cx.notify();
    }

    pub(super) fn dismiss_coach_hint(&mut self, cx: &mut Context<Self>) {
        self.coach.dismiss_hint();
        self.coach_expiry_task = None;
        self.coach_progress.set(0.0, Instant::now());
        cx.notify();
    }

    /// Clicking a panel to focus it. Only counted as a slow path when the
    /// keyboard would genuinely have done the same job: clicking the already
    /// focused panel, or reaching into another strip, is not a missed shortcut.
    /// Distance matters too, since crossing a strip is what `super-end` is for.
    pub(super) fn clicked_to_focus(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.active {
            return;
        }
        let Some(target_row) = self.slots.get(index).map(|slot| slot.row) else {
            return;
        };
        if target_row != self.active_row {
            // Another strip: the pointer is a reasonable way to get there.
            return;
        }
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        let from = indices.iter().position(|&i| i == self.active);
        let to = indices.iter().position(|&i| i == index);
        let Some((from, to)) = from.zip(to) else {
            return;
        };
        let steps = from.abs_diff(to);
        self.missed(click_skill(steps, to, indices.len()), cx);
    }

    pub(super) fn focus_left(&mut self, _: &FocusLeft, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("H", false, "Focus left", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && let Some(&target) = indices[..position]
                .iter()
                .rev()
                .find(|&&index| !self.slots[index].closing)
        {
            self.set_active(target, cx);
            self.begin_action_capture("focus_left", window);
            self.focus_active(window, cx);
            // Credit only when the key did something: pressing into the edge of
            // a strip is a no-op and proves nothing either way.
            self.learned("focus_left_right", cx);
            cx.notify();
        }
    }

    pub(super) fn focus_right(&mut self, _: &FocusRight, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("L", false, "Focus right", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && let Some(&target) = indices[position + 1..]
                .iter()
                .find(|&&index| !self.slots[index].closing)
        {
            self.set_active(target, cx);
            self.begin_action_capture("focus_right", window);
            self.focus_active(window, cx);
            self.learned("focus_left_right", cx);
            cx.notify();
        }
    }

    /// niri `focus-column-first`.
    pub(super) fn focus_first(&mut self, _: &FocusFirst, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("Home", false, "Focus first panel", cx);
        let first = self.row_indices(self.active_row).next();
        if let Some(index) = first.filter(|index| *index != self.active) {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            self.learned("focus_first_last", cx);
            cx.notify();
        }
    }

    /// niri `focus-column-last`.
    pub(super) fn focus_last(&mut self, _: &FocusLast, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("End", false, "Focus last panel", cx);
        let last = self.row_indices(self.active_row).last();
        if let Some(index) = last.filter(|index| *index != self.active) {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            self.learned("focus_first_last", cx);
            cx.notify();
        }
    }

    /// niri `focus-window-previous` (the user's Alt+Tab). Returns to the last
    /// focused panel wherever it now lives, including on another strip.
    pub(super) fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("Tab", false, "Return to previous panel", cx);
        let Some(previous) = self.previous else {
            return;
        };
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.entity_id() == previous)
        else {
            self.previous = None;
            return;
        };
        self.set_active(index, cx);
        self.focus_active(window, cx);
        self.learned("focus_previous", cx);
        cx.notify();
    }

    pub(super) fn focus_up(&mut self, _: &FocusUp, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("K", false, "Focus strip above", cx);
        if self.active_row > 0 {
            self.change_row(self.active_row - 1, window, cx);
            // Empty strips still animate. Learning credit must not decide
            // whether an actual navigation is included in latency telemetry.
            self.begin_action_capture("focus_up", window);
        } else {
            // Pressing into the top edge cannot navigate, but the chord was
            // still produced by hand, so the tutorial lesson must not linger.
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
    }

    pub(super) fn focus_down(&mut self, _: &FocusDown, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.showcase_motion("J", false, "Focus strip below", cx);
        if self.active_row + 1 < STRIP_COUNT {
            self.change_row(self.active_row + 1, window, cx);
            self.begin_action_capture("focus_down", window);
        } else {
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
    }

    /// Move focus to another strip, crediting the shortcut only when the move
    /// was meaningful. Stepping onto an empty strip when there is nothing else
    /// open shows the user pressed a key, not that they navigated anywhere, so
    /// it must not be taken as evidence of skill. It is still hands-on
    /// practice, though: the tutorial asked for the chord and the user
    /// produced it, so the lesson must clear rather than linger forever on a
    /// workspace whose other strips happen to be empty.
    pub(super) fn change_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let meaningful = self.switch_row_animated(row, window, cx);
        if meaningful {
            self.learned("focus_up_down", cx);
        } else {
            self.coach.used_shortcut_without_effect("focus_up_down");
            self.after_coach_update(cx);
        }
        cx.notify();
        meaningful
    }

    pub(super) fn begin_action_capture(&mut self, name: &'static str, window: &Window) {
        if let Some(capture) = self.action_capture.as_mut() {
            capture.begin(
                name,
                Instant::now(),
                window.frame_duration_snapshot(),
                window.input_latency_snapshot(),
            );
        }
    }

    /// Switch strips with the row transition animation. Returns whether the
    /// move actually landed on a different panel, so callers can decide if it
    /// counts as navigation.
    pub(super) fn switch_row_animated(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = self.active_position_in_row();
        let was_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        self.select_row(row, position);
        self.focus_active(window, cx);
        let is_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let landed_somewhere = self.row_indices(self.active_row).next().is_some();
        landed_somewhere && was_active != is_active
    }

    pub(super) fn move_panel_left(&mut self, _: &MovePanelLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("H", true, "Move panel left", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && position > 0
        {
            let previous = indices[position - 1];
            let moved_width = self.slots[self.active].width_fraction;
            let neighbour_width = self.slots[previous].width_fraction;
            self.slots.swap(self.active, previous);
            self.active = previous;
            self.start_order_animation(self.active, 1.0, neighbour_width);
            self.start_order_animation(indices[position], -1.0, moved_width);
            self.retarget_camera();
            self.learned("move_panel", cx);
            cx.notify();
        }
    }

    pub(super) fn move_panel_right(&mut self, _: &MovePanelRight, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("L", true, "Move panel right", cx);
        let indices: Vec<_> = self.row_indices(self.active_row).collect();
        if let Some(position) = indices.iter().position(|&index| index == self.active)
            && position + 1 < indices.len()
        {
            let next = indices[position + 1];
            let moved_width = self.slots[self.active].width_fraction;
            let neighbour_width = self.slots[next].width_fraction;
            self.slots.swap(self.active, next);
            self.active = next;
            self.start_order_animation(self.active, -1.0, neighbour_width);
            self.start_order_animation(indices[position], 1.0, moved_width);
            self.retarget_camera();
            self.learned("move_panel", cx);
            cx.notify();
        }
    }

    pub(super) fn start_order_animation(&mut self, index: usize, offset: f32, distance_fraction: f32) {
        let duration = transition::policy(Transition::PanelOrder).duration;
        self.slots[index].order_offset = AnimatedValue::new(offset, duration);
        self.slots[index].order_offset.set(0.0, Instant::now());
        self.slots[index].order_distance_fraction = distance_fraction;
    }

    pub(super) fn move_panel_up(&mut self, _: &MovePanelUp, window: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("K", true, "Move panel up", cx);
        self.move_panel_to_row(-1, window, cx);
    }

    /// niri `move-column-to-first`.
    pub(super) fn move_panel_to_first(
        &mut self,
        _: &MovePanelToFirst,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.showcase_motion("Home", true, "Move panel to start", cx);
        let first = self.row_indices(self.active_row).next();
        let on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == self.active_row);
        let Some(first) = first.filter(|first| on_strip && *first != self.active) else {
            return;
        };
        let slot = self.slots.remove(self.active);
        self.slots.insert(first, slot);
        self.active = first;
        self.retarget_camera();
        self.learned("move_panel_end", cx);
        cx.notify();
    }

    /// niri `move-column-to-last`.
    pub(super) fn move_panel_to_last(&mut self, _: &MovePanelToLast, _: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("End", true, "Move panel to end", cx);
        let last = self.row_indices(self.active_row).last();
        let on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == self.active_row);
        let Some(last) = last.filter(|last| on_strip && *last != self.active) else {
            return;
        };
        let slot = self.slots.remove(self.active);
        self.slots.insert(last, slot);
        self.active = last;
        self.retarget_camera();
        self.learned("move_panel_end", cx);
        cx.notify();
    }

    pub(super) fn move_panel_down(&mut self, _: &MovePanelDown, window: &mut Window, cx: &mut Context<Self>) {
        self.showcase_motion("J", true, "Move panel down", cx);
        self.move_panel_to_row(1, window, cx);
    }

    pub(super) fn move_panel_to_row(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.active_row as isize + delta;
        if !(0..STRIP_COUNT as isize).contains(&target) {
            return;
        }
        if self
            .slots
            .get(self.active)
            .is_none_or(|slot| slot.row != self.active_row)
        {
            return;
        }
        self.begin_map_navigation(target as usize);
        let slot = &mut self.slots[self.active];
        slot.row = target as usize;
        self.active_row = target as usize;
        self.row_focus[self.active_row] = Some(slot.panel.entity_id());
        self.retarget_camera();
        self.focus_active(window, cx);
        self.learned("move_panel_strip", cx);
        cx.notify();
    }

    pub(super) fn showcase_motion(
        &mut self,
        key: &str,
        shifted: bool,
        action: &'static str,
        cx: &mut Context<Self>,
    ) {
        if !self.showcase_mode {
            return;
        }
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        };
        self.showcase_cue = Some(ShowcaseCue {
            started_at: Some(Instant::now()),
            shortcut: if shifted {
                format!("{modifier} + Shift + {key}")
            } else {
                format!("{modifier} + {key}")
            },
            action,
            tutorial_group: if shifted { "move" } else { "navigate" },
        });
        self.schedule_showcase_expiry(cx);
        cx.notify();
    }

    pub(super) fn tutorial_cue(
        &mut self,
        key: &str,
        action: &'static str,
        tutorial_group: &'static str,
        cx: &mut Context<Self>,
    ) {
        if !self.showcase_mode {
            return;
        }
        let modifier = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        };
        self.showcase_cue = Some(ShowcaseCue {
            started_at: Some(Instant::now()),
            shortcut: format!("{modifier} + {key}"),
            action,
            tutorial_group,
        });
        self.schedule_showcase_expiry(cx);
        cx.notify();
    }

    pub(super) fn schedule_showcase_expiry(&mut self, cx: &mut Context<Self>) {
        if self.showcase_cue.is_none() {
            self.showcase_task = None;
            return;
        }
        self.showcase_task = Some(cx.spawn(async move |this, cx| {
            // Sleep through the readable hold, then wake rendering for the fade.
            // Replacing this task on every press cancels the previous expiry.
            cx.background_executor()
                .timer(SHOWCASE_DURATION - notifications::SHOWCASE_FADE)
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor()
                .timer(notifications::SHOWCASE_FADE)
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.showcase_cue = None;
                workspace.showcase_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn set_width(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let preset = match fraction {
            0.25 => Some(("1", "Panel width 25%")),
            0.5 => Some(("2", "Panel width 50%")),
            0.75 => Some(("3", "Panel width 75%")),
            1.0 => Some(("4", "Panel width 100%")),
            _ => None,
        };
        if let Some((key, action)) = preset {
            self.tutorial_cue(key, action, "resize", cx);
        }
        if let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        {
            slot.width_fraction = fraction;
            slot.animated_width.set(fraction, Instant::now());
            slot.restore_fraction = None;
            self.retarget_camera();
            self.learned("width_presets", cx);
        }
    }

    /// niri `switch-preset-column-width` (Alt+R): step to the next preset,
    /// wrapping around.
    pub(super) fn cycle_width(&mut self, _: &CycleWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("R", "Cycle panel width", "resize", cx);
        let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        else {
            return;
        };
        slot.width_fraction = next_preset(slot.width_fraction);
        slot.animated_width.set(slot.width_fraction, Instant::now());
        slot.restore_fraction = None;
        self.retarget_camera();
        self.learned("cycle_width", cx);
    }

    /// niri `maximize-column` (Alt+F): fill the viewport, or restore the
    /// previous width when already maximized.
    pub(super) fn maximize_width(&mut self, _: &MaximizeWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.tutorial_cue("F", "Maximize or restore panel", "resize", cx);
        let Some(slot) = self
            .slots
            .get_mut(self.active)
            .filter(|slot| slot.row == self.active_row)
        else {
            return;
        };
        let (width, restore) = toggle_maximize(slot.width_fraction, slot.restore_fraction);
        slot.width_fraction = width;
        slot.animated_width.set(width, Instant::now());
        slot.restore_fraction = restore;
        self.retarget_camera();
        self.learned("maximize", cx);
    }

    /// Keep the highlighted panel equal to the panel that receives typing.
    ///
    /// Keyboard focus can enter a panel without passing through `set_active`:
    /// a hot reload restores the focused composer by index, panel controls
    /// call `focus_input` and stop the click from reaching the surface, and a
    /// press inside one composer can be released over another panel. The
    /// reverse also happens: several paths call `set_active` and leave focus
    /// in the outgoing composer. Either way one session was highlighted while
    /// another took the prompt.
    ///
    /// Whichever side moved since the last frame is the user's intent. A new
    /// active panel pulls focus to it, otherwise the highlight follows focus.
    /// Focus held outside every panel (menus, search, the workspace) is left
    /// alone.
    pub(super) fn follow_keyboard_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overview || self.overview_progress.is_animating() {
            return;
        }
        let active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let owner = self
            .slots
            .iter()
            .position(|slot| {
                if slot.closing { return false; }
                let panel = self.panel_metadata(slot, cx);
                panel.keyboard_focus.iter().flatten().any(|handle| handle.is_focused(window))
                    || panel.panel_focus.contains_focused(window, cx)
            });
        if let Some(index) = owner
            && index != self.active
        {
            if self.focus_synced_active == active {
                self.set_active(index, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        self.focus_synced_active = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
    }

    pub fn focus_active(&self, window: &mut Window, cx: &mut App) {
        // Runtime updates must not move focus behind a launch-only modal.
        if self.account_sign_in.visible || self.onboarding_launch.error.is_some() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        // The session picker replaces every panel, so "the active panel" is
        // not on screen while it is open. Focus belongs to its query field.
        if let Some(state) = &self.resume {
            window.focus(&state.search.focus_handle(cx), cx);
            return;
        }
        // Overview cards contain copied content, not mounted panel views. Keep
        // shortcuts on the workspace until the exit animation remounts them.
        if self.overview || self.overview_progress.is_animating() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        if let Some(slot) = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
        {
            slot.panel.focus_input(window, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }
}
