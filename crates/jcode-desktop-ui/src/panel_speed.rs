use jcode_provider_core::{SpeedMode, SpeedOption};

use super::*;
use crate::speed_icon::{self, SpeedPicker};

#[derive(Default)]
pub(super) struct SessionSpeed {
    options: Vec<SpeedOption>,
    intended: SpeedMode,
    pending: Option<PendingSpeed>,
    serial: u64,
    rejection: Option<(SpeedMode, String)>,
    last_response: Option<String>,
}

#[derive(Clone, Copy)]
struct PendingSpeed {
    serial: u64,
    mode: SpeedMode,
    switching_model: bool,
}

impl SessionSpeed {
    pub(super) fn clear_options(&mut self) {
        self.options.clear();
        self.last_response = None;
    }
}

impl Panel {
    pub(super) fn apply_speed_picker_fixture(&mut self, cx: &mut Context<Self>) {
        if !crate::harness::screenshot_mode() {
            return;
        }
        let kind = match std::env::var("JCODE_DESKTOP_SCREENSHOT_COMPOSER_PICKER").as_deref() {
            Ok("model") => PickerKind::Model,
            Ok("effort") => PickerKind::Effort,
            _ => return,
        };
        let mut event = crate::harness::composer_picker_fixture_runtime(&self.session_id, None);
        if let ApiEvent::RuntimeInfo {
            reasoning_effort, ..
        } = &mut event
        {
            *reasoning_effort = self.reasoning_effort.clone();
        }
        self.apply(&event, cx);
        let ladder = self.effort_ladder();
        self.input.update(cx, |input, cx| {
            input.set_effort_state(ladder, self.reasoning_effort.clone(), cx)
        });
        self.open_picker_search(kind, cx);
        if let Some((_, input)) = &self.picker_search {
            input.update(cx, |input, cx| input.focus_next_picker_row_for_screenshot(cx));
        }
    }

    fn intended_speed(&self) -> SpeedMode {
        self.draft_config
            .speed
            .or(self
                .speed
                .pending
                .filter(|pending| !pending.switching_model)
                .map(|pending| pending.mode))
            .unwrap_or(self.speed.intended)
    }

    pub(super) fn visible_speed(&self) -> SpeedMode {
        speed_icon::visible_mode(&self.speed.options, self.intended_speed())
    }

    pub(super) fn speed_picker(&self) -> SpeedPicker {
        SpeedPicker::new(
            &self.speed.options,
            self.intended_speed(),
            self.speed.pending.is_some(),
            self.speed.last_response.as_deref(),
            self.speed
                .rejection
                .as_ref()
                .map(|(mode, reason)| (*mode, reason.as_str())),
        )
    }

    pub(super) fn sync_draft_speed(&mut self, routes: &[jcode_sdk::ModelRouteInfo]) {
        let model = self.draft_config.model.as_deref().or(self.model.as_deref());
        self.speed.options = model
            .and_then(|model| draft_route(routes, model))
            .map(|route| route.speed_options.clone())
            .unwrap_or_default();
    }

    pub(super) fn choose_speed(&mut self, mode: SpeedMode, cx: &mut Context<Self>) {
        if self.is_side_document() || self.preview_state.is_some() {
            return;
        }
        if mode != SpeedMode::Standard
            && self
                .speed
                .pending
                .is_some_and(|pending| pending.switching_model)
        {
            self.effort_notice(
                "Wait for the model switch before changing speed.".into(),
                cx,
            );
            return;
        }
        if !speed_icon::offered(&self.speed.options, mode) {
            self.effort_notice(
                format!(
                    "{} is no longer offered on this route.",
                    speed_icon::label(mode)
                ),
                cx,
            );
            return;
        }
        if mode == self.intended_speed()
            && self.speed.rejection.is_none()
            && (!self.is_pending_session() || self.draft_config.speed.is_some())
        {
            return;
        }
        if self.is_pending_session() {
            self.draft_config.speed = Some(mode);
            self.sync_open_speed_picker(cx);
            cx.notify();
        } else {
            self.send_speed(mode, cx);
        }
    }

