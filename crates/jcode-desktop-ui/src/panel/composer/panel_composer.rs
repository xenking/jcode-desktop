//! Composer chrome: the input with one row of compact pills below it.
//!
//! Mode, model and effort, and context are on the left. Location is on the
//! right. The voice button lives inside the input box.
//! Pills are detached from the input and shaped like the transcript's user
//! prompt cards, so nothing reads as a folder tab, but they are filled with a
//! lift of the pane surface: prompt paper is only a few steps away from the
//! focused pane's background in several palettes, and a pill that vanishes on
//! the pane you are typing in is not a pill.
use super::*;
use super::chrome::{BAR_ICON, EffortTooltip, OVERLAY_GAP, TAB_HEIGHT, composer_button, composer_pill, pretty_model_label};

/// Gap between the input and the pill row below it.
const PILL_GAP: f32 = 4.;
/// Distance from the input's right edge to the in-box voice button.
const VOICE_INSET: f32 = 8.;
/// Height of the soft ramp above the floating composer.
const DOCK_FADE: f32 = 28.;
/// Opacity of the veil behind the composer. High enough that history behind
/// the controls is a quiet hint, never competing with the draft.
const DOCK_VEIL_ALPHA: f32 = 0.86;
/// Right padding the input reserves so text never runs under the voice
/// button. Linux shows a single-glyph keycap; other platforms spell out the
/// chord, so their button is wider.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) const VOICE_TRAILING_SPACE: f32 = 44.;
#[cfg(target_os = "macos")]
pub(super) const VOICE_TRAILING_SPACE: f32 = 52.;
#[cfg(target_os = "windows")]
pub(super) const VOICE_TRAILING_SPACE: f32 = 128.;

impl Panel {
    /// The composer's identity pills: the model label and the effort, from the
    /// runtime when it has answered and from the session record otherwise.
    ///
    /// A headless swarm worker's panel is served by its daemon, which answers
    /// no model probe on attach. The record already names both, so a worker
    /// reads "DeepSeek V4.1 Flash · high" instead of "Choose model" with no
    /// effort at all. The runtime always wins once it says anything.
    pub(super) fn composer_identity(&self) -> (String, Option<String>) {
        let record = self.worker_record.as_ref();
        let model = self
            .model
            .as_deref()
            .or_else(|| record.and_then(|record| record.model.as_deref()));
        let effort = self
            .reasoning_effort
            .as_deref()
            .or_else(|| record.and_then(|record| record.effort.as_deref()))
            .map(str::trim)
            .filter(|effort| {
                !effort.is_empty() && (model.is_some() || self.draft_config.effort.is_some())
            })
            .map(str::to_string);
        (pretty_model_label(model), effort)
    }

    #[cfg(test)]
    pub(crate) fn test_composer_identity(&self) -> (String, Option<String>) {
        self.composer_identity()
    }

