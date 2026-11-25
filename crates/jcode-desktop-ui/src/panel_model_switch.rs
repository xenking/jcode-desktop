//! A model switch the daemon holds until the running turn ends.
//!
//! The model cannot change under a running turn, so the daemon answers a
//! mid-turn switch at once as queued and applies it afterwards. The chip keeps
//! showing the model the session runs now. The queued target is shown beside
//! it, with a way to cancel, and the transcript says when the switch landed.
use super::*;
use gpui::AnyElement;
use jcode_sdk::api::ModelSwitchState;

impl Panel {
    pub(super) fn apply_model_switch(
        &mut self,
        state: ModelSwitchState,
        model: Option<&str>,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        match state {
            ModelSwitchState::Queued => {
                self.pending_model = model.map(str::to_owned);
                let target = model.unwrap_or("the new model");
                let current = self.model.as_deref().unwrap_or("the current model");
                self.items.push(Item::Assistant(format!(
                    "Model switch to `{target}` is queued. It applies when this turn ends. \
                     Until then the session keeps running `{current}`."
                )));
            }
            ModelSwitchState::Applied => {
                self.pending_model = None;
                let target = model.or(self.model.as_deref()).unwrap_or("the new model");
                self.items
                    .push(Item::Assistant(format!("Switched to `{target}`.")));
            }
            ModelSwitchState::Cancelled => {
                let was_pending = self.pending_model.take();
                match model.map(str::to_owned).or(was_pending) {
                    Some(target) => self.items.push(Item::Assistant(format!(
                        "Model switch to `{target}` cancelled. The session stays on its current model."
                    ))),
                    None => self
                        .items
                        .push(Item::Assistant("No model switch was waiting.".into())),
                }
            }
            ModelSwitchState::Failed => {
                self.pending_model = None;
                self.items.push(Item::Error(format!(
                    "The queued model switch failed: {}",
                    error.unwrap_or("the model was not accepted")
                )));
            }
        }
        cx.notify();
    }

    /// Review fixture for `JCODE_DESKTOP_SCREENSHOT_MODEL_SWITCH`: `queued`
    /// holds a switch behind the turn, `switched` lands it after the cache
    /// figures were recorded on the previous model.
    pub(crate) fn apply_model_switch_fixture(&mut self, stage: &str, cx: &mut Context<Self>) {
        let session_id = self.session_id.clone();
        let target = "gpt-6.2-sol";
        let queued = ApiEvent::ModelSwitch {
            session_id: session_id.clone(),
            state: ModelSwitchState::Queued,
            model: Some(target.into()),
            error: None,
        };
        self.apply(&queued, cx);
        if stage != "switched" {
            return;
        }
        self.apply(
            &ApiEvent::ModelInfo {
                session_id: session_id.clone(),
                provider: Some("openai".into()),
                model: Some(target.into()),
                reasoning_effort: self.reasoning_effort.clone(),
                auth_method: None,
                account_label: None,
            },
            cx,
        );
        self.apply(
            &ApiEvent::ModelSwitch {
                session_id,
                state: ModelSwitchState::Applied,
                model: Some(target.into()),
                error: None,
            },
            cx,
        );
    }

    /// Withdraw the queued switch. The daemon answers with a cancelled event,
    /// which clears the chip.
    pub(super) fn cancel_pending_model(&mut self, cx: &mut Context<Self>) {
        if self.pending_model.is_none() {
            return;
        }
        self.bridge.send(Command::CancelPendingModel {
            session_id: self.session_id.clone(),
        });
        cx.notify();
    }

    /// The queued target as its own pill next to the model chip.
    pub(super) fn render_pending_model_chip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        let target = self.pending_model.as_deref()?;
        let label = format!("→ {} · queued", chrome::pretty_model_label(Some(target)));
        let hover = format!(
            "Switching to {target} when this turn ends. Click to keep the current model."
        );
        Some(
            chrome::composer_pill("panel-model-pending")
                .relative()
                .children(crate::fixture_control_targets::marker(
                    || "panel-model-pending".into(),
                    || Some(self.session_id.clone()),
                ))
                .flex_shrink(4.)
                .min_w_0()
                .overflow_hidden()
                .child(div().min_w_0().truncate().child(label))
                .tooltip(move |_, cx| cx.new(|_| chrome::EffortTooltip(hover.clone())).into())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cancel_pending_model(cx);
                    cx.stop_propagation();
                })),
        )
    }
}