    pub(super) fn choose_model_speed(
        &mut self,
        choice: crate::input::ModelSpeedChoice<'_>,
        cx: &mut Context<Self>,
    ) {
        if self.is_side_document() || self.preview_state.is_some() {
            return;
        }
        if choice.current {
            self.choose_speed(choice.mode, cx);
            return;
        }
        self.close_model_picker(cx);
        self.composer_popover_trigger = None;
        self.focus_pending = true;
        if self.is_pending_session() {
            if let Some(effort) = choice.effort {
                self.switch_model_with_effort(choice.spec, effort, cx);
            } else {
                self.pick_draft_model(choice.spec, cx);
                self.remember_draft_model(cx);
            }
            self.choose_speed(choice.mode, cx);
        } else {
            self.send_model_speed(
                choice.spec.into(),
                choice.effort.map(str::to_owned),
                choice.mode,
                cx,
            );
        }
    }

    pub(super) fn send_speed(&mut self, mode: SpeedMode, cx: &mut Context<Self>) {
        let serial = self.begin_speed_request(mode, false, cx);
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation: SessionOperation::SetSpeed { serial, mode },
        });
    }

    pub(super) fn send_model_speed(
        &mut self,
        model: String,
        effort: Option<String>,
        mode: SpeedMode,
        cx: &mut Context<Self>,
    ) {
        let serial = self.begin_speed_request(mode, true, cx);
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation: SessionOperation::SetModelWithSpeed {
                model,
                effort,
                serial,
                mode,
            },
        });
    }

    fn begin_speed_request(
        &mut self,
        mode: SpeedMode,
        switching_model: bool,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.speed.serial += 1;
        let serial = self.speed.serial;
        self.speed.pending = Some(PendingSpeed {
            serial,
            mode,
            switching_model,
        });
        self.speed.last_response = None;
        self.speed.rejection = None;
        self.sync_open_speed_picker(cx);
        cx.notify();
        serial
    }

    pub(super) fn apply_draft_speed(&mut self, requested: SpeedMode, cx: &mut Context<Self>) {
        let safe = self.safe_draft_speed(requested);
        self.send_speed(safe, cx);
        self.report_draft_speed_fallback(requested, safe, cx);
    }

    pub(super) fn safe_draft_speed(&self, requested: SpeedMode) -> SpeedMode {
        speed_icon::visible_mode(&self.speed.options, requested)
    }

    pub(super) fn report_draft_speed_fallback(
        &mut self,
        requested: SpeedMode,
        safe: SpeedMode,
        cx: &mut Context<Self>,
    ) {
        if safe != requested {
            self.speed.rejection = Some((
                requested,
                "The draft route does not advertise this speed.".into(),
            ));
            self.effort_notice(
                format!(
                    "{} unavailable. Using Standard.",
                    speed_icon::label(requested)
                ),
                cx,
            );
        }
    }

    pub(crate) fn speed_settled(
        &mut self,
        serial: u64,
        result: Result<SpeedMode, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.speed.pending else {
            return;
        };
        if serial != pending.serial {
            return;
        }
        self.speed.pending = None;
        let mode = pending.mode;
        match result {
            Ok(applied) => self.speed.intended = applied,
            Err(error) => {
                self.effort_notice(
                    format!("{} was not applied: {error}", speed_icon::label(mode)),
                    cx,
                );
                self.speed.rejection = Some((mode, error));
            }
        }
        self.sync_open_speed_picker(cx);
        cx.notify();
    }

    pub(super) fn update_speed(
        &mut self,
        intended: SpeedMode,
        options: &[SpeedOption],
        cx: &mut Context<Self>,
    ) {
        if self.speed.intended == intended && self.speed.options == options {
            return;
        }
        if self.speed.intended != intended {
            self.speed.last_response = None;
        }
        self.speed.intended = intended;
        self.speed.options = options.to_vec();
        self.sync_open_speed_picker(cx);
    }

    pub(super) fn invalidate_speed_route(&mut self, cx: &mut Context<Self>) {
        if self.speed.options.is_empty() {
            return;
        }
        self.speed.clear_options();
        self.sync_open_speed_picker(cx);
        self.bridge.send(Command::RefreshRuntime {
            session_id: self.session_id.clone(),
        });
    }

    pub(super) fn record_speed_report(
        &mut self,
        requested: Option<SpeedMode>,
        actual: SpeedMode,
        cx: &mut Context<Self>,
    ) {
        let response = response_label(requested, actual);
        if self.speed.last_response.as_deref() != Some(&response) {
            self.speed.last_response = Some(response);
            self.sync_open_speed_picker(cx);
        }
    }

    pub(super) fn sync_open_speed_picker(&self, cx: &mut Context<Self>) {
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let Some(panel) = panel.upgrade() else { return };
            let panel = panel.read(cx);
            let picker = panel.speed_picker();
            let input = panel.input.clone();
            let search = panel
                .picker_search
                .as_ref()
                .filter(|(kind, _)| *kind == PickerKind::Model)
                .map(|(_, search)| search.clone());
            input.update(cx, |input, cx| input.set_speed_picker(picker.clone(), cx));
            if let Some(search) = search {
                search.update(cx, |search, cx| search.set_speed_picker(picker, cx));
            }
        });
    }
}

