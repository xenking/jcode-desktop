//! Composer chrome: the input with one row of compact pills below it.
//!
//! Nothing sits above the input. The bottom row holds the model pill, a
//! separate reasoning effort pill, the credential method pill and the context
//! ring on the left, and the publish button plus the location (repo or
//! directory) on the right. The voice button lives inside the input box.
//! Pills are detached from the input and shaped like the transcript's user
//! prompt cards, so nothing reads as a folder tab.
use super::*;

/// Pill height.
pub(super) const TAB_HEIGHT: f32 = 22.;
/// One size for every drawn mark in the bar: the context ring, the effort
/// circle, the worker cluster, the wait mark and the cache timer.
pub(crate) const BAR_ICON: f32 = 13.;
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
            .filter(|effort| !effort.is_empty() && model.is_some())
            .map(str::to_string);
        (pretty_model_label(model), effort)
    }

    /// Prompt input with its pill row below. Every composer location
    /// (fresh session, startup layout, docked) uses this so the pills never
    /// drift apart from the input they label.
    pub(super) fn render_composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let _ = window;
        let theme = Theme::global();
        let screenshot_mode = crate::config::display().screenshot_mode;
        let (model_label, effort) = self.composer_identity();
        let remote_machine = (!screenshot_mode)
            .then(|| crate::harness::remote_host(&self.session_id))
            .flatten()
            .map(|host| {
                if host == crate::managed_cloud::HOST {
                    "Jcode Cloud".to_string()
                } else {
                    host
                }
            });
        let location = self
            .working_dir
            .as_deref()
            .filter(|dir| !screenshot_mode && !dir.is_empty())
            .map(|dir| {
                if remote_machine.is_some() {
                    remote_location_label(dir)
                } else {
                    location_label(dir)
                }
            });
        let can_switch_location = remote_machine.is_none() && self.offers_directory_switch();
        // Usage limits and status share the pill row, so the panel needs no
        // separate footer bar below the composer. The build label is gone from
        // the footer entirely: the version lives in Settings → About and the
        // account popup.
        let status_line = self.status_line();
        // Worker count (only while workers run), the prompt-cache countdown,
        // then the activity icon. All three are leaf views, so their animation
        // and ticks never rebuild the composer.
        let workers_pill = self.render_swarm_indicator(cx);
        let cache_pill = self.render_cache_pill(cx);
        let activity_icon = self.render_activity_icon(cx);
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
            .text_color(theme.TEXT_FAINT)
            .child(
                div()
                    .debug_selector(|| "panel-status".into())
                    .min_w_0()
                    .flex_shrink_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .children(workers_pill)
                    .children(cache_pill)
                    .children(activity_icon)
                    .children(self.render_voice_status(status_line))
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
                    .child(
                        composer_pill("panel-model")
                            .flex_shrink_1()
                            .min_w(px(56.))
                            .child(
                                div()
                                    .debug_selector(|| "panel-model-name".into())
                                    .min_w_0()
                                    .truncate()
                                    .child(model_label),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_model_picker(window, cx);
                                cx.stop_propagation();
                            })),
                    )
                    .children(effort.map(|effort| {
                        let hover = crate::effort::tooltip(&effort);
                        // The level is a drawn circle the size of the context
                        // ring beside it, filled to the rung and heat-tinted.
                        // An unknown value keeps its word.
                        let mark = crate::effort_icon::icon(&effort, BAR_ICON);
                        let word = mark.is_none().then(|| {
                            div()
                                .min_w_0()
                                .truncate()
                                .child(crate::effort::label(&effort).to_string())
                        });
                        composer_pill("panel-model-effort")
                            .flex_shrink(3.)
                            .px_1p5()
                            .children(mark)
                            .children(word)
                            .tooltip(move |_, cx| cx.new(|_| EffortTooltip(hover.clone())).into())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_effort_picker(window, cx);
                                cx.stop_propagation();
                            }))
                    }))
                    .children(self.render_context_meter()),
            )
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
                    .children(self.render_publish_button(cx))
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
                    .children(location.map(|(name, detail)| {
                        div()
                            .id("composer-location")
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
                            // The folder browser reads this computer's disk,
                            // so only local sessions offer the switch.
                            .when(can_switch_location, |el| {
                                el.cursor_pointer()
                                    .hover(|el| el.bg(theme.USER_BG))
                                    .tooltip(|_, cx| cx.new(|_| LocationTooltip).into())
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
                                    .text_color(theme.TEXT_DIM)
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .children(detail.map(|detail| {
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(10.5))
                                    .font_family(theme.FONT_MONO)
                                    .text_color(theme.TEXT_FAINT)
                                    .child(detail)
                            }))
                    })),
            );
        let strips = self.render_composer_applets(window, cx);
        div()
            .debug_selector(|| "composer".into())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .children(self.render_swarm_list(cx))
            .children(strips)
            .children(self.render_provider_picker(window, cx))
            .children(self.picker_search.as_ref().map(|(_, search)| {
                div()
                    .debug_selector(|| "picker-search".into())
                    .child(search.clone())
            }))
            .child(
                // The voice button floats inside the input's right edge. The
                // input reserves matching trailing space for it.
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(self.render_voice_input_slot(cx))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(VOICE_INSET))
                            .flex()
                            .items_center()
                            .child(self.render_voice_tab(cx)),
                    ),
            )
            .child(pills)
            .into_any_element()
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
        if self.ask_card.is_some() && self.input.read(cx).focus_handle.is_focused(window) {
            let panel = panel.clone();
            window.defer(cx, move |window, cx| {
                if let Some(panel) = panel.upgrade() {
                    let focus = {
                        let panel = panel.read(cx);
                        (panel.ask_card.is_some()
                            && panel.input.read(cx).focus_handle.is_focused(window))
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
                        if this.ask_card.is_some() {
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
                    .pb_2()
                    .bg(veil(DOCK_VEIL_ALPHA))
                    .when(self.recovery_picker_open, |el| {
                        el.child(self.render_recovery_model_picker(cx))
                    })
                    .children(self.render_voice_overlay(window, cx))
                    .children(self.render_preview_badge(cx))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .max_w(px(1120.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(self.render_ask_card(window, cx))
                            .children(self.render_side_question_card(cx))
                            .children(if self.ask_card.is_none() {
                                self.render_prompt_queue(cx)
                            } else {
                                None
                            })
                            .children(self.render_voice_pending())
                            .children(
                                self.ask_card
                                    .is_none()
                                    .then(|| self.render_composer(window, cx)),
                            )
                            .when(self.ask_card.is_none(), |el| {
                                el.child(startup::input_marker(input_bounds))
                            }),
                    ),
            )
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, _, cx| {
                        // Measure after paint. The fade is see-through, so
                        // only the solid part below it reserves list space.
                        let height = (f32::from(bounds.size.height) - DOCK_FADE).max(0.).round();
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
        let instances: Vec<String> = crate::applet_runtime::get(cx)
            .host
            .borrow()
            .composer_for(&self.session_id)
            .map(|mounted| mounted.instance.id.clone())
            .collect();
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
struct LocationTooltip;

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
            .child("Switch folder")
    }
}

/// Hover for the effort pill: the pill shows the dot alone, so the level name
/// has to live here.
struct EffortTooltip(String);

impl Render for EffortTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("EffortTooltip");
        div()
            .debug_selector(|| "composer-effort-tooltip".into())
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
            && self.items.is_empty()
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
        self.status = "Starting session · you can type now".into();
        self.connection_phase.clear();
        self.history_loaded = false;
        self.input
            .update(cx, |input, cx| input.set_pending_session(true, cx));
        cx.notify();
        previous
    }
}

pub(super) fn composer_pill(id: &'static str) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(TAB_HEIGHT))
        .px_2p5()
        .flex()
        .items_center()
        .gap_1p5()
        .rounded_full()
        .bg(theme.prompt_background(usize::MAX))
        .text_size(px(10.5))
        .font_family(theme.FONT_MONO)
        .text_color(theme.TEXT_DIM)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|el| el.text_color(theme.TEXT).bg(theme.USER_BG))
}

/// `claude-opus-4-8` reads `Opus 4.8`. The provider already appears in the
/// method pill, so the redundant `Claude` family prefix is dropped, matching
/// the TUI's compact model label.
pub(super) fn pretty_model_label(model: Option<&str>) -> String {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return "Choose model".into();
    };
    let pretty = jcode_provider_core::model_names::pretty_model_display_name(model);
    match pretty.strip_prefix("Claude ") {
        Some(rest) if !rest.trim().is_empty() => rest.to_string(),
        _ => pretty,
    }
}

/// Repo name (the last path component) plus its home-relative parent path.
/// `/home/me/src/jcode` reads `jcode` then `~/src`. Windows paths split on
/// either separator, so `C:\Users\me\src\jcode` reads `jcode` then `~\src`.
pub(super) fn location_label(path: &str) -> (String, Option<String>) {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return ("/".into(), None);
    }
    if crate::platform_dirs::tilde(trimmed) == "~" {
        return ("~".into(), None);
    }
    let (parent, name) = match trimmed.rsplit_once(['/', '\\']) {
        Some((parent, name)) => (parent, name),
        None => return (trimmed.into(), None),
    };
    // `C:\` keeps its separator so the parent of `C:\repo` is not `C:`.
    let parent = if parent.is_empty() {
        "/".to_string()
    } else if parent.ends_with(':') {
        trimmed[..parent.len() + 1].to_string()
    } else {
        crate::platform_dirs::tilde(parent)
    };
    (name.to_string(), Some(parent))
}