    /// Prompt input with its pill row below. Every composer location
    /// (fresh session, startup layout, docked) uses this so the pills never
    /// drift apart from the input they label.
    pub(super) fn render_composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let screenshot_mode = crate::config::display().screenshot_mode
            && !crate::fixture_control_targets::draft_session();
        let (mut model_label, effort) = self.composer_identity();
        if self.session_deferred() && self.model.is_none() {
            model_label = "Models".into();
        }
        let effort = effort.or_else(|| {
            (self.session_deferred() && self.model.is_some() && !self.effort_ladder().is_empty())
                .then(|| "default".to_owned())
        });
        let remote_machine = (!screenshot_mode)
            .then(|| crate::harness::remote_host(&self.session_id))
            .flatten()
            .map(|host| {
                crate::managed_cloud::Destination::parse(&host)
                    .map(|target| target.label().to_owned())
                    .unwrap_or(host)
            });
        let location = self
            .working_dir
            .as_deref()
            .filter(|dir| !screenshot_mode && !dir.is_empty())
            .map(|dir| (location_label(dir), dir.to_owned()));
        let can_switch_location = remote_machine.is_none() && self.offers_directory_switch();
        let location = location.or_else(|| {
            (can_switch_location && self.session_deferred() && !screenshot_mode)
                .then(|| ("Choose folder".into(), "Choose a working folder".into()))
        });
        let workers_pill = self.render_swarm_indicator(cx);
        let cache_pill = self.render_cache_pill(cx);
        let rig_hud = self.render_rig_hud(cx);
        let activity_icon = self.render_session_status(cx);
        let routing_chip = self.render_routing_chip(cx);
        let meta = div()
            .debug_selector(|| "panel-meta".into())
            .flex_shrink(4.)
            .min_w_0()
            .h(px(TAB_HEIGHT))
            .px_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .flex_nowrap()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(10.0))
            .font_family(theme.FONT_MONO)
            .text_color(theme.composer_secondary())
            .child(
                div()
                    .debug_selector(|| "panel-status".into())
                    .min_w_0()
                    .flex_shrink_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .children(routing_chip)
                    .children(workers_pill)
                    .children(rig_hud)
                    .children(cache_pill)
                    .child(activity_icon)
                    .children(self.render_image_pane_toggle(cx)),
            );
        let pills = div()
            .debug_selector(|| "composer-tabs".into())
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .mt(px(PILL_GAP))
            .child(
                div()
                    .debug_selector(|| "panel-identity".into())
                    // Auto basis: the pills claim their width before the
                    // trailing directory, which truncates first.
                    .flex_auto()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .children((self.preview_state.is_none() || !self.mode_names().is_empty() || !self.skills.is_empty() || self.session_deferred()).then(|| {
                        let names = self.mode_names();
                        let hover = if names.is_empty() {
                            "Add context".to_owned()
                        } else {
                            format!("Active modes: {}", names.join(", "))
                        };
                        let icons = names.iter().enumerate().map(|(index, name)| {
                            let icon = crate::mode_catalog::icon(name, self.mode_catalog.get(name));
                            div()
                                .debug_selector(move || format!("panel-mode-icon-{}-{index}", icon.name()))
                                .flex_none()
                                .flex()
                                .child(icon.render(BAR_ICON, theme.composer_secondary()))
                        });
                        crate::keyboard::popover_trigger(composer_pill("panel-mode"), &self.picker_triggers[PickerKind::Context as usize])
                            .relative()
                            .children(crate::fixture_control_targets::marker(|| "panel-mode".into(), || Some(self.session_id.clone())))
                            .flex_shrink(3.)
                            .min_w(px(TAB_HEIGHT))
                            .max_w(relative(0.4))
                            .overflow_hidden()
                            .child(div().debug_selector(|| "composer-context-plus".into()).text_size(px(17.)).child("+"))
                            .children(icons)
                            .tooltip(move |_, cx| cx.new(|_| EffortTooltip(hover.clone())).into())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_context_picker(window, cx);
                                this.remember_picker_trigger(PickerKind::Context);
                                cx.stop_propagation();
                            }))
                    }))
                    .child(self.render_model_control(model_label, effort, cx))
                    .children(self.render_pending_model_chip(cx))
                    .children(self.render_context_meter(cx)),
            )
            .when(!self.transcript_only && !self.is_pending_session(), |el| {
                el.child(self.render_clean_chat_toggle(cx))
            })
            .child(
                div()
                    .debug_selector(|| "composer-trailing".into())
                    // The directory yields space before the identity pills.
                    .flex_shrink(8.)
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .overflow_hidden()
                    .child(meta)
                    .children(remote_machine.map(|machine| {
                        // Remote sessions name their machine first, so a cloud
                        // chat never reads like a local one.
                        div()
                            .id("composer-machine")
                            .debug_selector(|| "composer-machine".into())
                            .flex_none()
                            .h(px(TAB_HEIGHT))
                            .px_2p5()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded_full()
                            .bg(theme.ACCENT_DIM)
                            .text_color(theme.ACCENT)
                            .text_size(px(10.5))
                            .font_family(theme.FONT_MONO)
                            .whitespace_nowrap()
                            .child(machine)
                    }))
                    .children(location.map(|(name, full_path)| {
                        div()
                            .id("composer-location")
                            .relative()
                            .children(crate::fixture_control_targets::marker(|| "composer-location".into(), || Some(self.session_id.clone())))
                            .debug_selector(|| "composer-location".into())
                            .flex_shrink_1()
                            .min_w_0()
                            .h(px(TAB_HEIGHT))
                            .px_2()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.5))
                            .rounded_full()
                            .tooltip(move |_, cx| {
                                cx.new(|_| LocationTooltip(full_path.clone())).into()
                            })
                            // The folder browser reads this computer's disk,
                            // so only local sessions offer the switch.
                            .when(can_switch_location, |el| {
                                el.cursor_pointer()
                                    .hover(|el| el.bg(theme.chip_active_background()))
                                    .on_click(cx.listener(|_, _, window, cx| {
                                        cx.stop_propagation();
                                        window.dispatch_action(
                                            Box::new(crate::ui_actions::SwitchDirectory {
                                                source: cx.entity_id(),
                                            }),
                                            cx,
                                        );
                                    }))
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(theme.composer_secondary())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(name),
                            )
                    })),
            );
        let strips = self.render_composer_applets(window, cx);
        let ask = self.render_ask_card(window, cx);
        let routing_gate = self.render_routing_gate_card(window, cx);
        if self.routing_gate_wants_focus(window, cx) {
            // Every composer layout shows the gate, so take the keyboard here.
            let panel = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let Some(panel) = panel.upgrade() else { return };
                let focus = {
                    let panel = panel.read(cx);
                    panel
                        .routing_gate_wants_focus(window, cx)
                        .then(|| panel.focus_handle.clone())
                };
                if let Some(focus) = focus {
                    window.focus(&focus, cx);
                }
            });
        }
        let picker_open = self.picker_search.is_some();
        let hide_voice = picker_open && !self.input.read(cx).has_user_draft();
        self.input
            .update(cx, |input, cx| input.set_external_picker_open(picker_open, cx));
        div()
            .debug_selector(|| "composer".into())
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .children(self.render_context_usage(cx))
            .children(self.render_swarm_list(window, cx))
            .children(strips)
            .children(ask)
            .children(routing_gate)
            .when(self.ask_card.is_none() && self.routing_gate.is_none(), |el| {
                el.child(
                    // The voice button floats inside the input's right edge. The
                    // input reserves matching trailing space for it.
                    div()
                        .debug_selector(|| "composer-text-slot".into())
                        .relative()
                        .w_full()
                        .min_w_0()
                        .child(self.render_voice_input_slot(cx))
                        .when(!hide_voice, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .right(px(VOICE_INSET))
                                    .flex()
                                    .items_center()
                                    .child(self.render_voice_tab(cx)),
                            )
                        }),
                )
            })
            .child(pills)
            .into_any_element()
    }

    fn render_model_control(
        &self,
        model_label: String,
        effort: Option<String>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        const MODEL_MIN: f32 = 56.;
        const EFFORT_MIN: f32 = BAR_ICON + 12.;
        const MODEL: &str = "panel-model";
        const EFFORT: &str = "panel-model-effort";
        let joined = effort.is_some();
        let speed = self.visible_speed();
        let accelerated = speed != jcode_provider_core::SpeedMode::Standard;
        let speed_width = if accelerated { 24. } else { 0. };
        let name = div()
            .debug_selector(|| "panel-model-name".into())
            .min_w_0()
            .truncate()
            .child(model_label);
        div()
            .debug_selector(|| "panel-model-control".into())
            .flex()
            .items_center()
            .flex_shrink_1()
            .min_w(px(MODEL_MIN
                + if joined { EFFORT_MIN } else { 0. }
                + speed_width))
            .rounded_full()
            .bg(Theme::global().chip_background())
            .child(
                crate::keyboard::popover_trigger(
                    composer_button(MODEL, joined || accelerated),
                    &self.picker_triggers[PickerKind::Model as usize],
                )
                .relative()
                .children(crate::fixture_control_targets::marker(
                    || MODEL.into(),
                    || Some(self.session_id.clone()),
                ))
                .flex_shrink_1()
                .min_w(px(MODEL_MIN))
                .when(joined || accelerated, |el| el.pl_4().pr_1())
                .child(if joined || accelerated {
                    composer_hover_content(MODEL, 3.).child(name)
                } else {
                    name
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_model_picker(window, cx);
                    this.remember_picker_trigger(PickerKind::Model);
                    cx.stop_propagation();
                })),
            )
            .children(effort.map(|effort| {
                let hover = crate::effort::tooltip(&effort);
                let mark = crate::effort_icon::icon(&effort, BAR_ICON);
                let word = mark.is_none().then(|| {
                    div().min_w_0().truncate().child(if effort == "default" {
                        "Effort".to_owned()
                    } else {
                        crate::effort::label(&effort).to_string()
                    })
                });
                crate::keyboard::popover_trigger(
                    composer_button(EFFORT, true),
                    &self.picker_triggers[PickerKind::Effort as usize],
                )
                .relative()
                .children(crate::fixture_control_targets::marker(
                    || EFFORT.into(),
                    || Some(self.session_id.clone()),
                ))
                .flex_shrink(3.)
                .min_w(px(EFFORT_MIN))
                .gap_1()
                .pl_0p5()
                .when(accelerated, |el| el.pr_0p5())
                .when(!accelerated, |el| el.pr_2p5())
                .child(
                    composer_hover_content(EFFORT, 2.)
                        .children(mark)
                        .children(word),
                )
                .tooltip(move |_, cx| cx.new(|_| EffortTooltip(hover.clone())).into())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_effort_picker(window, cx);
                    this.remember_picker_trigger(PickerKind::Effort);
                    cx.stop_propagation();
                }))
            }))
            .children(accelerated.then(|| {
                let panel = cx.weak_entity();
                composer_button("panel-model-speed", true)
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || "panel-model-speed".into(),
                        || Some(self.session_id.clone()),
                    ))
                    .debug_selector(move || {
                        format!("speed-mode-{}", crate::speed_icon::label(speed))
                    })
                    .flex_none()
                    .w(px(speed_width))
                    .px_0()
                    .justify_center()
                    .child(crate::speed_icon::render(
                        speed,
                        BAR_ICON,
                        Theme::global().composer_secondary(),
                    ))
                    .tooltip(move |_, cx| {
                        let tooltip = panel.upgrade().map_or_else(String::new, |panel| {
                            format!(
                                "{} Click to use Standard.",
                                panel.read(cx).speed_picker().tooltip
                            )
                        });
                        cx.new(|_| crate::components::Tooltip::header_text(tooltip)).into()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.choose_speed(jcode_provider_core::SpeedMode::Standard, cx);
                        cx.stop_propagation();
                    }))
            }))
    }

    fn remember_picker_trigger(&mut self, kind: PickerKind) {
        self.composer_popover_trigger = self.picker_search.as_ref().and_then(|(opened, _)| {
            (*opened == kind).then(|| self.picker_triggers[kind as usize].clone())
        });
    }

    pub(super) fn render_composer_stack(
        &mut self,
        bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
        docked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let overlays = [
            self.render_side_question_card(window, cx),
            self.render_provider_picker(window, cx),
            self.picker_search.as_ref().map(|(_, search)| {
                div()
                    .id("picker-search")
                    .debug_selector(|| "picker-search".into())
                    .occlude()
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(search.clone())
                    .into_any_element()
            }),
        ];
        div()
            .relative()
            .w_full()
            .max_w(px(1120.))
            .when(docked, |el| el.mx_auto())
            .flex()
            .flex_col()
            .gap_2()
            .when(
                self.has_visible_queued_prompts() && (!docked || self.ask_card.is_none()),
                |el| el.gap_0(),
            )
            .children(self.render_prompt_queue(cx))
            .children(self.render_voice_pending())
            .child(self.render_composer(window, cx))
            .when(!docked || self.ask_card.is_none(), |el| {
                el.child(startup::input_marker(bounds))
            })
            // Keep both popover inputs and results out of the measured dock.
            .children(overlays.into_iter().flatten().map(|overlay| {
                gpui::deferred(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(gpui::relative(1.))
                        .mb(px(OVERLAY_GAP))
                        .child(overlay),
                )
            }))
    }

    /// The ordinary composer, floating over the bottom of the transcript.
    /// History scrolls beneath a translucent veil instead of stopping at a
    /// hard edge. The veil eases from clear to mostly opaque so text behind
    /// the controls reads as a soft, frosted hint rather than competing ink.
    /// Its measured height pads the list so the live end clears it.
    pub(super) fn render_composer_dock(
        &mut self,
        input_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let background = Theme::global().panel_background(self.surface_focused);
        let veil = |alpha: f32| gpui::Rgba {
            a: alpha,
            ..background
        };
        let panel = cx.entity().downgrade();
        let should_focus_ask = self.ask_card.as_ref().is_some_and(|card| {
            self.input.read(cx).is_focused(window)
                || (card.submitting() && self.ask_editor_focused(window, cx))
        });
        if should_focus_ask {
            let panel = panel.clone();
            window.defer(cx, move |window, cx| {
                if let Some(panel) = panel.upgrade() {
                    let focus = {
                        let panel = panel.read(cx);
                        panel
                            .ask_card
                            .as_ref()
                            .is_some_and(|card| {
                                panel.input.read(cx).is_focused(window)
                                    || (card.submitting() && panel.ask_editor_focused(window, cx))
                            })
                            .then(|| panel.focus_handle.clone())
                    };
                    if let Some(focus) = focus {
                        window.focus(&focus, cx);
                    }
                }
            });
        }
        div()
            .debug_selector(|| "composer-dock".into())
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .flex_col()
            // Do not let clicks reach transcript rows hidden under the veil,
            // but keep wheel and touchpad scrolling the transcript.
            .block_mouse_except_scroll()
            // Blocking the mouse also hides the press from the workspace, so
            // take the keyboard here. The workspace highlights whichever
            // panel owns the keyboard, which keeps the two in step.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.owns_keyboard_focus(window, cx) {
                        if this.ask_card.is_some() || this.routing_gate.is_some() {
                            window.focus(&this.focus_handle, cx);
                        } else {
                            this.focus_input(window, cx);
                        }
                    }
                }),
            )
            .child(
                div()
                    .debug_selector(|| "composer-veil".into())
                    .w_full()
                    .h(px(DOCK_FADE))
                    .bg(gpui::linear_gradient(
                        0.,
                        gpui::linear_color_stop(veil(DOCK_VEIL_ALPHA), 0.),
                        gpui::linear_color_stop(veil(0.), 1.),
                    )),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .pb(px(COMPOSER_BOTTOM_INSET))
                    .bg(veil(DOCK_VEIL_ALPHA))
                    .when(self.recovery_picker_open, |el| {
                        el.child(self.render_recovery_model_picker(cx))
                    })
                    .children(self.render_voice_overlay(window, cx))
                    .children(self.render_preview_badge(cx))
                    .child(self.render_composer_stack(input_bounds, true, window, cx)),
            )
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, _, cx| {
                        // The fade also intercepts clicks. Keep the transcript
                        // tail above the entire dock, not just the solid part.
                        let height = f32::from(bounds.size.height).round();
                        let panel = panel.clone();
                        cx.defer(move |cx| {
                            let Some(entity) = panel.upgrade() else {
                                return;
                            };
                            if (entity.read(cx).composer_dock_height - height).abs() < 0.5 {
                                return;
                            }
                            entity.update(cx, |panel, cx| {
                                panel.composer_dock_height = height;
                                cx.notify();
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }

    /// Composer-placed applet instances for this session, as a compact strip
    /// directly above the pills (suggestions, pickers).
    fn render_composer_applets(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let instances: Vec<String> =
            crate::applet_runtime::get(cx).composer_instance_ids(&self.session_id);
        if instances.is_empty() {
            return None;
        }
        let selection = self.applet_selection.clone();
        let cards: Vec<gpui::AnyElement> = instances
            .iter()
            .filter_map(|instance| {
                crate::applet_surface::card(instance, false, &selection, window, cx)
            })
            .collect();
        Some(
            div()
                .debug_selector(|| "composer-applets".into())
                .w_full()
                .min_w_0()
                .px_1()
                .mb(px(PILL_GAP))
                .flex()
                .flex_col()
                .gap_1()
                .children(cards)
                .into_any_element(),
        )
    }
}

/// A compact rounded pill, filled like the transcript's user prompt cards.
struct LocationTooltip(String);

impl Render for LocationTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("LocationTooltip");
        div()
            .debug_selector(|| "composer-location-tooltip".into())
            .max_w(px(300.))
            .p_2()
            .rounded_md()
            .bg(Theme::global().HEADER_BG)
            .text_size(px(12.))
            .text_color(Theme::global().TEXT)
            .child(self.0.clone())
    }
}


impl Panel {
    /// Chat panels on this computer can pick another folder from the pill.
    fn offers_directory_switch(&self) -> bool {
        self.preview_state.is_none()
            && !self.transcript_only
            && !self.is_side_document()
            && !self.is_publish_tracker()
            && crate::harness::remote_host(&self.session_id).is_none()
            && !self.session_id.starts_with("startup://draft/remote/")
            && (self.is_pending_session() || !self.session_id.contains("://"))
    }

    /// An untouched session has nothing tied to its folder yet, so it can be
    /// replaced by one in another folder without losing anything. History
    /// must have arrived first: an unloaded transcript also looks empty.
    pub(crate) fn can_switch_directory_in_place(&self) -> bool {
        self.offers_directory_switch()
            && (self.is_pending_session() || self.history_loaded)
            && !self.session_id.starts_with("startup://draft/help/")
            && !self.awaiting_persisted
            && !self.restored_transcript
            && self.provisional_items == 0
            && (self.session_deferred() || self.items.is_empty())
            && self.pending_users.is_empty()
            && !self.has_queued_prompts()
            && !self.activity_active()
    }

    /// Become a draft for a session in `directory`, keeping the editor and
    /// its text. Returns the identity this panel had before.
    pub(crate) fn retarget_draft(
        &mut self,
        request_id: String,
        directory: String,
        cx: &mut Context<Self>,
    ) -> String {
        let previous = std::mem::replace(&mut self.session_id, request_id);
        self.working_dir = Some(directory);
        self.title = "New session".into();
        self.draft_session = super::DraftSession::Deferred;
        self.status = super::DEFERRED_SESSION_STATUS.into();
        self.connection_phase.clear();
        self.history_loaded = false;
        let directory = self.working_dir.as_deref();
        self.input.update(cx, |input, cx| {
            input.set_pending_session(true, cx);
            input.set_working_dir(directory);
        });
        self.load_draft_skills(cx);
        cx.notify();
        previous
    }
}


// The segment owns hover, click and focus. Its absolute fill follows the
// content width without growing the layout or taking any event handlers.
fn composer_hover_content(group: &'static str, pad: f32) -> gpui::Div {
    const HEIGHT: f32 = BAR_ICON + 4.;
    div().relative().min_w_0().flex().child(
        div()
            .id("hover-fill")
            .absolute()
            .left(px(-pad))
            .right(px(-pad))
            .top(gpui::relative(0.5))
            .mt(px(-HEIGHT / 2.))
            .h(px(HEIGHT))
            .rounded_full()
            .group_hover(group, |el| el.bg(Theme::global().chip_active_background())),
    )
}


/// Last path component. The full path is kept in the hover card.
fn location_label(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return "/".into();
    }
    let name = match trimmed.rsplit_once(['/', '\\']) {
        Some((_, name)) => name,
        None => return trimmed.into(),
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A headless worker panel has no runtime identity, so its record's model
    /// and effort fill the footer. Without this the pills read "Choose model"
    /// and no effort at all, which is what "unset" looked like.
    #[gpui::test]
    fn a_worker_panel_takes_its_model_and_effort_from_its_record(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("worker-panel", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());

        let record = jcode_sdk::WorkerMeta {
            model: Some("deepseek-v4.1-flash".into()),
            provider: Some("opencode-go".into()),
            effort: Some("high".into()),
            ..jcode_sdk::WorkerMeta::default()
        };
        let identity = panel.update(vcx, |panel, cx| {
            panel.set_worker_record(Some(record.clone()), cx);
            panel.composer_identity()
        });
        assert_ne!(identity.0, "Choose model", "the record names the model");
        assert!(identity.0.contains("4.1"), "{}", identity.0);
        assert_eq!(identity.1.as_deref(), Some("high"));
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("panel-model-effort").is_some(),
            "the effort pill exists once a model is known"
        );

        // The runtime wins once it answers.
        let identity = panel.update(vcx, |panel, cx| {
            panel.model = Some("claude-opus-5-5".into());
            panel.reasoning_effort = Some("max".into());
            panel.set_worker_record(Some(record), cx);
            panel.composer_identity()
        });
        assert!(identity.0.contains("Opus 5.5"), "{}", identity.0);
        assert_eq!(identity.1.as_deref(), Some("max"));
    }

    /// The bar's effort mark is drawn (canvas), not a `◑` glyph: the level's
    /// shape sits in the pill at the shared icon size, and an unknown
    /// level keeps its word instead of inventing a rung.
    #[gpui::test]
    fn the_effort_pill_draws_the_level_at_the_bar_icon_size(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        for level in ["low", "medium", "high", "xhigh", "max"] {
            panel.update(vcx, |panel, cx| {
                panel.model = Some("gpt-5.6-sol".into());
                panel.provider = Some("openai".into());
                panel.reasoning_effort = Some(level.into());
                cx.notify();
            });
            vcx.run_until_parked();
            let pill = vcx.debug_bounds("panel-model-effort").expect("effort pill");
            let selector: &'static str = Box::leak(format!("effort-icon-{level}").into_boxed_str());
            let icon = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{level} icon"));
            assert!(icon.left() >= pill.left() && icon.right() <= pill.right());
            assert_eq!(icon.size, gpui::size(px(BAR_ICON), px(BAR_ICON)));
        }
        assert!(crate::effort_icon::icon("future", BAR_ICON).is_none());
    }

    #[gpui::test]
    fn joined_model_and_effort_stay_close_with_full_hit_targets(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        panel.update(vcx, |panel, cx| {
            panel.model = Some("claude-opus-5-5".into());
            panel.provider = Some("anthropic".into());
            panel.reasoning_effort = Some("high".into());
            cx.notify();
        });
        for width in [320., 1000.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(500.)));
            vcx.run_until_parked();
            let model = vcx.debug_bounds("panel-model").unwrap();
            let name = vcx.debug_bounds("panel-model-name").unwrap();
            let effort = vcx.debug_bounds("panel-model-effort").unwrap();
            let icon = vcx.debug_bounds("effort-icon-high").unwrap();
            assert_eq!(model.right(), effort.left());
            assert!(model.size.width >= px(56.));
            assert!(effort.size.width >= px(BAR_ICON + 12.));
            assert_eq!(model.size.height, px(TAB_HEIGHT));
            assert_eq!(effort.size.height, px(TAB_HEIGHT));
            assert!(icon.left() - name.right() <= px(8.), "width {width}");
        }
    }

    #[gpui::test]
    fn joined_picker_padding_and_keys_open_the_correct_picker(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (bridge, _) = crate::harness::spawn_recording();
        let (panel, vcx) = cx
            .add_window_view(|_, cx| Panel::new("composer-pickers".into(), None, None, bridge, cx));
        vcx.update(|_, cx| Panel::connect_input(&panel, cx));
        panel.update(vcx, |panel, cx| {
            panel.model = Some("claude-opus-5-5".into());
            panel.provider = Some("anthropic".into());
            panel.reasoning_effort = Some("high".into());
            panel.available_models = vec!["anthropic:claude-opus-5-5".into()];
            cx.notify();
        });
        vcx.run_until_parked();

        for (selector, kind, key) in [
            ("panel-model", PickerKind::Model, "enter"),
            ("panel-model-effort", PickerKind::Effort, "space"),
        ] {
            let segment = vcx.debug_bounds(selector).unwrap();
            let x = if kind == PickerKind::Model {
                segment.left() + px(7.)
            } else {
                segment.right() - px(5.)
            };
            vcx.simulate_click(point(x, segment.center().y), gpui::Modifiers::default());
            for opening in ["mouse", "keyboard"] {
                if opening == "keyboard" {
                    vcx.simulate_keystrokes(key);
                    vcx.simulate_event(gpui::KeyUpEvent {
                        keystroke: gpui::Keystroke::parse(key).unwrap(),
                    });
                }
                vcx.run_until_parked();
                vcx.update(|window, cx| {
                    let Some((opened, search)) = panel.read(cx).picker_search.as_ref() else {
                        panic!("{opening} did not open {selector}");
                    };
                    assert!(*opened == kind, "{selector} opened the other picker");
                    assert!(search.read(cx).is_focused(window));
                });
                vcx.simulate_keystrokes("escape");
                vcx.run_until_parked();
                vcx.update(|window, cx| {
                    let panel = panel.read(cx);
                    assert!(panel.picker_search.is_none());
                    assert!(panel.picker_triggers[kind as usize].is_focused(window));
                });
            }
        }
    }

    #[gpui::test]
    fn model_search_types_space_with_an_empty_composer(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        cx.update(super::super::shortcuts::bind_keys);
        let (bridge, _) = crate::harness::spawn_recording();
        let (panel, vcx) = cx
            .add_window_view(|_, cx| Panel::new("model-search".into(), None, None, bridge, cx));
        vcx.update(|_, cx| Panel::connect_input(&panel, cx));
        panel.update(vcx, |panel, cx| {
            panel.model = Some("gpt-5.6-sol".into());
            panel.provider = Some("openai".into());
            panel.reasoning_effort = Some("high".into());
            panel.available_models = vec!["openai-oauth:gpt-5.6-sol".into()];
            let route = jcode_sdk::ModelRouteInfo {
                model: "gpt-5.6-sol".into(),
                provider: "OpenAI".into(),
                api_method: "openai-oauth".into(),
                available: true,
                detail: String::new(),
                usage: None,
                speed_options: Vec::new(),
                reasoning_efforts: None,
            };
            panel.input.update(cx, |input, cx| {
                input.set_model_routes(Vec::new(), &[route], Some("gpt-5.6-sol".into()), cx);
            });
            cx.notify();
        });
        vcx.run_until_parked();
        let trigger = vcx.debug_bounds("panel-model").unwrap();
        vcx.simulate_click(trigger.center(), gpui::Modifiers::default());
        vcx.run_until_parked();

        let search = panel.read_with(vcx, |panel, _| {
            let (kind, search) = panel.picker_search.as_ref().expect("model search opened");
            assert!(*kind == PickerKind::Model);
            search.clone()
        });
        let check_query = |vcx: &mut gpui::VisualTestContext, keys, query| {
            vcx.update(|window, cx| assert!(search.read(cx).is_focused(window)));
            vcx.simulate_keystrokes(keys);
            assert_eq!(
                search.read_with(vcx, |search, _| search.content.to_string()),
                query,
                "typed {keys:?}"
            );
            search.update(vcx, |search, cx| search.set_content(String::new(), cx));
            vcx.run_until_parked();
        };
        check_query(vcx, "a space b", "a b");
        check_query(vcx, "space a", " a");
        check_query(vcx, "shift-space a", " a");

        vcx.simulate_keystrokes("down");
        vcx.run_until_parked();
        let manage = vcx.debug_bounds("slash-command-row-1").unwrap();
        let highlighted = vcx.debug_bounds("picker-keyboard-focus").unwrap();
        assert!(manage.contains(&highlighted.center()));
        check_query(vcx, "a space b", "a b");

        let model = vcx.debug_bounds("slash-command-row-0").unwrap();
        vcx.update(|window, cx| window.simulate_mouse_move(manage.center(), cx));
        vcx.run_until_parked();
        vcx.update(|window, cx| window.simulate_mouse_move(model.center(), cx));
        vcx.run_until_parked();
        let highlighted = vcx.debug_bounds("picker-keyboard-focus").unwrap();
        assert!(model.contains(&highlighted.center()));
        check_query(vcx, "a space b", "a b");

        let effort = vcx.debug_bounds("model-row-effort-0-medium").unwrap();
        vcx.simulate_click(effort.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        check_query(vcx, "a space b", "a b");

        let manage = vcx.debug_bounds("slash-command-row-1").unwrap();
        vcx.simulate_click(manage.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        check_query(vcx, "a space b", "a b");
    }

    #[test]
    fn location_label_only_shows_the_last_component() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(location_label(&format!("{home}/jcode")), "jcode");
        assert_eq!(location_label(&format!("{home}/src/jcode/")), "jcode");
        assert_eq!(location_label(&home), home.rsplit('/').next().unwrap());
        assert_eq!(location_label("/srv/app"), "app");
        assert_eq!(location_label("/"), "/");
        assert_eq!(location_label(r"D:\work\jcode\"), "jcode");
        assert_eq!(location_label(r"C:\repo"), "repo");
    }

    #[gpui::test]
    fn composer_location_shows_basename_and_keeps_full_path_for_hover(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.working_dir = Some("/srv/projects/jcode".into());
            cx.notify();
        });
        vcx.run_until_parked();
        let location = vcx
            .debug_bounds("composer-location")
            .expect("location visible");
        assert_eq!(location_label("/srv/projects/jcode"), "jcode");
        vcx.update(|window, cx| window.simulate_mouse_move(location.center(), cx));
        vcx.run_until_parked();
        vcx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-location-tooltip").is_some());
    }

    #[gpui::test]
    fn pills_sit_below_the_input_with_location_on_the_right(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [240., 480., 1440.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            panel.update(vcx, |panel, cx| {
                panel.model = Some("claude-opus-4-8".into());
                panel.reasoning_effort = Some("high".into());
                panel.provider = Some("anthropic".into());
                panel.auth_method = Some("oauth".into());
                panel.pinned_modes = vec!["selfdev".into(), "swarm".into()];
                panel.working_dir = Some("/srv/projects/jcode".into());
                *panel.items = vec![Item::User("Hello".into())];
                cx.notify();
            });
            vcx.run_until_parked();
            let input = vcx.debug_bounds("prompt-input").unwrap();
            let editor = vcx.debug_bounds("prompt-editor").unwrap();
            let identity = vcx.debug_bounds("panel-identity").unwrap();
            let mode = vcx.debug_bounds("panel-mode").unwrap();
            let control = vcx.debug_bounds("panel-model-control").unwrap();
            let model = vcx.debug_bounds("panel-model").unwrap();
            let name = vcx.debug_bounds("panel-model-name").unwrap();
            let effort = vcx.debug_bounds("panel-model-effort").unwrap();
            let context = vcx.debug_bounds("panel-context-meter").unwrap();
            let voice = vcx.debug_bounds("voice-toggle").unwrap();
            for (tab_name, tab) in [
                ("mode", mode),
                ("model and effort", control),
                ("context", context),
            ] {
                assert!(
                    tab.top() > input.bottom(),
                    "{tab_name} pill sits below the input at {width}: {tab:?} {input:?}"
                );
                assert!(tab.left() >= input.left() && tab.right() <= input.right());
            }
            assert_eq!(mode.left(), identity.left(), "mode is first at {width}");
            assert_eq!(
                control.left() - mode.right(),
                px(4.),
                "only the pill gap follows mode"
            );
            assert_eq!(model.left(), control.left());
            assert_eq!(
                model.right(),
                effort.left(),
                "segments share one surface"
            );
            assert_eq!(effort.right(), control.right());
            assert!(name.right() <= model.right());
            assert!(control.right() <= context.left());
            // The voice button lives inside the input box, on its right, and
            // the editor never runs underneath it.
            assert!(
                voice.left() >= input.left()
                    && voice.right() <= input.right()
                    && voice.top() >= input.top()
                    && voice.bottom() <= input.bottom(),
                "voice sits inside the input at {width}: {voice:?} {input:?}"
            );
            assert!(editor.right() <= voice.left(), "{editor:?} {voice:?}");
            if width >= 480. {
                let location = vcx.debug_bounds("composer-location").unwrap();
                assert!(
                    location.top() > input.bottom(),
                    "location is below the input"
                );
                assert!(context.right() <= location.left(), "location sits right");
                assert!(
                    input.right() - location.right() < px(12.),
                    "location hugs the right edge at {width}: {location:?} {input:?}"
                );
            }
        }
    }

    /// The model pill keeps the same padding and the same place whether its
    /// pane owns focus or not. Only the fill used to change, and once that fill
    /// matched the focused pane's background the label looked flush with the
    /// row's edge, as if the padding had gone. Pin the inset in both states so
    /// a later fill change cannot hide the padding again.
    #[gpui::test]
    fn the_model_pill_keeps_its_inset_when_the_pane_loses_focus(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.provider = Some("openai".into());
            panel.model = Some("gpt-6.2-sol".into());
            cx.notify();
        });
        for joined in [false, true] {
            let mut focused_bounds = None;
            for focused in [true, false] {
                panel.update(vcx, |panel, cx| {
                    panel.reasoning_effort = joined.then(|| "high".into());
                    panel.set_surface_focused(focused, cx);
                    cx.notify();
                });
                vcx.run_until_parked();
                let pill = vcx.debug_bounds("panel-model").expect("model pill");
                let name = vcx.debug_bounds("panel-model-name").expect("model name");
                assert!(
                    name.left() - pill.left() >= px(9.),
                    "focused={focused}: the label keeps its left padding: {name:?} in {pill:?}"
                );
                assert!(
                    pill.right() - name.right() >= px(if joined { 4. } else { 9. }),
                    "focused={focused} joined={joined}: expected right padding: {name:?} in {pill:?}"
                );
                match focused_bounds {
                    None => focused_bounds = Some((pill, name)),
                    Some(bounds) => assert_eq!((pill, name), bounds, "focus does not move the row"),
                }
            }
        }
    }
}
