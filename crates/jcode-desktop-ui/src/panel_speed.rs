use jcode_provider_core::{SpeedMode, SpeedOption};

use super::*;
use crate::speed_icon::{self, SpeedPicker};

#[derive(Default)]
pub(super) struct SessionSpeed {
    options: Vec<SpeedOption>,
    intended: SpeedMode,
    pending: Option<(u64, SpeedMode)>,
    serial: u64,
    rejection: Option<(SpeedMode, String)>,
    last_response: Option<String>,
}

impl Panel {
    pub(super) fn apply_speed_picker_fixture(&mut self, cx: &mut Context<Self>) {
        if !crate::harness::screenshot_mode() {
            return;
        }
        let kind = match std::env::var("JCODE_DESKTOP_SCREENSHOT_COMPOSER_PICKER").as_deref() {
            Ok("model") => PickerKind::Model,
            Ok("effort") => PickerKind::Effort,
            Ok("speed") => PickerKind::Speed,
            _ => return,
        };
        let routes = ["gpt-6.2-astra", "gpt-6.2-sol"].map(|model| jcode_sdk::ModelRouteInfo {
            model: model.into(),
            provider: "openai".into(),
            api_method: "openai-oauth".into(),
            available: true,
            detail: String::new(),
            usage: None,
            speed_options: Vec::new(),
        });
        self.provider = Some("openai".into());
        self.model = Some(routes[0].model.clone());
        let ladder = self.effort_ladder();
        self.input.update(cx, |input, cx| {
            input.set_model_routes(Vec::new(), &routes, self.model.clone(), cx);
            input.set_effort_state(ladder, self.reasoning_effort.clone(), cx);
        });
        self.update_speed(
            SpeedMode::Fast,
            &[SpeedMode::Fast, SpeedMode::Ultrafast].map(|mode| SpeedOption {
                mode,
                availability: jcode_provider_core::SpeedAvailability::Available,
                reason: None,
            }),
            cx,
        );
        self.open_picker_search(kind, cx);
    }

    fn intended_speed(&self) -> SpeedMode {
        self.draft_config
            .speed
            .or(self.speed.pending.map(|(_, mode)| mode))
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
            cx.notify();
        } else {
            self.send_speed(mode, cx);
        }
    }

    pub(super) fn send_speed(&mut self, mode: SpeedMode, cx: &mut Context<Self>) {
        self.speed.serial += 1;
        let serial = self.speed.serial;
        self.speed.pending = Some((serial, mode));
        self.speed.last_response = None;
        self.speed.rejection = None;
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation: SessionOperation::SetSpeed { serial, mode },
        });
        cx.notify();
    }

    pub(super) fn apply_draft_speed(&mut self, requested: SpeedMode, cx: &mut Context<Self>) {
        let safe = speed_icon::visible_mode(&self.speed.options, requested);
        self.send_speed(safe, cx);
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
        let Some((pending, mode)) = self.speed.pending else {
            return;
        };
        if serial != pending {
            return;
        }
        self.speed.pending = None;
        match result {
            Ok(applied) => self.speed.intended = applied,
            Err(error) => {
                self.effort_notice(format!("{} was not applied.", speed_icon::label(mode)), cx);
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
        self.speed.options.clear();
        self.speed.last_response = None;
        self.sync_open_speed_picker(cx);
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
        if let Some((PickerKind::Speed, input)) = &self.picker_search {
            let picker = self.speed_picker();
            input.update(cx, |input, cx| input.set_speed_picker(picker, cx));
        }
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
