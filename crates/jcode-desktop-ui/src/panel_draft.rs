//! A draft panel: its model choice, config commands and the deferred request
//! that turns it into a session on the first prompt.
use super::*;

impl Panel {
    /// Hold the runtime session back until the user commits to this draft.
    /// A draft that was already requested keeps its in-flight request.
    pub(crate) fn defer_session(&mut self, cx: &mut Context<Self>) {
        if self.is_pending_session() && self.draft_session == DraftSession::Unrequested {
            self.draft_session = DraftSession::Deferred;
            self.load_draft_model_defaults(cx);
            if self.has_queued_prompts() {
                // Sent before the runtime was reachable: the commitment is
                // already made.
                self.request_deferred_session(cx);
                return;
            }
            self.status = DEFERRED_SESSION_STATUS.into();
            self.seed_draft_models(cx);
            self.load_draft_skills(cx);
            cx.notify();
        }
    }

    pub(super) fn load_draft_model_defaults(&mut self, cx: &mut Context<Self>) {
        if self.draft_config.model.is_some() || self.draft_config.effort.is_some() {
            return;
        }
        let defaults = cx
            .try_global::<RememberedDraftModel>()
            .map(|defaults| defaults.0.clone())
            .unwrap_or_else(|| {
                #[cfg(not(test))]
                {
                    crate::config::get().models.draft_defaults.clone()
                }
                #[cfg(test)]
                crate::config::DraftModelDefaults::default()
            });
        if let Some(model) = defaults.model {
            self.pick_draft_model(&model, cx);
        }
        self.draft_config.effort = defaults.effort.clone();
        self.reasoning_effort = defaults.effort;
        self.sync_effort_menu(cx);
    }

    pub(super) fn remember_draft_model(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        let defaults = crate::config::DraftModelDefaults {
            model: self
                .draft_config
                .model
                .clone()
                .or_else(|| self.model.clone()),
            effort: self.draft_config.effort.clone(),
        };
        if let Err(error) = crate::config::persist_draft_model_defaults(&defaults) {
            self.effort_notice(format!("Could not save new session defaults: {error}"), cx);
            return;
        }
        cx.set_global(RememberedDraftModel(defaults));
    }

    /// The workspace sent, or is about to send, this draft's create request.
    pub(crate) fn mark_session_requested(&mut self) {
        self.draft_session = DraftSession::Requested;
    }

    /// True while nothing has asked the runtime to create this draft's session.
    pub(crate) fn session_deferred(&self) -> bool {
        self.is_pending_session() && self.draft_session == DraftSession::Deferred
    }