/// Repo name plus its full parent path on another machine. The local `$HOME`
/// says nothing about the remote user's home, so nothing is abbreviated
/// except the managed cloud's own home directory.
pub(super) fn remote_location_label(path: &str) -> (String, Option<String>) {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return ("/".into(), None);
    }
    let cloud_home = "/home/ec2-user";
    if trimmed == cloud_home {
        return ("~".into(), None);
    }
    let Some((parent, name)) = trimmed.rsplit_once('/') else {
        return (trimmed.into(), None);
    };
    let parent = if parent.is_empty() { "/" } else { parent };
    let parent = match parent.strip_prefix(cloud_home) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => parent.to_string(),
    };
    (name.to_string(), Some(parent))
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
    /// shape sits in the pill at the context ring's size, and an unknown
    /// level keeps its word instead of inventing a rung.
    #[gpui::test]
    fn the_effort_pill_draws_the_level_at_the_context_ring_size(cx: &mut gpui::TestAppContext) {
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
            let ring = vcx.debug_bounds("panel-context-ring").expect("ring");
            assert!(icon.left() >= pill.left() && icon.right() <= pill.right());
            assert_eq!(icon.size, ring.size, "{level} matches the context ring");
        }
        assert!(crate::effort_icon::icon("future", BAR_ICON).is_none());
    }

    #[test]
    fn remote_location_keeps_the_remote_path() {
        assert_eq!(
            remote_location_label("/home/ec2-user/workspaces"),
            ("workspaces".into(), Some("~".into()))
        );
        assert_eq!(
            remote_location_label("/home/ec2-user/workspaces/app/"),
            ("app".into(), Some("~/workspaces".into()))
        );
        assert_eq!(
            remote_location_label("/srv/app"),
            ("app".into(), Some("/srv".into()))
        );
        assert_eq!(remote_location_label("/home/ec2-user"), ("~".into(), None));
        assert_eq!(
            remote_location_label("/home/ec2-userx/a"),
            ("a".into(), Some("/home/ec2-userx".into()))
        );
    }

    #[test]
    fn location_label_splits_repo_name_from_parent() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            location_label(&format!("{home}/jcode")),
            ("jcode".into(), Some("~".into()))
        );
        assert_eq!(
            location_label(&format!("{home}/src/jcode/")),
            ("jcode".into(), Some("~/src".into()))
        );
        assert_eq!(location_label(&home), ("~".into(), None));
        assert_eq!(
            location_label("/srv/app"),
            ("app".into(), Some("/srv".into()))
        );
        assert_eq!(location_label("/"), ("/".into(), None));
        assert_eq!(
            location_label(r"D:\work\jcode\"),
            ("jcode".into(), Some(r"D:\work".into()))
        );
        assert_eq!(
            location_label(r"C:\repo"),
            ("repo".into(), Some(r"C:\".into()))
        );
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
                panel.working_dir = Some("/srv/projects/jcode".into());
                *panel.items = vec![Item::User("Hello".into())];
                cx.notify();
            });
            vcx.run_until_parked();
            let input = vcx.debug_bounds("prompt-input").unwrap();
            let editor = vcx.debug_bounds("prompt-editor").unwrap();
            let model = vcx.debug_bounds("panel-model").unwrap();
            let name = vcx.debug_bounds("panel-model-name").unwrap();
            let effort = vcx.debug_bounds("panel-model-effort").unwrap();
            let context = vcx.debug_bounds("panel-context-meter").unwrap();
            let voice = vcx.debug_bounds("voice-toggle").unwrap();
            for (tab_name, tab) in [("model", model), ("context", context)] {
                assert!(
                    tab.top() > input.bottom(),
                    "{tab_name} pill sits below the input at {width}: {tab:?} {input:?}"
                );
                assert!(tab.left() >= input.left() && tab.right() <= input.right());
            }
            assert!(name.right() <= model.right());
            assert!(model.right() <= effort.left(), "effort follows the model");
            // Usage left the bar: the context ring now follows the effort
            // glyph directly, and the cache countdown lives in the status row.
            assert!(
                effort.right() <= context.left(),
                "context ring follows effort"
            );
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
}