fn response_label(requested: Option<SpeedMode>, actual: SpeedMode) -> String {
    let actual = speed_icon::label(actual);
    match requested {
        Some(requested) => format!(
            "Last response: {actual} (requested {}).",
            speed_icon::label(requested)
        ),
        None => format!("Last response: {actual} (provider report)."),
    }
}

fn draft_route<'a>(
    routes: &'a [jcode_sdk::ModelRouteInfo],
    model: &str,
) -> Option<&'a jcode_sdk::ModelRouteInfo> {
    let routes = routes.iter().filter(|route| route.available);
    if model.contains(':')
        && let Some(exact) = routes
            .clone()
            .find(|route| crate::input::model_menu::route_spec(route) == model)
    {
        return Some(exact);
    }
    let mut matches = routes.filter(|route| route.model == model);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_response_does_not_claim_requested_fast_was_served() {
        assert_eq!(
            response_label(Some(SpeedMode::Fast), SpeedMode::Standard),
            "Last response: Standard (requested Fast)."
        );
        assert_eq!(
            response_label(None, SpeedMode::Fast),
            "Last response: Fast (provider report)."
        );
    }

    #[gpui::test]
    fn restoring_a_draft_keeps_its_speed_but_does_not_seed_another_draft(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, sent) = crate::harness::spawn_recording();
        let new_panel = |cx: &mut Context<Panel>| {
            Panel::new(
                Panel::STARTUP_SESSION_ID.into(),
                None,
                None,
                bridge.clone(),
                cx,
            )
        };
        let panel = cx.new(new_panel);
        let options = [SpeedOption {
            mode: SpeedMode::Fast,
            availability: jcode_provider_core::SpeedAvailability::Available,
            reason: None,
        }];
        panel.update(cx, |panel, cx| {
            panel.update_speed(SpeedMode::Standard, &options, cx);
            panel.choose_speed(SpeedMode::Fast, cx);
            assert_eq!(panel.visible_speed(), SpeedMode::Fast);
        });
        let encoded = panel.read_with(cx, |panel, cx| {
            serde_json::to_vec(&panel.snapshot(cx)).unwrap()
        });

        let restored = cx.new(new_panel);
        let fresh = cx.new(new_panel);
        restored.update(cx, |panel, cx| {
            panel.restore_snapshot(serde_json::from_slice(&encoded).unwrap(), cx);
            assert_eq!(panel.visible_speed(), SpeedMode::Standard);
            assert!(
                panel
                    .speed_picker()
                    .tooltip
                    .contains("Requested Fast is unavailable")
            );
            panel.update_speed(SpeedMode::Standard, &options, cx);
            assert_eq!(panel.visible_speed(), SpeedMode::Fast);
        });
        fresh.update(cx, |panel, cx| {
            panel.update_speed(SpeedMode::Standard, &options, cx);
            assert_eq!(panel.visible_speed(), SpeedMode::Standard);
        });
        assert!(
            sent.try_recv().is_err(),
            "draft edit and restore must not send a command"
        );
    }
}