    /// Prefer the daemon's unbound catalog to any previously observed session.
    pub(crate) fn seed_draft_models(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        let catalog = cx.try_global::<LocalDraftModels>().cloned().or_else(|| {
            cx.try_global::<KnownModelRoutes>()
                .map(|known| LocalDraftModels {
                    current: None,
                    routes: known.0.clone(),
                })
        });
        let Some(LocalDraftModels { current, routes }) = catalog.filter(|c| !c.routes.is_empty())
        else {
            return;
        };
        if self.draft_config.model.is_none() && current.is_some() {
            self.model = current;
        }
        if cx.try_global::<LocalDraftModels>().is_some() {
            self.sync_draft_speed(&routes);
            self.sync_open_speed_picker(cx);
        }
        let models = available_model_names(&routes);
        self.available_models = models.clone();
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = panel.update(cx, |panel, cx| {
                if !panel.session_deferred() {
                    return;
                }
                let current = panel.draft_config.model.clone().or_else(|| panel.model.clone());
                panel.input.update(cx, |input, cx| {
                    input.set_model_routes(models, &routes, current, cx);
                });
                if panel.recovery_picker_open {
                    panel.open_model_picker(cx);
                }
                panel.sync_effort_menu(cx);
                cx.notify();
            });
        });
    }

    pub(crate) fn publish_draft_models(
        current: Option<String>,
        routes: Vec<jcode_sdk::ModelRouteInfo>,
        cx: &mut App,
    ) {
        if !routes.is_empty() {
            cx.set_global(LocalDraftModels { current, routes });
        }
    }

    /// Record a model picked on a draft. Nothing is sent until the session
    /// exists.
    pub(crate) fn pick_draft_model(&mut self, model: &str, cx: &mut Context<Self>) {
        if self.draft_config.model.as_deref().or(self.model.as_deref()) != Some(model) {
            self.draft_config.speed = None;
            self.speed.clear_options();
        }
        self.draft_config.model = Some(model.to_string());
        if let Some(local) = cx.try_global::<LocalDraftModels>() {
            self.sync_draft_speed(&local.routes);
        }
        if self
            .draft_config
            .effort
            .as_deref()
            .is_some_and(|level| !crate::effort::ladder(None, Some(model)).contains(&level))
        {
            self.draft_config.effort = None;
            self.reasoning_effort = None;
        }
        // The runtime reports the bare id once it serves the model. Show the
        // same name now, without the picker's route prefix.
        let shown = match model.split_once(':') {
            Some((route, bare)) if !route.contains('/') && !bare.is_empty() => bare,
            _ => model,
        };
        self.model = Some(shown.to_string());
        // Deferred: `/model` runs inside the composer's own submit callback.
        let input = self.input.clone();
        let current = Some(model.to_owned());
        cx.defer(move |cx| input.update(cx, |input, cx| input.set_current_model(current, cx)));
        self.sync_effort_menu(cx);
        self.sync_open_speed_picker(cx);
        cx.notify();
    }

    /// Send the draft's picked model and effort to its new session, ahead of
    /// the prompts queued behind them.
    pub(super) fn apply_draft_config(&mut self, cx: &mut Context<Self>) {
        let ladder = self.effort_ladder();
        let DraftConfig {
            model,
            effort,
            speed,
            modes,
        } = std::mem::take(&mut self.draft_config);
        let (model, effort, speed) = match (model, effort, speed) {
            (Some(model), effort, Some(speed)) => {
                let safe = self.safe_draft_speed(speed);
                self.send_model_speed(model, effort, safe, cx);
                self.report_draft_speed_fallback(speed, safe, cx);
                (None, None, None)
            }
            config => config,
        };
        if let Some(model) = model {
            self.bridge.send(Command::SetModel {
                session_id: self.session_id.clone(),
                model,
            });
        }
        if let Some(effort) = effort {
            self.reasoning_effort = None;
            self.request_effort_on(&ladder, &effort, cx);
        }
        if let Some(speed) = speed {
            self.apply_draft_speed(speed, cx);
        }
        for mode in modes {
            self.bridge.send(Command::RuntimeCommand {
                session_id: self.session_id.clone(),
                command: jcode_sdk::api::RuntimeCommand::Mode,
                args: vec!["toggle".into(), mode],
            });
        }
    }

    /// Create the session a deferred draft stands for. Called when the user
    /// sends a prompt or reaches for something only a runtime session has.
    pub(crate) fn request_deferred_session(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        self.draft_session = DraftSession::Requested;
        self.status = "Starting session · you can type now".into();
        self.bridge.send(Command::CreateSession {
            working_dir: self.working_dir.clone(),
            request_id: Some(self.session_id.clone()),
        });
        cx.notify();
    }

    /// A draft restored after a restart resumes where it stood: prompts that
    /// were queued need their session now, an untouched editor does not.
    pub(crate) fn resume_draft_session(&mut self, needs_session: bool, cx: &mut Context<Self>) {
        self.draft_session = DraftSession::Deferred;
        if needs_session || self.has_queued_prompts() {
            self.request_deferred_session(cx);
        } else {
            self.status = DEFERRED_SESSION_STATUS.into();
            self.seed_draft_models(cx);
            self.load_draft_skills(cx);
            cx.notify();
        }
    }

    /// Promote in place so focus, selection, undo history, and pasted images
    /// all survive the asynchronous runtime connection.
    pub fn attach_startup_session(
        &mut self,
        session: jcode_sdk::SessionInfo,
        cx: &mut Context<Self>,
    ) {
        self.context_usage = context_usage::State::default();
        self.session_id = session.session_id;
        self.pending_await_refresh = None;
        self.pending_awaits.clear();
        self.refresh_local_pending_awaits(cx);
        self.title = session
            .title
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| short_id(&self.session_id))
            .into();
        // Older bridges report no directory for a session that has not
        // persisted yet. Keep the draft's requested directory in that case.
        if let Some(dir) = session.working_dir.filter(|dir| !dir.trim().is_empty()) {
            self.working_dir = Some(dir);
        }
        self.status = "idle".into();
        self.input.update(cx, |input, cx| {
            input.set_submission_enabled(true, cx);
            input.set_pending_session(!self.history_loaded, cx);
        });
        self.apply_draft_config(cx);
        self.send_queued_prompts(cx);
        cx.notify();
    }
}
