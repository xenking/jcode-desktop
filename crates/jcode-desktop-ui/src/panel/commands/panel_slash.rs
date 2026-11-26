//! Slash commands and skill invocations typed in the composer.
use super::*;

impl Panel {
    pub(crate) fn dispatch_composer_command(
        panel: &Entity<Self>,
        command: String,
        window: &mut Window,
        app: &mut App,
    ) {
        use crate::{commands, ui_actions};

        if Self::dispatch_workspace_command(panel, &command, window, app) {
            return;
        }
        if commands::is_resume_command(&command) {
            window.dispatch_action(Box::new(ui_actions::OpenResume), app);
            return;
        }
        if commands::is_accounts_command(&command) || command == "/subscribe" {
            window.dispatch_action(
                Box::new(ui_actions::OpenAccounts {
                    source: panel.entity_id(),
                    login_command: None,
                }),
                app,
            );
            return;
        }
        if let Some(command) = commands::reset_command(&command) {
            let provider = match command {
                commands::ResetCommand::OpenAi => crate::accounts::ResetProvider::OpenAi,
                commands::ResetCommand::Claude => crate::accounts::ResetProvider::Claude,
                commands::ResetCommand::Current => panel.read(app).reset_provider(),
            };
            window.dispatch_action(Box::new(ui_actions::OpenUsageReset { provider }), app);
            return;
        }
        match command.as_str() {
            "/applets" => window.dispatch_action(Box::new(ui_actions::OpenAppletShowcase), app),
            "/onboarding-sim" | "/onboarding-preview" => {
                window.dispatch_action(Box::new(ui_actions::ToggleOnboardingSimulator), app);
            }
            "/changelog" => window.dispatch_action(Box::new(ui_actions::OpenChangelog), app),
            "/publish" if panel.read(app).publish_checkout().is_some() => {
                window.dispatch_action(
                    Box::new(ui_actions::PublishDesktop {
                        source: panel.entity_id(),
                    }),
                    app,
                );
            }
            _ if command.split_whitespace().next() == Some("/login") => {
                window.dispatch_action(
                    Box::new(ui_actions::OpenAccounts {
                        source: panel.entity_id(),
                        login_command: Some(command),
                    }),
                    app,
                );
            }
            _ => panel.update(app, |this, cx| {
                this.handle_slash_command(&command, cx);
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn handle_slash_command_for_test(
        &mut self,
        content: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.handle_slash_command(content, cx)
    }

    pub(crate) fn handle_slash_command(&mut self, content: &str, cx: &mut Context<Self>) -> bool {
        if self.is_side_document() {
            return true;
        }
        self.transcript_measurements.invalidate();
        if self.preview_state.is_some() {
            return self.handle_preview_command(content, cx);
        }
        let trimmed = content.trim();
        if let Some(mode) = crate::speed_icon::command_tier(trimmed) {
            self.choose_speed(mode, cx);
            return true;
        }
        if self.session_deferred() && trimmed.split_whitespace().next() == Some("/mode") {
            self.select_draft_mode(trimmed["/mode".len()..].trim(), cx);
            return true;
        }
        if self.handle_skill_invocation(trimmed, cx) {
            return true;
        }
        if trimmed == "/rewind" {
            self.start_rewind(cx);
            return true;
        }
        if self.handle_clean_chat_command(trimmed, cx) {
            return true;
        }
        if self.handle_special_command(trimmed, cx)
            || self.handle_recovery_command(trimmed, cx)
            || self.handle_runtime_command(trimmed, cx)
            || self.handle_desktop_command(trimmed, cx)
        {
            self.stick_to_bottom = true;
            self.transcript_list.scroll_to_end();
            return true;
        }
        if matches!(trimmed, "/improve stop" | "/refactor stop") {
            self.pause_queue_for_stop();
            self.request_user_stop(cx);
            self.items.push(Item::Assistant("Stopped the active workflow. Queued prompts are preserved and paused, and no continuation was submitted.".into()));
            cx.notify();
            return true;
        }
        if matches!(trimmed, "/improve status" | "/refactor status") {
            let payload = self.latest_todo_payload().unwrap_or_default();
            self.items.push(Item::Todos(payload));
            self.items.push(Item::Assistant(format!(
                "Session workflow status: {}. Use `{}` to continue existing work.",
                self.status,
                trimmed.replace("status", "resume")
            )));
            cx.notify();
            return true;
        }
        if let Some(prompt) = jcode_desktop_model::command_workflows::workflow_prompt(trimmed) {
            self.submit_or_queue(prompt, Vec::new(), true, cx);
            return true;
        }
        if self.handle_acct_command(trimmed, cx) {
            return true;
        }
        if self.login_command(trimmed, cx) {
            return true;
        }
        if let Some(command) = queue::PokeCommand::parse(trimmed) {
            match command {
                Ok(command) => self.handle_poke_command(command, cx),
                Err(usage) => self.items.push(Item::Error(usage)),
            }
            cx.notify();
            return true;
        }
        if let Some((model, effort)) = trimmed
            .strip_prefix("/model ")
            .map(crate::input::parse_model_pick)
            .filter(|(model, _)| !model.is_empty())
        {
            self.close_model_picker(cx);
            if let Some(effort) = effort {
                self.switch_model_with_effort(model, &effort.to_ascii_lowercase(), cx);
            } else if self.is_pending_session() {
                self.pick_draft_model(model, cx);
                self.remember_draft_model(cx);
            } else {
                self.bridge.send(Command::SetModel {
                    session_id: self.session_id.clone(),
                    model: model.to_string(),
                });
                self.items
                    .push(Item::Assistant(format!("Switching model to `{model}`…")));
            }
        } else if let Some(effort) = trimmed.strip_prefix("/effort ").map(str::trim) {
            let effort = effort.to_ascii_lowercase();
            if self.request_effort(&effort, cx) {
                if self
                    .picker_search
                    .as_ref()
                    .is_some_and(|(kind, _)| *kind == PickerKind::Effort)
                {
                    self.picker_search = None;
                    self.focus_pending = true;
                } else {
                    let input = self.input.clone();
                    cx.defer(move |cx| input.update(cx, |input, cx| input.close_effort_menu(cx)));
                }
                if !self.is_pending_session() {
                    self.items.push(Item::Assistant(format!(
                        "Reasoning effort set to `{effort}`."
                    )));
                }
            }
        } else if let Some(title) = trimmed.strip_prefix("/rename ").map(str::trim) {
            if title.is_empty() {
                self.items.push(Item::Error(
                    "Usage: `/rename <session name>` or `/rename --clear`.".into(),
                ));
            } else {
                let title = (title != "--clear").then(|| title.to_string());
                self.run_session_operation(SessionOperation::Rename(title), "Session renamed.");
            }
        } else if trimmed == "/save" || trimmed.starts_with("/save ") {
            let label = trimmed["/save".len()..].trim();
            let label = (!label.is_empty()).then(|| label.to_string());
            self.request_session_saved(true, label);
        } else if trimmed == "/unsave" {
            self.request_session_saved(false, None);
        } else if let Some(target) = trimmed.strip_prefix("/rewind ").map(str::trim) {
            if target == "undo" {
                self.run_session_operation(SessionOperation::RewindUndo, "Rewind undone.");
            } else if let Ok(index) = target.parse::<usize>() {
                if index == 0 {
                    self.items
                        .push(Item::Error("Rewind numbering starts at 1.".into()));
                } else {
                    self.run_session_operation(
                        SessionOperation::Rewind(index),
                        format!("Rewound to message {index}."),
                    );
                }
            } else {
                self.items.push(Item::Error(
                    "Usage: `/rewind <message number|undo>`.".into(),
                ));
            }
        } else {
            match trimmed {
                "/cancel" => {
                    self.request_user_stop(cx);
                }
                "/cls" | "/clear-view" => {
                    self.startup_layout = None;
                    self.image_pane_selected = None;
                    self.items.clear();
                    self.user_times.clear();
                    self.response_stats = response_stats::Tracker::default();
                    self.streaming_text.clear();
                    self.live_assistant_output = None;
                    self.streaming_reasoning.clear();
                }
                "/clear" => self.run_session_operation(
                    SessionOperation::Clear,
                    "Conversation history cleared.",
                ),
                "/compact" => self.run_session_operation(
                    SessionOperation::Compact,
                    "Context compacted.",
                ),
                "/help" | "/commands" | "/?" => {
                    self.items.push(Item::Assistant(help_markdown()))
                }
                "/todos" | "/todo" => {
                    if let Some(payload) = self.latest_todo_payload() {
                        self.items.push(Item::Todos(payload));
                    } else {
                        self.items.push(Item::Todos(TodoCardPayload::default()));
                    }
                }
                "/model" | "/models" => self.open_model_picker_deferred(cx),
                "/update" if crate::updates::paired_configured() => {
                    // The workspace owns the update flow, so the panel asks it.
                    cx.dispatch_action(&crate::ui_actions::CheckForUpdates);
                    self.items.push(Item::Assistant("Checking for a paired CLI and Desktop update…".into()));
                }
                "/update" => {
                    let request = crate::updates::request_now();
                    let message = match request {
                        crate::updates::UpdateRequest::Checking =>
                            "Checking for Jcode Desktop updates…",
                        crate::updates::UpdateRequest::AlreadyChecking =>
                            "Jcode Desktop is already checking for updates.",
                        crate::updates::UpdateRequest::Downloading =>
                            "A Jcode Desktop update is downloading in the background.",
                        crate::updates::UpdateRequest::Restarting =>
                            "Installing the update and restarting Jcode Desktop…",
                        crate::updates::UpdateRequest::Unavailable =>
                            "Automatic updates are unavailable in this build of Jcode Desktop.",
                    };
                    self.items.push(Item::Assistant(message.into()));
                    #[cfg(not(target_os = "macos"))]
                    if request == crate::updates::UpdateRequest::Checking {
                        cx.spawn(async move |this, cx| {
                            loop {
                                // The worker owns IO, never the UI thread. Surface
                                // its terminal result even if the user is idle.
                                let result = match crate::updates::current() {
                                    crate::updates::UpdateState::Finished { message } => Some(Ok(message)),
                                    crate::updates::UpdateState::Failed { message } => Some(Err(message)),
                                    crate::updates::UpdateState::ReadyToRestart { version } => Some(Ok(format!(
                                        "Jcode Desktop {version} is installed. Run `/update` again or click the update chip to restart into it now, or it starts the next time you open Jcode Desktop."
                                    ))),
                                    crate::updates::UpdateState::Idle => break,
                                    _ => None,
                                };
                                if let Some(result) = result {
                                    let _ = this.update(cx, |panel, cx| {
                                        panel.items.push(match result {
                                            Ok(message) => Item::Assistant(message),
                                            Err(message) => Item::Error(message),
                                        });
                                        cx.notify();
                                    });
                                    break;
                                }
                                if this.upgrade().is_none() { break; }
                                cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                            }
                        }).detach();
                    }
                }
                "/effort" => {
                    let ladder = self.effort_ladder();
                    let current = self.reasoning_effort.as_deref().unwrap_or("default");
                    let keys = crate::effort::bound_keys_label(cx)
                        .map(|keys| format!(" or {keys}"))
                        .unwrap_or_default();
                    self.items.push(Item::Assistant(if ladder.is_empty() {
                        "Reasoning effort is not available for this model.".into()
                    } else {
                        format!(
                            "Effort: `{}`. Use `/effort <{}>`{keys} to change it.",
                            crate::effort::spoken(current),
                            ladder.join("|")
                        )
                    }));
                }
                "/rename" => self.items.push(Item::Error(
                    "Usage: `/rename <session name>` or `/rename --clear`.".into(),
                )),
                "/commit" => self.submit_command_prompt(
                    "Make interactive, logical commits for the current uncommitted work. Inspect git state first, group related changes into coherent commits, preserve unrelated work, validate appropriately, and report the commits created plus remaining changes.",
                    cx,
                ),
                "/publish" => {
                    let item = match self.publish_checkout() {
                        Some(_) => Item::Assistant(
                            "Opened the publish tracker. Commit, push, release, and publish progress appears in its panel.".into(),
                        ),
                        None => Item::Error(
                            "`/publish` is only available in Jcode Desktop self-development sessions.".into(),
                        ),
                    };
                    self.items.push(item);
                }
                "/restart-all" | "/restart-all desktop" => {
                    let restart_server = trimmed == "/restart-all";
                    self.items.push(match crate::restart_spawn::spawn_worker(restart_server) {
                        Ok(()) => Item::Assistant(if restart_server {
                            "Restarting the Jcode server and every Desktop window. Windows, sessions, drafts, and layout come back in a few seconds.".into()
                        } else {
                            "Restarting every Desktop window. Windows, sessions, drafts, and layout come back in a few seconds.".into()
                        }),
                        Err(error) => Item::Error(format!("Could not start the restart: {error}")),
                    });
                }
                "/commit-push" | "/commit-and-push" => self.submit_command_prompt(
                    "Make logical commits for the current uncommitted work, preserving unrelated work and validating appropriately. Then push to the tracking branch without force-pushing, and report the commits and push result.",
                    cx,
                ),
                _ if trimmed.split_whitespace().next().and_then(registered_command).is_some()
                    || jcode_base::skill::SkillRegistry::parse_invocation(trimmed).is_some() => self.items.push(Item::Error(command_unavailable_message(trimmed))),
                _ => return false,
            }
        }
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        cx.notify();
        true
    }

    pub(crate) fn handle_skill_invocation(&mut self, content: &str, cx: &mut Context<Self>) -> bool {
        self.handle_skill_invocation_with_images(content, Vec::new(), false, cx)
    }

    pub(crate) fn handle_skill_invocation_with_images(
        &mut self,
        content: &str,
        images: Vec<(String, String)>,
        queued: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = content.strip_prefix('/') else {
            return false;
        };
        let Some((name, description)) = self
            .skills
            .iter()
            .filter(|(name, _)| {
                rest.strip_prefix(name)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
                    && (name == "selfdev"
                        || crate::commands::registered_command(&format!("/{name}")).is_none())
            })
            .max_by_key(|(name, _)| name.len())
            .cloned()
        else {
            return false;
        };
        let prompt = rest[name.len()..].trim();
        let prompt = match prompt.as_bytes() {
            [b'"', .., b'"'] | [b'\'', .., b'\''] if prompt.len() >= 2 => {
                &prompt[1..prompt.len() - 1]
            }
            _ => prompt,
        }
        .to_string();
        self.active_skill = Some(name.clone());
        self.sync_mode_menu(cx);
        self.items.push(Item::Assistant(format!(
            "Activated skill: {name} - {description}"
        )));
        if !prompt.is_empty() || !images.is_empty() {
            self.submit_or_queue(prompt, images, queued, cx);
        }
        cx.notify();
        true
    }

    /// Compose cards a later Gmail call replaced, recomputed only when the
    /// transcript changes.
    pub(super) fn superseded_drafts(&self) -> std::rc::Rc<HashMap<String, gmail_draft_card::Superseded>> {
        let revision = self.items.revision();
        let mut cache = self.superseded_drafts.borrow_mut();
        if cache.0 != revision {
            let tools = self.items.iter().filter_map(|item| match item {
                Item::Tool {
                    call_id,
                    name,
                    input,
                    output,
                    done,
                    error,
                    ..
                } => Some((
                    call_id.as_str(),
                    name.as_str(),
                    input.as_str(),
                    output.as_str(),
                    *done,
                    error.as_deref(),
                )),
                _ => None,
            });
            *cache = (
                revision,
                std::rc::Rc::new(gmail_draft_card::superseded_cards(tools)),
            );
        }
        cache.1.clone()
    }

    pub(super) fn saved_settled(
        &mut self,
        saved: bool,
        label: Option<&str>,
        error: Option<&str>,
    ) -> bool {
        if self.pending_save_requests == 0 {
            return false;
        }
        self.pending_save_requests -= 1;
        self.items.push(match (error, saved, label) {
            (Some(error), true, _) => Item::Error(format!("Failed to save session: {error}")),
            (Some(error), false, _) => Item::Error(format!("Failed to unsave session: {error}")),
            (None, true, Some(label)) => Item::Assistant(format!(
                "📌 Session saved as \"{label}\". It will appear at the top of /resume."
            )),
            (None, true, None) => {
                Item::Assistant("📌 Session saved. It will appear at the top of /resume.".into())
            }
            (None, false, _) => Item::Assistant("Session removed from saved.".into()),
        });
        true
    }
}
