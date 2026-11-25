//! The strip of panels, painted, scrolled and animated.
use super::*;

impl Workspace {
    pub(super) fn render_row(
        &mut self,
        row: usize,
        viewport_w: f32,
        viewport_h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.row_indices(row).next().is_some() {
            self.render_strip(row, viewport_w, viewport_h, window, cx)
        } else {
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap_2()
                .items_center()
                .justify_center()
                .text_color(Theme::global().TEXT_DIM)
                // Empty strips still allow an upward touchpad hop. Moving
                // down requires the keyboard shortcut, even on an empty row.
                .on_scroll_wheel(cx.listener(
                    move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                        if this.active_row != row || !event.delta.precise() {
                            return;
                        }
                        let delta = event.delta.pixel_delta(window.line_height());
                        let dy = f32::from(delta.y);
                        // Moving down a workspace requires an explicit shortcut.
                        // Ignore touch travel in that direction, including previews.
                        if dy < 0.0 {
                            this.gesture.clear_pull();
                            cx.notify();
                            return;
                        }
                        let now = cx.background_executor().now();
                        if this
                            .gesture_seen
                            .is_none_or(|seen| now.duration_since(seen) > GESTURE_RESET)
                        {
                            this.gesture.reset();
                        }
                        this.gesture_seen = Some(now);
                        let switch = this.gesture.feed_vertical(dy, event.touch_phase);
                        if switch != 0 {
                            let target = if switch > 0 {
                                (this.active_row + 1).min(STRIP_COUNT - 1)
                            } else {
                                this.active_row.saturating_sub(1)
                            };
                            if target != this.active_row {
                                this.switch_row_animated(target, window, cx);
                            }
                        }
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .debug_selector(|| "empty-strip-hint".into())
                        .text_color(Theme::global().TEXT)
                        .child(format!("Workspace {} is empty", row + 1)),
                )
                .child(
                    div()
                        .debug_selector(|| "empty-strip-shortcut".into())
                        .child("Press Super+Enter to start a new session here"),
                )
                .child(div().text_size(px(12.0)).child(self.status.clone()))
                .into_any_element()
        }
    }

    /// A one-line snapshot of the strip layout. Written to the path in
    /// `JCODE_DESKTOP_STATE` on every render so an automated check can observe
    /// what the running window is actually doing.
    pub(super) fn dump_state(&self, window: &Window, cx: &mut Context<Self>) {
        if self.launch.var("JCODE_DESKTOP_STATE").is_none() {
            return;
        }
        // Written after the frame, outside rendering: reading every panel's
        // input here would make this view depend on all of them, and draw it
        // again whenever any caret blinks.
        let workspace = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            let _ = workspace.read_with(cx, |workspace, cx| workspace.write_state(window, cx));
        });
    }

    pub(super) fn write_state(&self, window: &Window, cx: &App) {
        let Some(path) = self.launch.var("JCODE_DESKTOP_STATE").map(str::to_owned) else {
            return;
        };
        let widths: Vec<f32> = self
            .row_indices(self.active_row)
            .map(|index| self.slots[index].width_fraction)
            .collect();
        let focus = self
            .row_indices(self.active_row)
            .position(|index| index == self.active);
        let line = describe_strip(&widths, focus, self.active_row);
        // The coach's decisions are also dumped, so an automated check can see
        // what the running app believes the user knows and what it is teaching.
        let now = learning::now();
        let coach = describe_coach(
            self.coach.overall_mastery(now),
            self.coach.effort_saved,
            self.coach.effort_wasted,
            self.coach.active_hint_id(),
        );
        let performance = self.performance.as_ref().map(|profile| {
            let snapshot = profile.snapshot();
            let gpui = self.gpui_performance;
            format!(
                "perf animation_fps={:.1} frame_p95_ms={:.1} missed={} view_p95_ms={:.1} draw_p95_ms={:.1} present_p95_ms={:.1} input_to_frame_p95_ms={:.1} coalesced={} mid_draw={}",
                snapshot.animation_fps,
                snapshot.animation_frame_p95_ms,
                snapshot.missed_animation_frames,
                snapshot.render_p95_ms,
                gpui.draw_p95_ms,
                gpui.present_p95_ms,
                gpui.input_to_frame_p95_ms,
                gpui.input_events_per_frame_p95,
                gpui.mid_draw_inputs,
            )
        });
        let suffix = performance
            .map(|line| format!("\n{line}"))
            .unwrap_or_default();
        let appearance = format!(
            "layout={:?} sidebar={:?}",
            self.layout_mode, self.sidebar_view
        );
        let navigation = self.navigation_state(window, cx);
        let contents = format!("{line}\n{coach}\n{appearance}{suffix}\nnavigation={navigation}\n");
        // Frames mostly repeat the last state. Skipping identical writes keeps
        // the file syscalls out of the frame loop the harness is measuring.
        thread_local! {
            static LAST: std::cell::RefCell<(String, String)> = Default::default();
        }
        LAST.with_borrow_mut(|last| {
            if last.0 == path && last.1 == contents {
                return;
            }
            if std::fs::write(&path, &contents).is_ok() {
                *last = (path, contents);
            }
        });
    }

    /// Strip rendering normally advances these values. Overview and completed
    /// row switches omit hidden strips, but elapsed animations must still retire
    /// or animation_active will keep rearming the fallback frame timer forever.
    pub(super) fn advance_hidden_strip_animations(&mut self, now: Instant, overview_visible: bool) {
        static STALE_FIXTURE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
            (harness::screenshot_mode() || cfg!(test))
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_STALE_HIDDEN_ANIMATIONS").as_deref()
                    == Ok("1")
        });
        if *STALE_FIXTURE {
            return;
        }
        let active_row = self.active_row;
        let outgoing_row = self.outgoing_row;
        let hidden = |row| overview_visible || (row != active_row && Some(row) != outgoing_row);
        for slot in &mut self.slots {
            if hidden(slot.row) {
                slot.animated_width.sample(now);
                slot.order_offset.sample(now);
            }
        }
        for row in 0..STRIP_COUNT {
            if !hidden(row) {
                continue;
            }
            let duration = if self.camera_touch_pan[row] {
                TOUCH_PAN_DURATION
            } else {
                transition::policy(Transition::Focus).duration
            };
            if self.camera_started[row]
                .is_some_and(|started| now.saturating_duration_since(started) >= duration)
            {
                self.camera_x[row] = self.camera_target[row];
                self.camera_started[row] = None;
                self.camera_touch_pan[row] = false;
            }
        }
    }

    pub(super) fn animation_active(&self) -> bool {
        self.row_progress.is_animating()
            || self.workspace_pull.is_animating()
            || self.live_tabs.is_animating()
            || self.overview_progress.is_animating()
            || self.hints_progress.is_animating()
            || self.coach_progress.is_animating()
            || self.camera_started.iter().any(Option::is_some)
            || self
                .slots
                .iter()
                .any(|slot| slot.animated_width.is_animating() || slot.order_offset.is_animating())
    }

    /// GPUI's next-frame callback is presentation-driven. Some Wayland
    /// compositors can delay that callback despite accepting 60 Hz presents,
    /// which leaves a lightweight workspace transition visibly stepping at
    /// 20–30 Hz. A single coalesced timer wake keeps animation state advancing;
    /// the compositor still decides when the resulting frame is presented.
    pub(super) fn ensure_animation_tick(&mut self, cx: &mut Context<Self>) {
        if self.animation_tick_task.is_some() {
            return;
        }
        self.animation_tick_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.animation_tick_task = None;
                if workspace.animation_active()
                    || workspace
                        .action_capture
                        .as_ref()
                        .is_some_and(ActionCapture::is_pending)
                {
                    crate::render_stats::note("workspace-notify:animation-tick");
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn render_strip(
        &mut self,
        row: usize,
        viewport_w: f32,
        viewport_h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let now = Instant::now();
        self.remove_finished_closing_panels(now, window, cx);
        if row == self.active_row && self.camera_dirty[row] {
            self.resolve_camera_target(viewport_w);
        }
        if self.camera_snap_pending[row] {
            self.camera_snap_pending[row] = false;
            self.camera_started[row] = None;
            self.camera_touch_pan[row] = false;
            self.camera_from[row] = self.camera_target[row];
        }
        // Touchpad pans use a much shorter interpolation than navigation. This
        // coalesces irregular input delivery into presentation frames while
        // remaining close enough to the fingers to feel direct.
        let camera_duration = if self.camera_touch_pan[row] {
            TOUCH_PAN_DURATION
        } else {
            transition::policy(Transition::Focus).duration
        };
        match self.camera_started[row] {
            Some(started) => {
                let elapsed = started.elapsed();
                if elapsed >= camera_duration {
                    self.camera_x[row] = self.camera_target[row];
                    self.camera_started[row] = None;
                    self.camera_touch_pan[row] = false;
                } else {
                    self.camera_x[row] = smoothed_camera_position(
                        self.camera_from[row],
                        self.camera_target[row],
                        elapsed,
                        camera_duration,
                    );
                    window.request_animation_frame();
                }
            }
            None => self.camera_x[row] = self.camera_target[row],
        }

        let panel_h = (viewport_h - STRIP_PADDING_TOP - STRIP_PADDING_Y).max(1.0);
        let mut panels = Vec::with_capacity(self.row_indices(row).count());
        let mut strip_width = 0.0;
        // Sample each slot once and keep its geometry together. Four parallel
        // heap buffers used to be allocated and zipped on every pan frame.
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.row != row {
                continue;
            }
            let fraction = slot.animated_width.sample(now);
            if slot.animated_width.is_animating() {
                window.request_animation_frame();
            }
            let close_progress = slot.close_progress.sample(now);
            if slot.close_progress.is_animating() {
                window.request_animation_frame();
            }
            let width = Self::width_for_fraction(fraction, viewport_w) * close_progress;
            strip_width += width;
            let progress = slot.order_offset.sample(now);
            if slot.order_offset.is_animating() {
                window.request_animation_frame();
            }
            let distance = Self::width_for_fraction(slot.order_distance_fraction, viewport_w) + GAP;
            panels.push((index, width, progress * distance, close_progress));
        }
        let camera_x = self.camera_x[row];
        let mut strip = div()
            .absolute()
            .top(px(STRIP_PADDING_TOP))
            .left(px(-camera_x))
            .w(px(strip_width + GAP * panels.len().saturating_sub(1) as f32))
            .h(px(panel_h));

        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let mut panel_left = 0.0;
        let mut active_surface = None;
        // Capture actual painted bounds, including camera/width/order animation
        // and clipping. Keyboard focus is not a wheel hit-test target.
        let panel_hitboxes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        for (index, width, order_offset, close_progress) in panels {
            let slot = &self.slots[index];
            let focused = index == self.active;
            let content_width = px((width - if folders { 0. } else { 12. }).max(1.));
            let metadata = self.panel_metadata(slot, cx);
            let openrig = metadata.openrig;
            let pending = metadata.pending;
            let default_directory = metadata.session_id == Panel::DEFAULT_DIRECTORY_SESSION_ID;
            let machines = metadata.session_id == Panel::MACHINES_SESSION_ID;
            let needs_surface_update = metadata.surface_focused != focused
                || (focused && metadata.finished_unseen)
                || (metadata.terminal && slot.panel.surface().needs_update(focused, cx));
            if openrig
                && slot
                    .panel
                    .read_chat(cx)
                    .is_some_and(|panel| panel.openrig_width_changed(content_width, cx))
            {
                slot.panel
                    .update_chat(cx, |panel, cx| panel.set_openrig_width(content_width, cx));
            }
            // Match the selected pane, even when a menu temporarily owns keyboard focus.
            // Update only on a real change: GPUI retains views between frames and
            // treats any update while drawing as a change to what they read.
            if needs_surface_update {
                slot.panel.surface().set_focused(focused, cx);
            }
            let surface_hitboxes = panel_hitboxes.clone();
            let left = panel_left + order_offset;
            let top = FOLDER_CONTENT_INSET.min(panel_h / 2.0);
            panel_left += width + GAP;
            // Skip panels entirely outside the viewport. Every camera frame
            // moves each panel's bounds, which defeats view retention, so an
            // off-screen panel would otherwise be rebuilt, laid out and
            // prepainted on every animation frame only to be clipped. The
            // focused panel always stays mounted so keyboard focus survives.
            if !focused && !strip_panel_visible(left, width, camera_x, viewport_w) {
                continue;
            }
            let surface = div()
                .id(("panel", index))
                .absolute()
                .left(px(left + if folders { 0. } else { 6. }))
                .top(px(top))
                .opacity(close_progress)
                // Tagged so a render test can click the real panel element
                // and exercise the pointer slow-path detection.
                .debug_selector(move || format!("panel-{index}"))
                .w(content_width)
                .h(px(panel_h - top))
                .flex_none()
                // Paint focus on each pane, rather than inheriting the shared sheet.
                .bg(Theme::global().panel_background(focused))
                .when(folders, |el| el.rounded(px(CORNER_RADIUS)))
                .when(!folders, |el| {
                    el.rounded(px(CORNER_RADIUS))
                        .border_1()
                        .border_color(if focused {
                            Theme::global().PANEL_BORDER_FOCUS
                        } else {
                            Theme::global().PANEL_BORDER
                        })
                })
                .when(folders, |el| {
                    el.child(folder_surface::measure(
                        self.folder_frame.clone(),
                        folder_surface::Region::Panel { active: focused },
                    ))
                })
                .overflow_hidden()
                // Activate during capture so no child can swallow the press.
                // Composers, selectable text and buttons all stop propagation,
                // which used to leave the old panel highlighted while the
                // pressed one took keyboard focus.
                .capture_any_mouse_down(cx.listener(
                    move |this, event: &gpui::MouseDownEvent, _window, cx| {
                        if event.button == gpui::MouseButton::Left {
                            this.clicked_to_focus(index, cx);
                            this.set_active(index, cx);
                        }
                    },
                ))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        this.clicked_to_focus(index, cx);
                        this.set_active(index, cx);
                        this.focus_pending = false;
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                    }),
                )
                // Selectable transcript text intentionally consumes mouse-down
                // so it can retain keyboard focus for copy. Mouse-up still
                // bubbles, which lets an inactive panel become active without
                // stealing that focus. This is especially important after a
                // hot reload, when every restored panel contains a fresh text
                // selection model.
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, _window, cx| {
                        this.clicked_to_focus(index, cx);
                        this.set_active(index, cx);
                    }),
                )
                .child(
                    gpui::canvas(
                        move |bounds, window, _| {
                            let hitbox = window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal);
                            surface_hitboxes.borrow_mut().push((index, hitbox));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(if default_directory {
                    self.render_default_directory_picker()
                } else if machines {
                    self.render_machines(cx)
                } else if pending {
                    self.render_pending_session(index, cx)
                } else {
                    // GPUI retains every view: a panel is drawn again from the
                    // last frame while nothing it read changed, and when only
                    // the panel changed, the workspace around it is drawn from
                    // the last frame with the panel built again in its place.
                    // A `.cached` view would opt out of the latter.
                    div()
                        .size_full()
                        .child(slot.panel.surface().view())
                        .into_any_element()
                });
            if focused {
                active_surface = Some(surface);
            } else {
                strip = strip.child(surface);
            }
        }
        // Paint the active folder last within this strip, not in a global
        // deferred layer that would incorrectly cover menus and overlays.
        strip = strip.children(active_surface);

        // Two-finger touchpad swipes (and horizontal mouse wheels) pan the
        // strip directly, like grabbing the canvas. The sticky axis lock in
        // `StripGesture` decides, per gesture, whether deltas pan the strip,
        // scroll the transcript under the pointer, or hop strips vertically.
        let total_width = self
            .row_indices(row)
            .map(|index| self.slot_width(index, viewport_w) + GAP)
            .sum::<f32>()
            + STRUT * 2.0;
        let reticle_alpha = (row == self.active_row)
            .then(|| self.gesture_reticle_alpha(cx))
            .flatten();
        if reticle_alpha.is_some() {
            // Keep painting so the reticle's hold and fade actually play out
            // after the last scroll delta.
            window.request_animation_frame();
        }
        // The reticle leans into an unbroken vertical pull, so the rubber
        // band toward the next strip is visible before it snaps.
        let reticle_pull =
            self.gesture.reticle_lean(viewport_h) + self.workspace_pull.value * viewport_h;
        let entity = cx.entity();
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .pl(px(GAP))
            // A capture-phase router: it sees every scroll event over the
            // strip before the transcripts do, so a gesture that committed
            // to panning can consume its vertical deltas instead of leaking
            // them into whichever panel happens to sit under the pointer.
            // The hitbox respects occlusion, so the minimap keeps its own
            // scroll behavior.
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
                    },
                    move |_, hitbox, window, _| {
                        window.on_mouse_event(
                            move |event: &gpui::ScrollWheelEvent, phase, window, cx| {
                                if phase != gpui::DispatchPhase::Capture
                                    || !hitbox.should_handle_scroll(window)
                                {
                                    return;
                                }
                                entity.update(cx, |this, cx| {
                                    // During a row transition both rows paint
                                    // a router; only the active row's routes,
                                    // and it also stops propagation for the
                                    // outgoing row's transcripts.
                                    if this.active_row != row {
                                        return;
                                    }
                                    let hovered_panel = panel_hitboxes
                                        .borrow()
                                        .iter()
                                        .rev()
                                        .find_map(|(index, hitbox)| {
                                            hitbox.should_handle_scroll(window).then_some(*index)
                                        });
                                    if this.route_strip_scroll(
                                        row,
                                        hovered_panel,
                                        event,
                                        total_width,
                                        viewport_w,
                                        window,
                                        cx,
                                    ) {
                                        cx.stop_propagation();
                                    }
                                });
                            },
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(strip)
            // The gesture reticle: while a touchpad swipe is panning this
            // strip, a ring at the camera's focal point shows exactly where
            // focus will land when the gesture settles. It fades right after
            // the fingers lift so it never lingers over content.
            .when_some(reticle_alpha, |el, alpha| {
                el.child(
                    div()
                        .debug_selector(|| "gesture-reticle".into())
                        .absolute()
                        .left(px((viewport_w - GESTURE_RETICLE_SIZE) / 2.0))
                        .top(px((viewport_h - GESTURE_RETICLE_SIZE) / 2.0 + reticle_pull))
                        .w(px(GESTURE_RETICLE_SIZE))
                        .h(px(GESTURE_RETICLE_SIZE))
                        .rounded_full()
                        .border_2()
                        .border_color(Theme::global().ACCENT)
                        .bg(gpui::rgba(0xffffff14))
                        .opacity(alpha)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(5.0))
                                .h(px(5.0))
                                .rounded_full()
                                .bg(Theme::global().ACCENT),
                        ),
                )
            })
            .into_any_element()
    }

    /// Opacity of the gesture reticle right now, or `None` once it has fully
    /// faded. Held bright while deltas keep arriving, then a short fade.
    pub(super) fn gesture_reticle_alpha(&self, cx: &App) -> Option<f32> {
        self.gesture_last
            .and_then(|last| gesture_alpha(cx.background_executor().now().duration_since(last)))
    }

    /// Route one scroll event over the strip through the sticky axis lock.
    /// Returns true when the strip consumed the event and the transcripts
    /// underneath must not also scroll.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn route_strip_scroll(
        &mut self,
        row: usize,
        hovered_panel: Option<usize>,
        event: &gpui::ScrollWheelEvent,
        total_width: f32,
        viewport_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let delta = event.delta.pixel_delta(window.line_height());
        let mut dx = f32::from(delta.x);
        let mut dy = f32::from(delta.y);
        if event.modifiers.shift {
            // Shift redirects vertical scrolling into a horizontal pan.
            if dx == 0.0 {
                dx = dy;
            }
            dy = 0.0;
        }
        let empty_panel = hovered_panel
            .and_then(|index| self.slots.get(index))
            .filter(|slot| slot.row == row)
            .is_some_and(|slot| !slot.panel.surface().scrollable(cx));
        // Use the platform clock for both gesture continuity and presentation.
        // In tests it advances explicitly, so a busy test runner cannot turn
        // consecutive touchpad events into a new gesture or expire the reticle.
        let now = cx.background_executor().now();
        if event.delta.precise() {
            // Apply continuity before every precise route, including empty
            // conversations, so small pulls separated by pauses do not add up.
            if self
                .gesture_seen
                .is_none_or(|seen| now.duration_since(seen) > GESTURE_RESET)
                || event.touch_phase == gpui::TouchPhase::Started
            {
                self.gesture.reset();
            }
            self.gesture_seen = Some(now);
        }
        let horizontally_locked = self.gesture.axis == GestureAxis::Horizontal
            && self
                .gesture_seen
                .is_some_and(|seen| now.duration_since(seen) <= GESTURE_RESET);
        // Downward workspace navigation is keyboard-only for now. Keep normal
        // transcript scrolling and horizontal panning, but do not accumulate a
        // downward pull over empty panels or during a horizontally locked pan.
        if dy < 0.0
            && (empty_panel
                || horizontally_locked
                || self.gesture.is_vertical_pull())
        {
            self.gesture.clear_pull();
            dy = 0.0;
        }
        let vertical_workspace_pull = event.delta.precise() && self.gesture.is_vertical_pull();
        if (empty_panel || vertical_workspace_pull)
            && !horizontally_locked
            && dx == 0.0
            && dy != 0.0
        {
            // With no conversation beneath the pointer, vertical scrolling is
            // workspace navigation. A wheel notch moves one row immediately;
            // precise touchpad deltas accumulate to the same deliberate
            // threshold used by a vertical breakout from a horizontal pan.
            let switch = if event.delta.precise() {
                self.gesture.feed_vertical(dy, event.touch_phase)
            } else if dy < 0.0 {
                1
            } else {
                -1
            };
            if switch != 0 {
                let target = if switch > 0 {
                    (self.active_row + 1).min(STRIP_COUNT - 1)
                } else {
                    self.active_row.saturating_sub(1)
                };
                if target != self.active_row {
                    self.switch_row_animated(target, window, cx);
                }
            }
            cx.notify();
            return true;
        }
        if !event.delta.precise() {
            // Mouse wheels have no gesture continuity: horizontal clicks pan,
            // vertical ones stay with the panel under the pointer.
            if dx != 0.0 {
                self.pan_strip(row, dx, total_width, viewport_w, false, window, cx);
                self.note_strip_pan(row, -dx, false, viewport_w, window, cx);
                cx.notify();
            }
            return false;
        }
        let lifted = event.touch_phase == gpui::TouchPhase::Ended;
        match self.gesture.feed(dx, dy, event.touch_phase) {
            Routed::ToPanel => {
                if lifted {
                    // Settle a pan whose lock was released before lifting.
                    self.note_strip_pan(row, 0.0, true, viewport_w, window, cx);
                }
                false
            }
            Routed::Strip {
                dx,
                switch,
                exclusive,
            } => {
                // The reticle explains the resisted vertical pull that hops
                // strips. A purely horizontal pan needs no visualization.
                if self.gesture.axis == GestureAxis::Horizontal && dy != 0.0 {
                    self.gesture_last = Some(now);
                }
                if dx != 0.0 {
                    self.pan_strip(row, dx, total_width, viewport_w, true, window, cx);
                }
                if dx != 0.0 || lifted {
                    self.note_strip_pan(row, -dx, lifted, viewport_w, window, cx);
                }
                if switch != 0 {
                    // The vertical pull broke the sticky axis: hop to the
                    // neighbouring strip. Remaining travel is latched until
                    // the next gesture rather than skipping more strips.
                    let target = if switch > 0 {
                        (self.active_row + 1).min(STRIP_COUNT - 1)
                    } else {
                        self.active_row.saturating_sub(1)
                    };
                    if target != self.active_row {
                        self.switch_row_animated(target, window, cx);
                    }
                }
                cx.notify();
                exclusive
            }
        }
    }

    /// Pan a strip's camera directly, cancelling any in-flight animation, and
    /// keep focus attached to what the gesture has actually brought under the
    /// camera. Without the focus follow, a swipe only moved the pixels: the
    /// old off-screen panel remained active and the next keyboard action
    /// snapped back to it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pan_strip(
        &mut self,
        row: usize,
        dx: f32,
        total_width: f32,
        viewport_w: f32,
        smooth: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Natural scrolling: content follows the fingers, so the camera
        // moves opposite the delta.
        // Accumulate precise deltas onto the target, not the lagging painted
        // position. This preserves every pixel of travel while the short
        // interpolation filters event/compositor timing jitter.
        let base = if smooth {
            self.camera_target[row]
        } else {
            self.camera_x[row]
        };
        let next = pan_camera(base, -dx, total_width, viewport_w);
        if (next - base).abs() < f32::EPSILON {
            return;
        }
        self.camera_target[row] = next;
        if smooth {
            self.camera_from[row] = self.camera_x[row];
            self.camera_started[row] = Some(Instant::now());
            self.camera_touch_pan[row] = true;
        } else {
            self.camera_x[row] = next;
            self.camera_from[row] = next;
            self.camera_started[row] = None;
            self.camera_touch_pan[row] = false;
        }
        self.camera_dirty[row] = false;
        if let Some(index) = panel_at_viewport_center(
            self.slots.iter().enumerate().filter_map(|(index, slot)| {
                (slot.row == row).then_some((index, slot.width_fraction))
            }),
            next,
            viewport_w,
        ) && index != self.active
        {
            if let Some(outgoing) = self.slots.get(self.active) {
                self.previous = Some(outgoing.panel.entity_id());
            }
            self.active = index;
            self.active_row = row;
            self.focus_active(window, cx);
        }
    }

    pub(super) fn render_overview(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut grid = div().relative().size_full().overflow_hidden();
        let mut columns = [0usize; STRIP_COUNT];

        for (index, slot) in self.slots.iter().enumerate() {
            let panel = self.panel_metadata(slot, cx);
            let focused = index == self.active;
            let title = panel.title.clone();
            let status = panel.status.clone();
            let preview = panel.overview_preview.clone();

            let column = columns[slot.row];
            columns[slot.row] += 1;
            let (left, top) = self.map_gesture.card_position(column, slot.row);
            grid = grid.child(
                div()
                    .id(("overview-panel", index))
                    .debug_selector(move || format!("overview-panel-{index}"))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(300.0))
                    .h(px(190.0))
                    .flex()
                    .flex_col()
                    .bg(if focused {
                        Theme::global().PANEL_BG
                    } else {
                        Theme::global().HEADER_BG
                    })
                    .rounded_xl()
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|el| el.bg(Theme::global().TOOL_BG))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _event, window, cx| {
                            this.set_active(index, cx);
                            this.overview = false;
                            this.overview_progress.set(0.0, Instant::now());
                            this.focus_active(window, cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .bg(Theme::global().HEADER_BG)
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(Theme::global().TEXT)
                                    .overflow_hidden()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(Theme::global().TEXT_DIM)
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p_3()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .overflow_hidden()
                            .line_height(relative(1.4))
                            .child(preview),
                    ),
            );
        }

        // New session card.
        let (new_left, new_top) = self
            .map_gesture
            .card_position(columns[self.active_row], self.active_row);
        grid = grid.child(
            div()
                .id("overview-new")
                .absolute()
                .left(px(new_left))
                .top(px(new_top))
                .w(px(300.0))
                .h(px(190.0))
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_dashed()
                .border_color(Theme::global().PANEL_BORDER)
                .rounded_xl()
                .cursor_pointer()
                .text_color(Theme::global().TEXT_DIM)
                .hover(|el| {
                    el.border_color(Theme::global().ACCENT)
                        .text_color(Theme::global().ACCENT)
                })
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _event, _window, cx| {
                        this.missed("new_panel", cx);
                        this.open_new_session(cx);
                        this.overview = false;
                        this.overview_progress.set(0.0, Instant::now());
                        cx.notify();
                    }),
                )
                .child("+ new session"),
        );

        div()
            .id("session-map")
            .debug_selector(|| "session-map".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(Self::pan_session_map))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::start_map_drag))
            .on_mouse_move(cx.listener(Self::drag_session_map))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::end_map_drag))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::end_map_drag))
            .child(grid)
            .child(div().absolute().bottom_2().left_4().text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .child("Session map · two-finger scroll to pan · pinch open or select a session to return"))
            .into_any_element()
    }

    pub(super) fn render_map_trip(
        &mut self,
        progress: f32,
        width: f32,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let y = self.map_row_position(progress);
        let mut scene = div().relative().size_full().overflow_hidden();
        for row in 0..STRIP_COUNT {
            let top = (row as f32 - y) * height;
            // Keep the destination mounted even before the camera reaches it.
            // Its input owns keyboard focus during rapid successive shortcuts.
            if top.abs() > height && row != self.active_row {
                continue;
            }
            let outgoing = self.outgoing_row == Some(row);
            let incoming = self.active_row == row;
            let content = self.render_row(row, width, height, window, cx);
            scene = scene.child(
                div()
                    .debug_selector(move || {
                        if incoming {
                            "row-transition-incoming".into()
                        } else if outgoing {
                            "row-transition-outgoing".into()
                        } else {
                            format!("row-transition-through-{row}")
                        }
                    })
                    .absolute()
                    .top(px(top))
                    .left_0()
                    .size_full()
                    .child(content),
            );
        }
        scene.into_any_element()
    }

    pub(super) fn render_workspace_pull(
        &mut self,
        pull: f32,
        width: f32,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let source = self.active_row;
        let entity = cx.entity();
        let total_width = self
            .row_indices(source)
            .map(|index| self.slot_width(index, width) + GAP)
            .sum::<f32>()
            + STRUT * 2.0;
        let mut scene = div().relative().size_full().overflow_hidden().child(
            gpui::canvas(
                move |bounds, window, _| {
                    (
                        bounds,
                        window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
                    )
                },
                move |_, (bounds, hitbox), window, _| {
                    window.on_mouse_event(
                        move |event: &gpui::ScrollWheelEvent, phase, window, cx| {
                            if phase != gpui::DispatchPhase::Capture
                                || !hitbox.should_handle_scroll(window)
                            {
                                return;
                            }
                            let y = f32::from(event.position.y - bounds.origin.y);
                            // The source row's router owns its visible region.
                            // Capture only the sliver uncovered by the preview,
                            // before its transcript can receive this gesture.
                            if y >= -pull * height && y < (1.0 - pull) * height {
                                return;
                            }
                            entity.update(cx, |this, cx| {
                                if this.active_row == source
                                    && this.route_strip_scroll(
                                        source,
                                        None,
                                        event,
                                        total_width,
                                        width,
                                        window,
                                        cx,
                                    )
                                {
                                    cx.stop_propagation();
                                }
                            });
                        },
                    );
                },
            )
            .absolute()
            .size_full(),
        );
        for row in 0..STRIP_COUNT {
            let top = (row as f32 - self.active_row as f32 - pull) * height;
            if top.abs() >= height {
                continue;
            }
            let active = row == self.active_row;
            let content = self.render_row(row, width, height, window, cx);
            scene = scene.child(
                div()
                    .debug_selector(move || {
                        if active {
                            "row-pull-active".into()
                        } else {
                            "row-pull-neighbor".into()
                        }
                    })
                    .absolute()
                    .top(px(top))
                    .left_0()
                    .size_full()
                    .child(content),
            );
        }
        scene.into_any_element()
    }
}
