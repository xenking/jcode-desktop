//! Chrome around the strip: minimap, update chip and hints.
use super::*;

impl Workspace {
    /// The automatic-update chip.
    ///
    /// Sparkle used to update entirely in the background, so a user running a
    /// broken build had no way to learn that a fix already existed. The chip
    /// makes each phase visible: a pulse while a check or download is in
    /// flight, and a solid, clickable prompt once a build is staged and one
    /// restart away.
    pub(super) fn render_update_chip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let state = updates::current();
        let label = state.label()?;
        let actionable = state.is_actionable();
        let paired = matches!(state, updates::UpdateState::PairDrain { .. });
        let ink = if actionable {
            Theme::global().ACCENT
        } else {
            Theme::global().TEXT_DIM
        };

        let mut chip = div()
            .id("update-chip")
            // Tagged so a render test can prove the chip painted and can click
            // the real element, rather than only asserting on updater state.
            .debug_selector(|| "update-chip".into())
            .when(!paired, |chip| {
                chip.absolute()
                    .bottom(px(UPDATE_CHIP_BOTTOM))
                    .right(px(MINIMAP_RIGHT))
            })
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2p5()
            .py_1()
            .rounded_full()
            .max_w(relative(1.0))
            .bg(Theme::global().MINIMAP_BG)
            .border_1()
            .border_color(if actionable {
                Theme::global().PANEL_BORDER_FOCUS
            } else {
                Theme::global().PANEL_BORDER
            })
            .text_size(px(10.5))
            .font_family(Theme::global().FONT_MONO)
            .text_color(ink)
            .occlude()
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(label),
            );

        if let updates::UpdateState::PairDrain { blockers, retry } = state {
            if !blockers.is_empty() {
                let details = blockers.join("\n");
                chip = chip
                    .tooltip(move |_, cx| cx.new(|_| PairedUpdateTooltip(details.clone())).into());
            }
            if retry {
                chip = chip.child(
                    div()
                        .id("cancel-paired-update-retry")
                        .debug_selector(|| "cancel-paired-update-retry".into())
                        .flex_none()
                        .rounded_full()
                        .px_2()
                        .bg(Theme::global().HEADER_BG)
                        .text_color(Theme::global().TEXT)
                        .hover(|el| el.opacity(0.85))
                        .cursor_pointer()
                        .on_click(|_, _, cx| {
                            cx.stop_propagation();
                            updates::cancel_pair_retry();
                        })
                        .child("Cancel"),
                );
            }
        }

        if actionable {
            chip = chip
                .cursor_pointer()
                .hover(|el| el.text_color(Theme::global().TEXT))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|_this, _event, _window, _cx| {
                        // Source builds have no Sparkle framework, so this is a
                        // no-op there rather than a crash.
                        updates::install_now();
                    }),
                );
        }

        Some(if paired {
            div()
                .w_full()
                .h(px(PAIRED_UPDATE_ROW_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_end()
                .px_3()
                .child(chip)
                .into_any_element()
        } else {
            chip.into_any_element()
        })
    }

    /// The minimap: a rounded card in the top right that draws every strip to
    /// scale. Panels are proportional rectangles, the focused panel is lit, a
    /// lens shows where the camera is looking, clicking a panel jumps to it,
    /// clicking a track switches strips, and scrolling over the map pans the
    /// active strip.
    pub(super) fn render_minimap(
        &self,
        viewport_w: f32,
        viewport_h: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let track_w = MINIMAP_WIDTH - MINIMAP_PADDING * 2.0 - MINIMAP_LABEL_WIDTH;
        let panel_track_h = MINIMAP_ROW_HEIGHT - MINIMAP_PANEL_INSET * 2.0;
        let widest = (0..STRIP_COUNT)
            .map(|row| {
                self.row_indices(row)
                    .map(|index| self.slot_width(index, viewport_w) + GAP)
                    .sum::<f32>()
                    + STRUT * 2.0
            })
            .fold(viewport_w, f32::max);
        let scale = minimap_scale(track_w, panel_track_h, viewport_w, viewport_h, widest);

        let mut card = div()
            .id("minimap")
            .debug_selector(|| "minimap".into())
            .absolute()
            .top(px(MINIMAP_TOP))
            .right(px(MINIMAP_RIGHT))
            .w(px(MINIMAP_WIDTH))
            .h(px(MINIMAP_HEIGHT))
            .p(px(MINIMAP_PADDING))
            .pl(px(MINIMAP_PADDING + MINIMAP_LABEL_WIDTH))
            .flex()
            .flex_col()
            .gap(px(MINIMAP_ROW_GAP))
            .rounded_lg()
            .bg(Theme::global().MINIMAP_BG)
            .border_1()
            .border_color(Theme::global().PANEL_BORDER)
            .occlude()
            // Scrolling over the map pans the active strip's camera, scaled
            // back up to canvas distance so the map and canvas move 1:1.
            .on_scroll_wheel(cx.listener(
                move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let row = this.active_row;
                    let delta = event.delta.pixel_delta(window.line_height());
                    let mut dx = f32::from(delta.x);
                    if dx == 0.0 {
                        dx = f32::from(delta.y);
                    }
                    if dx == 0.0 || scale <= f32::EPSILON {
                        return;
                    }
                    let total = this
                        .row_indices(row)
                        .map(|index| this.slot_width(index, viewport_w) + GAP)
                        .sum::<f32>()
                        + STRUT * 2.0;
                    let next = pan_camera(this.camera_x[row], -dx / scale, total, viewport_w);
                    if (next - this.camera_x[row]).abs() >= f32::EPSILON {
                        this.camera_x[row] = next;
                        this.camera_target[row] = next;
                        this.camera_from[row] = next;
                        this.camera_started[row] = None;
                        this.camera_dirty[row] = false;
                    }
                    cx.notify();
                },
            ));

        for row in 0..STRIP_COUNT {
            let active_row = row == self.active_row;
            let accent = Theme::global().workspace_accent(row);
            let mut track = div()
                .id(("minimap-row", row))
                .debug_selector(move || format!("minimap-row-{row}"))
                .relative()
                .h(px(MINIMAP_ROW_HEIGHT))
                .rounded(px(3.0))
                .cursor_pointer()
                .bg(Theme::global().minimap_row_background(active_row, accent))
                .hover(|el| el.bg(Theme::global().minimap_row_hover(accent)))
                .child(
                    div()
                        .id(("workspace-map-badge", row))
                        .debug_selector(move || format!("workspace-map-badge-{row}"))
                        .absolute()
                        .left(px(-MINIMAP_LABEL_WIDTH))
                        .top_0()
                        .w(px(MINIMAP_LABEL_WIDTH - 4.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                let position = this.active_position_in_row();
                                this.select_row(row, position);
                                this.focus_active(window, cx);
                                cx.notify();
                            }),
                        )
                        .bg(if active_row {
                            accent
                        } else {
                            Theme::global().MINIMAP_BG
                        })
                        .text_color(if active_row {
                            Theme::global().PANEL_BG
                        } else {
                            accent
                        })
                        .child(format!("{}", row + 1))
                        .when(active_row, |el| {
                            el.child(div().absolute().inset_0().debug_selector(move || {
                                format!("workspace-map-badge-{row}-active")
                            }))
                        }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _event, window, cx| {
                        let position = this.active_position_in_row();
                        this.select_row(row, position);
                        this.focus_active(window, cx);
                        cx.notify();
                    }),
                );

            for index in self.row_indices(row) {
                let left = self.slot_left(index, viewport_w) * scale;
                let width = (self.slot_width(index, viewport_w) * scale - 1.0).max(2.0);
                // Panels keep the canvas aspect ratio: the height is the real
                // panel height under the same scale, so a panel taller than
                // wide on screen reads taller than wide here too.
                let height = (viewport_h * scale).clamp(3.0, panel_track_h);
                let top = MINIMAP_PANEL_INSET + (panel_track_h - height) / 2.0;
                let focused = active_row && index == self.active;
                let (state, todo_progress) = {
                    let panel = self.panel_metadata(&self.slots[index], cx);
                    let progress = panel.latest_todo_progress().and_then(|(done, total)| {
                        (total > 0).then_some(done as f32 / total as f32)
                    });
                    (panel.minimap_state, progress)
                };
                let (state_name, state_color) = match state {
                    crate::panel::MinimapSessionState::Idle => {
                        ("idle", Theme::global().MINIMAP_PANEL)
                    }
                    crate::panel::MinimapSessionState::Working => ("working", Theme::global().WARN),
                    crate::panel::MinimapSessionState::Streaming => {
                        ("streaming", Theme::global().ACCENT)
                    }
                    crate::panel::MinimapSessionState::Complete => ("complete", Theme::global().OK),
                    crate::panel::MinimapSessionState::Error => ("error", Theme::global().ERROR),
                };
                track =
                    track.child(
                        div()
                            .id(("minimap-panel", index))
                            .debug_selector(move || format!("minimap-panel-{index}"))
                            .when(cfg!(test) && focused, |el| {
                                el.child(div().absolute().size_full().debug_selector(move || {
                                    format!("minimap-panel-{index}-focused")
                                }))
                            })
                            .absolute()
                            .left(px(left))
                            .top(px(top))
                            .w(px(width))
                            .h(px(height))
                            .rounded(px(2.0))
                            .cursor_pointer()
                            .bg(if focused {
                                Theme::global().MINIMAP_PANEL_BUSY
                            } else {
                                Theme::global().MINIMAP_PANEL
                            })
                            .child(
                                div()
                                    .debug_selector(move || {
                                        format!("minimap-panel-{index}-{state_name}")
                                    })
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w_full()
                                    .h(px(2.0))
                                    .bg(state_color),
                            )
                            // The green footline is a literal completion meter for
                            // the latest todo card. Session color remains visible
                            // above it, so progress and live state do not compete.
                            .when_some(todo_progress, |panel, progress| {
                                panel.child(
                                    div()
                                        .debug_selector(move || {
                                            format!("minimap-panel-{index}-todo-progress")
                                        })
                                        .absolute()
                                        .bottom_0()
                                        .left_0()
                                        .h(px(2.0))
                                        .w(relative(progress))
                                        .bg(Theme::global().OK),
                                )
                            })
                            .hover(|el| el.bg(Theme::global().MINIMAP_PANEL_BUSY))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _event, window, cx| {
                                    cx.stop_propagation();
                                    this.set_active(index, cx);
                                    this.overview = false;
                                    this.overview_progress.set(0.0, Instant::now());
                                    this.focus_active(window, cx);
                                    cx.notify();
                                }),
                            ),
                    );
            }

            // The lens: where the camera is looking on the active strip.
            if active_row {
                let lens_left = ((self.camera_x[row] + STRUT) * scale).max(0.0);
                let lens_width = (viewport_w * scale).min(track_w - lens_left).max(3.0);
                track = track.child(
                    div()
                        .debug_selector(|| "minimap-viewport".into())
                        .absolute()
                        .left(px(lens_left))
                        .top(px(0.0))
                        .w(px(lens_width))
                        .h(px(MINIMAP_ROW_HEIGHT))
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(accent),
                );

                // A persistent pin marks the exact focused panel. Unlike the
                // viewport lens, it remains unambiguous when several panels
                // overlap the visible camera region.
                if let Some(index) = self.row_indices(row).find(|index| *index == self.active) {
                    let panel_left = self.slot_left(index, viewport_w) * scale;
                    let panel_width = (self.slot_width(index, viewport_w) * scale - 1.0).max(2.0);
                    track = track.child(
                        div()
                            .debug_selector(|| "minimap-you-pin".into())
                            .absolute()
                            .left(px(panel_left + panel_width / 2.0 - 2.0))
                            .top(px(MINIMAP_ROW_HEIGHT / 2.0 - 2.0))
                            .w(px(4.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(Theme::global().TEXT),
                    );
                }

                // The gesture dot: the same focal point the canvas reticle
                // marks, mirrored onto the map at the lens center so the eye
                // can track the swipe in either place. The dark ring keeps it
                // legible even over the lit focused-panel rectangle.
                if let Some(alpha) = self.gesture_reticle_alpha(cx) {
                    let dot_left = lens_left + lens_width / 2.0 - MINIMAP_GESTURE_DOT / 2.0;
                    track = track.child(
                        div()
                            .debug_selector(|| "minimap-gesture-dot".into())
                            .absolute()
                            .left(px(dot_left))
                            .top(px((MINIMAP_ROW_HEIGHT - MINIMAP_GESTURE_DOT) / 2.0))
                            .w(px(MINIMAP_GESTURE_DOT))
                            .h(px(MINIMAP_GESTURE_DOT))
                            .rounded_full()
                            .border_2()
                            .border_color(gpui::rgba(0x000000cc))
                            .bg(Theme::global().ACCENT)
                            .opacity(alpha),
                    );
                }
            }

            card = card.child(track);
        }

        card.into_any_element()
    }

    /// A single skill row: the keys, what they do, and a bar showing how well
    /// the model believes this shortcut is known right now.
    pub(super) fn render_skill_row(&self, skill: &learning::Skill, mastery: f32) -> gpui::AnyElement {
        let known = mastery >= 0.7;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .font_family(Theme::global().FONT_MONO)
                    .text_color(if known {
                        Theme::global().TEXT_DIM
                    } else {
                        Theme::global().TEXT
                    })
                    .child(skill.keys),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.0))
                    .text_color(if known {
                        Theme::global().TEXT_DIM
                    } else {
                        Theme::global().TEXT
                    })
                    .child(skill.label),
            )
            // The bar is the model's belief, not a usage count: it decays when
            // a shortcut goes unused, so it reads as "how well you know this".
            .child(
                div()
                    .w(px(56.0))
                    .h(px(4.0))
                    .flex_none()
                    .rounded_full()
                    .bg(Theme::global().PANEL_BORDER)
                    .child(
                        div()
                            .w(relative(mastery.clamp(0.02, 1.0)))
                            .h_full()
                            .rounded_full()
                            .bg(if known {
                                Theme::global().OK
                            } else {
                                Theme::global().ACCENT
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The coach view: what the user knows, what they do not, and what to learn
    /// next. This replaces a flat cheat sheet, because the useful information is
    /// which of these the user has not yet made their own.
    pub(super) fn render_hints_overlay(&self, progress: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let now = learning::now();
        let overall = self.coach.overall_mastery(now);
        let next = self.coach.next_lesson(now);

        let mut card = div()
            .id("hints-card")
            .debug_selector(|| "coach-card".into())
            .relative()
            .top(px((1.0 - progress) * 12.0))
            .w(px(560.0))
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .bg(Theme::global().PANEL_BG)
            .border_1()
            .border_color(Theme::global().PANEL_BORDER_FOCUS)
            .rounded_xl()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_size(px(18.0)).child("Your workspace fluency"))
                    .child(
                        div()
                            .text_color(Theme::global().TEXT_DIM)
                            .text_size(px(11.0))
                            .child("Super+/ or F1 to close"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .text_size(px(11.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(format!("{}% learned", (overall * 100.0).round() as u32))
                    .child(format!("{} keystrokes saved", self.coach.effort_saved))
                    .child(format!("{} spent the long way", self.coach.effort_wasted)),
            );

        if let Some(next) = next {
            card = card.child(
                div()
                    .p_2p5()
                    .rounded_lg()
                    .bg(Theme::global().HEADER_BG)
                    .border_1()
                    .border_color(Theme::global().PANEL_BORDER)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Theme::global().TEXT_DIM)
                            .child("learn next"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .font_family(Theme::global().FONT_MONO)
                                    .text_size(px(12.0))
                                    .child(next.keys),
                            )
                            .child(div().text_size(px(12.0)).child(next.label)),
                    ),
            );
        }

        card = card.child(
            div()
                .p_2p5()
                .rounded_lg()
                .bg(Theme::global().HEADER_BG)
                .text_size(px(11.0))
                .text_color(Theme::global().TEXT_DIM)
                .child("F2 rename session · Composer: ↑/↓ history · Ctrl+K/J prompts · Ctrl+W word delete · Alt+B/F word move · Ctrl+U delete to start · Ctrl/Cmd+Z undo · Esc clear"),
        );

        for (area, rows) in self.coach.report(now) {
            card = card.child(
                div()
                    .mt_1()
                    .text_size(px(10.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(area.label()),
            );
            for (skill, mastery) in rows {
                card = card.child(self.render_skill_row(skill, mastery));
            }
        }

        card = card.child(
            div()
                .id("new-help-session")
                .mt_3()
                .p_3()
                .rounded_lg()
                .bg(Theme::global().HEADER_BG)
                .border_1()
                .border_color(Theme::global().PANEL_BORDER)
                .cursor_pointer()
                .hover(|el| el.border_color(Theme::global().ACCENT))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _event, window, cx| {
                        this.new_help_session(&NewHelpSession, window, cx);
                    }),
                )
                .child("Ask Jcode about the app")
                .child(
                    div()
                        .mt_1()
                        .text_size(px(11.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Opens a new session with the hints and bundled docs loaded into context."),
                ),
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .opacity(progress)
            .bg(gpui::rgba(0x000000b8))
            .child(card)
            .into_any_element()
    }
}