/// The model list popover opened from the composer or from an error card.
impl Panel {
    pub(crate) fn toggle_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A draft borrows the list another session reported; opening it must
        // not create this one.
        self.seed_draft_models(cx);
        if self.recovery_picker_open {
            self.recovery_picker_open = false;
        } else if self.model_picker_open && self.picker_search.is_some() {
            self.close_model_picker(cx);
        } else {
            self.open_recovery_models(cx);
        }
        self.focus_input(window, cx);
        cx.notify();
    }

    pub(super) fn open_recovery_models(&mut self, cx: &mut Context<Self>) {
        self.login = None;
        if self.available_models.is_empty() {
            self.recovery_picker_open = true;
        } else {
            self.open_model_picker(cx);
        }
        self.focus_pending = true;
        cx.notify();
    }

    pub(super) fn render_recovery_model_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .debug_selector(|| "recovery-model-picker".into())
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1p5()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .bg(Theme::global().HEADER_BG)
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child("Choose a model")
                    .child(
                        div()
                            .id("recovery-model-close")
                            .debug_selector(|| "recovery-model-close".into())
                            .cursor_pointer()
                            .child("Close")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recovery_picker_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.available_models.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("recovery-connect-account")
                                .debug_selector(|| "recovery-connect-account".into())
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|el| el.bg(Theme::global().ACCENT_DIM))
                                .child("Connect account")
                                .on_click(cx.listener(|this, _, _, cx| this.open_login_picker(cx))),
                        )
                        .child(
                            div()
                                .id("recovery-refresh-models")
                                .debug_selector(|| "recovery-refresh-models".into())
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|el| el.bg(Theme::global().ACCENT_DIM))
                                .child("Refresh choices")
                                .on_click(cx.listener(|this, _, _, _| {
                                    this.bridge.send(Command::RefreshRuntime {
                                        session_id: this.session_id.clone(),
                                    });
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Command;

    fn identity(session_id: &str, model: &str) -> ApiEvent {
        ApiEvent::ModelInfo {
            session_id: session_id.into(),
            provider: Some("anthropic".into()),
            model: Some(model.into()),
            reasoning_effort: None,
            auth_method: None,
            account_label: None,
        }
    }

    fn switch(state: ModelSwitchState, model: Option<&str>) -> ApiEvent {
        ApiEvent::ModelSwitch {
            session_id: "s".into(),
            state,
            model: model.map(str::to_owned),
            error: None,
        }
    }

    fn assistant_lines(panel: &Panel) -> Vec<String> {
        panel
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Assistant(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn a_queued_switch_leaves_the_running_model_alone_until_it_lands(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, _commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new("s".into(), None, None, bridge, cx));
        panel.update(cx, |panel, cx| {
            panel.apply(&identity("s", "claude-opus-5-5"), cx);
            panel.apply(&switch(ModelSwitchState::Queued, Some("gpt-6.2-sol")), cx);
            assert_eq!(panel.model.as_deref(), Some("claude-opus-5-5"));
            assert_eq!(panel.pending_model.as_deref(), Some("gpt-6.2-sol"));
            assert!(
                panel.render_pending_model_chip(cx).is_some(),
                "the target is shown beside the chip"
            );
            assert!(assistant_lines(panel).last().is_some_and(|line| line
                .contains("queued")
                && line.contains("gpt-6.2-sol")
                && line.contains("claude-opus-5-5")));

            // The daemon reports the new identity, then that the switch landed.
            panel.apply(&identity("s", "gpt-6.2-sol"), cx);
            panel.apply(&switch(ModelSwitchState::Applied, Some("gpt-6.2-sol")), cx);
            assert_eq!(panel.model.as_deref(), Some("gpt-6.2-sol"));
            assert_eq!(panel.pending_model, None);
            assert!(panel.render_pending_model_chip(cx).is_none());
            assert_eq!(
                assistant_lines(panel).last().map(String::as_str),
                Some("Switched to `gpt-6.2-sol`.")
            );
        });
    }

    #[gpui::test]
    fn a_queued_switch_can_be_cancelled(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new("s".into(), None, None, bridge, cx));
        panel.update(cx, |panel, cx| {
            panel.apply(&identity("s", "claude-opus-5-5"), cx);
            panel.cancel_pending_model(cx);
            assert!(commands.try_recv().is_err(), "nothing queued, nothing sent");

            panel.apply(&switch(ModelSwitchState::Queued, Some("gpt-6.2-sol")), cx);
            panel.cancel_pending_model(cx);
            assert!(matches!(
                commands.try_recv(),
                Ok(Command::CancelPendingModel { session_id }) if session_id == panel.session_id
            ));

            panel.apply(&switch(ModelSwitchState::Cancelled, Some("gpt-6.2-sol")), cx);
            assert_eq!(panel.pending_model, None);
            assert_eq!(panel.model.as_deref(), Some("claude-opus-5-5"));
        });
    }

    #[gpui::test]
    fn the_cache_card_names_the_model_the_session_runs_now(cx: &mut gpui::TestAppContext) {
        let (bridge, _commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new("s".into(), None, None, bridge, cx));
        panel.update(cx, |panel, cx| {
            panel.apply(&identity("s", "claude-opus-5-5"), cx);
            panel.apply(
                &ApiEvent::RequestUsage {
                    session_id: "s".into(),
                    request_id: "r1".into(),
                    provider: "anthropic".into(),
                    model: "claude-opus-5-5".into(),
                    input: Some(100),
                    output: Some(10),
                    cache_read_input: Some(900),
                    cache_creation_input: Some(0),
                },
                cx,
            );
            assert_eq!(panel.cache_route_left_behind(), None);

            panel.provider = Some("openai".into());
            panel.apply(&identity("s", "gpt-6.2-sol"), cx);
            assert_eq!(
                panel.cache_route_left_behind().as_deref(),
                Some("anthropic · claude-opus-5-5"),
                "the figures are from the model the session left"
            );
        });
    }
}
