//! Applying the runtime's events to a panel: the one place a streamed event
//! becomes transcript rows, footer state and follow-up work.
use super::*;

impl Panel {
    pub(crate) fn apply_event(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        if matches!(
            event,
            ApiEvent::TextDelta { .. }
                | ApiEvent::TextReplace { .. }
                | ApiEvent::ReasoningDelta { .. }
                | ApiEvent::ToolStart { .. }
                | ApiEvent::MessageAccepted { .. }
                | ApiEvent::Error { .. }
                | ApiEvent::TurnStopped { .. }
        ) {
            // New live evidence supersedes the earlier persisted candidate.
            self.reconnect_response = None;
        }
        // Streaming appends only affect the live suffix. Keep a conservative
        // full invalidation for tools, status transitions, images and errors.
        // Metadata-only events still repaint chrome without discarding heights.
        if !matches!(
            event,
            ApiEvent::TextDelta { .. }
                | ApiEvent::ReasoningDelta { .. }
                | ApiEvent::TokenUsage { .. }
                | ApiEvent::RequestUsage { .. }
                | ApiEvent::ContextOccupancy { .. }
                | ApiEvent::CacheKeepalive { .. }
                | ApiEvent::ModelInfo { .. }
                | ApiEvent::RuntimeInfo { .. }
                | ApiEvent::SessionRenamed { .. }
        ) {
            self.transcript_measurements.invalidate();
        }
        // Any event from the session proves the stream is back. A reattach
        // may resume events without a SessionConnected update, so the
        // `lost: …` banner must not outlive the outage.
        if self.status.starts_with("lost:") && !matches!(event, ApiEvent::Error { .. }) {
            self.status = "connected".into();
            self.queue_reconnected(cx);
        }
        self.response_stats.observe(event, self.provider.as_deref());
        self.observe_activity_state(event);
        self.observe_session_image_ids(event, cx);
        if matches!(event, ApiEvent::TurnDone { .. } | ApiEvent::TurnStopped { .. }) {
            self.dismiss_routing_gate(cx);
        }
        if let Some(cue) = self.sound_events.observe(event)
            && self.preview_state.is_none()
            && !self.demo
        {
            crate::sounds::play(cue, cx);
        }
        match event {
            ApiEvent::SessionRecovery { .. } => {
                self.continuing_after_reload = true;
            }
            ApiEvent::AskRequest {
                request_id,
                tool_call_id,
                questions,
                kind,
                ..
            } => {
                self.continuing_after_reload = false;
                self.show_live_ask(request_id, tool_call_id, questions, *kind, cx);
            }
            ApiEvent::RoutingDecision {
                account,
                reason,
                warnings,
                pending,
                ..
            } => self.routing_decision(account, reason, warnings, pending.as_deref(), cx),
            ApiEvent::RoutingGate {
                gate_id,
                reason,
                options,
                ..
            } => self.open_routing_gate(gate_id, reason, options, cx),
            ApiEvent::RoutingGateClosed { gate_id, .. } => self.close_routing_gate(gate_id, cx),
            ApiEvent::PermissionRequest {
                tool_name,
                description,
                ..
            } => {
                self.pending_approval = Some(clean_chat::PendingApproval {
                    tool_name: tool_name.clone(),
                    description: description.clone(),
                });
            }
            ApiEvent::RuntimeCommandResult {
                session_id,
                command,
                data,
                message,
            } => {
                if *command == jcode_sdk::api::RuntimeCommand::BackgroundOutput {
                    if let (Some(task_id), Some(output)) =
                        (data["task_id"].as_str(), data["output"].as_str())
                    {
                        for item in self.items.iter_mut() {
                            match item {
                                Item::BackgroundTask {
                                    task_id: id,
                                    detail,
                                    ..
                                } if id == task_id => {
                                    *detail = Some(format!(
                                        "**Background task**\n\n```text\n{output}\n```"
                                    ));
                                }
                                Item::Tool {
                                    name,
                                    input,
                                    output: body,
                                    ..
                                } if name == "bg"
                                    && serde_json::from_str::<serde_json::Value>(input)
                                        .ok()
                                        .and_then(|v| {
                                            v.get("task_id")
                                                .and_then(|v| v.as_str())
                                                .map(str::to_owned)
                                        })
                                        .as_deref()
                                        == Some(task_id) =>
                                {
                                    let metadata = body
                                        .split_once("\n\nOutput preview:\n")
                                        .map(|(metadata, _)| metadata)
                                        .unwrap_or(body.as_str());
                                    *body = format!(
                                        "{metadata}\n\nOutput preview:\n```text\n{output}\n```"
                                    );
                                }
                                _ => {}
                            }
                        }
                        self.transcript_measurements.invalidate();
                        cx.notify();
                    }
                    return;
                }
                if *command == jcode_sdk::api::RuntimeCommand::Mode {
                    self.mode_multiselect = data["pinned_skills"].is_array();
                    self.pinned_modes = crate::mode_catalog::pinned_names(data, "pinned_skill");
                    // A modern daemon already applied these modes to its session.
                    // Do not send its compatibility primary back as a legacy replacement.
                    self.active_skill = if self.mode_multiselect {
                        None
                    } else {
                        self.pinned_modes.first().cloned()
                    };
                    self.sync_mode_menu(cx);
                    if message.is_empty() {
                        cx.notify();
                        return;
                    }
                }
                if *command == jcode_sdk::api::RuntimeCommand::Skills {
                    let skills = data["skills"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|skill| {
                            Some((
                                skill["name"].as_str()?.to_string(),
                                skill["description"].as_str().unwrap_or("").to_string(),
                            ))
                        })
                        .collect::<Vec<_>>();
                    self.skills = skills;
                    self.mode_catalog = data["skills"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|skill| {
                            Some((
                                skill["name"].as_str()?.to_owned(),
                                crate::mode_catalog::ModeMetadata {
                                    mode: skill["mode"].as_bool(),
                                    icon: skill["icon"].as_str().map(str::to_owned),
                                },
                            ))
                        })
                        .collect();
                    let multiselect = data["pinned_skills"].is_array();
                    if multiselect
                        && !self.mode_multiselect
                        && self.active_skill == self.pinned_modes.first().cloned()
                    {
                        self.active_skill = None;
                    }
                    self.mode_multiselect = multiselect;
                    self.pinned_modes = crate::mode_catalog::pinned_names(data, "active_skill");
                    if !multiselect && self.active_skill.is_none() {
                        self.active_skill = self.pinned_modes.first().cloned();
                    }
                    self.sync_mode_menu(cx);
                    cx.notify();
                    if message.is_empty() {
                        return;
                    }
                }
                let output = jcode_sdk::api::RuntimeCommandOutput {
                    session_id: session_id.clone(),
                    command: *command,
                    data: data.clone(),
                    message: message.clone(),
                };
                self.items.push(Item::Assistant(output.message.clone()));
                self.apply_special_command_artifacts(&output, cx);
                self.stick_to_bottom = true;
            }
            ApiEvent::PeerUserMessage { content, .. } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.stamp_user(self.items.len());
                self.items.push(Item::User(content.clone()));
                self.stick_to_bottom = true;
            }
            ApiEvent::AgentMessage {
                from_session,
                from_name,
                message,
                ..
            } => {
                self.flush_reasoning();
                self.flush_streaming();
                self.items.push(Item::AgentMessage {
                    from_session: from_session.clone(),
                    from_name: from_name.clone(),
                    message: message.clone(),
                    timestamp_ms: response_stats::now_ms(),
                });
            }
            ApiEvent::SwarmWorkerNotice { message, .. } => {
                self.flush_reasoning();
                self.flush_streaming();
                self.items.push(Item::SwarmWorkerNotice(message.clone()));
            }
            ApiEvent::MessageAccepted { .. } => {
                acknowledge_next(
                    &mut self.pending_users,
                    &mut self.accepted_users,
                    Instant::now(),
                );
            }
            ApiEvent::TextDelta { text, message_id, .. } => {
                self.append_assistant_delta(text, message_id.as_deref());
            }
            ApiEvent::TextDone { message_id, .. } => {
                self.close_assistant_output(message_id.as_deref());
            }
            ApiEvent::TextReplace { text, message_id, .. } => {
                self.replace_assistant_output(message_id.as_deref(), text);
            }
            ApiEvent::ReasoningDelta { text, .. } => {
                // Thinking that resumes after answer text is a new phase. Settle
                // the text first so the thought paints below it instead of
                // merging into the earlier thinking block above the text.
                self.flush_streaming();
                self.continue_after_closed_output();
                self.streaming_reasoning.push_str(text);
            }
            ApiEvent::ReasoningDone { .. } => {
                self.flush_reasoning();
            }
            ApiEvent::ToolStart { call_id, name, .. } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.arriving_tools.insert(call_id.clone(), Instant::now());
                self.tool_started.insert(call_id.clone(), Instant::now());
                self.tool_wall_times
                    .insert(call_id.clone(), (response_stats::now_ms(), None));
                self.items.push(Item::Tool {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    input: String::new(),
                    output: String::new(),
                    done: false,
                    error: None,
                    ask: None,
                });
            }
            ApiEvent::ToolInputDelta { call_id, delta, .. } => {
                if let Some(Item::Tool { input, name, .. }) = self.find_tool(call_id) {
                    input.push_str(delta);
                    if name == "ask" {
                        self.schedule_ask_recovery(cx);
                    }
                }
            }
            ApiEvent::ToolExec { call_id, name, .. } => {
                if self
                    .pending_approval
                    .as_ref()
                    .is_some_and(|approval| approval.tool_name == *name)
                {
                    self.pending_approval = None;
                }
                if name == "ask" {
                    self.schedule_ask_recovery(cx);
                }
                self.tool_started.insert(call_id.clone(), Instant::now());
                self.tool_wall_times
                    .insert(call_id.clone(), (response_stats::now_ms(), None));
                let key = format!("clock:{call_id}");
                self.running_tool_tickers
                    .get_mut()
                    .retain(|existing| existing != key);
            }
            ApiEvent::ToolDone {
                call_id,
                name,
                output,
                error,
                codemode,
                ask: record,
                ..
            } => {
                let record = ask::resolve_record(name, record.as_deref(), output);
                if classify::is_codemode(name) && let Some(summary) = codemode {
                    self.codemode_summaries
                        .insert(call_id.clone(), summary.clone());
                }
                self.tool_wall_times
                    .entry(call_id.clone())
                    .or_insert((None, None))
                    .1 = response_stats::now_ms();
                let completed_ask = self.ask_card.as_ref().is_some_and(|card| {
                    name == "ask"
                        && (card.tool_call_id == *call_id
                            || (card.tool_call_id.is_empty()
                                && card.request_id.strip_prefix("ask-") == Some(call_id)))
                });
                let ask_input = if completed_ask {
                    self.cancel_ask_voice(cx);
                    let card = self.ask_card.take().expect("checked above");
                    self.ask_focus_return = card.focus_scope.clone();
                    let was_reload = output.contains("reload")
                        || error
                            .as_deref()
                            .is_some_and(|error| error.contains("reload"));
                    self.pending_ask_draft = was_reload.then(|| card.draft(cx));
                    let input = serde_json::json!({"questions": card.questions}).to_string();
                    Some(input)
                } else {
                    None
                };
                if let Some(Item::Tool {
                    input,
                    done,
                    error: slot,
                    output: output_slot,
                    ask: record_slot,
                    ..
                }) = self.find_tool(call_id)
                {
                    *done = true;
                    *slot = error.clone();
                    *output_slot = output.clone();
                    *record_slot = record;
                    if input.is_empty()
                        && let Some(ask_input) = &ask_input
                    {
                        *input = ask_input.clone();
                    }
                    if let Some(started) = self.tool_started.remove(call_id) {
                        self.tool_durations
                            .insert(call_id.clone(), started.elapsed());
                    }
                    let running = &self.tool_started;
                    self.running_tool_tickers.get_mut().retain(|key| {
                        key.split_once(':')
                            .is_some_and(|(_, call)| running.contains_key(call))
                    });
                } else {
                    self.arriving_tools.insert(call_id.clone(), Instant::now());
                    self.items.push(Item::Tool {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        input: ask_input.unwrap_or_default(),
                        output: output.clone(),
                        done: true,
                        error: error.clone(),
                        ask: record,
                    });
                }
                // Any finished tool retires the questions it was waiting on,
                // including computer-use consent prompts.
                self.forget_queued_ask(call_id);
                if name == "ask" {
                    self.show_next_queued_ask(cx);
                    self.schedule_ask_recovery(cx);
                }
            }
            ApiEvent::Compacted { message, .. } => {
                self.context_usage.compacting = true;
                // Older daemons include a multi-line context report in the acknowledgement.
                let label = if message.lines().count() == 1 {
                    message.as_str()
                } else {
                    "Compacting context…"
                };
                self.show_compaction_running(label);
            }
            ApiEvent::MemoryInjected { content, .. } => {
                // The daemon stores the same row it sends here, so the live
                // line and the reloaded one come from one body. Never coalesce
                // repeated injections: each delivery is part of the timeline.
                if !content.trim().is_empty() {
                    self.flush_reasoning();
                    self.flush_streaming();
                    self.items
                        .push(Item::Memory(compaction::CompactionRow::complete(
                            content.clone(),
                        )));
                }
            }
            ApiEvent::MemoryActivity { activity, .. } => {
                // Activity is chrome, not transcript: keep the latest snapshot
                // only while the pipeline is doing something.
                self.memory_activity = (!activity.is_idle()).then(|| activity.clone());
            }
            // A compaction started or finished. The divider replaces the old
            // transient "Context compaction" card: it is one rule line, it
            // carries the method/token/duration label, and the completed row is
            // also written to session history so it survives a reload.
            ApiEvent::CompactionDivider {
                content,
                running,
                pre_tokens,
                post_tokens,
                trigger,
                ..
            } => {
                self.context_usage.compacting = *running;
                if *running {
                    if matches!(trigger.as_deref(), None | Some("manual")) {
                        self.manual_compaction_started();
                    }
                    self.show_compaction_running(content.clone());
                } else {
                    self.finish_compaction_divider(content.clone());
                    // The composer's context figure described the pre-compaction
                    // transcript until the next provider report; adopt the
                    // daemon's measured post-compaction size now.
                    if let Some(post_tokens) = post_tokens {
                        self.context_tokens = Some(*post_tokens);
                        self.set_compacted_context_estimate(*post_tokens);
                    }
                    if matches!(trigger.as_deref(), None | Some("manual")) {
                        self.finish_manual_context_compaction(
                            trigger.as_deref() == Some("manual"),
                            *pre_tokens,
                            *post_tokens,
                            cx,
                        );
                    } else {
                        self.refresh_context_usage(cx);
                    }
                }
            }
            ApiEvent::BackgroundTasks { snapshot, .. } => {
                self.apply_background_tasks(snapshot, cx);
                return;
            }
            ApiEvent::BackgroundProgress {
                task_id,
                summary,
                done: true,
                ..
            } if task_id == "context-compaction" => {
                self.context_usage.compacting = false;
                self.fail_compaction(summary);
                let reason = summary.lines().rev().find(|line| !line.trim().is_empty());
                self.settle_context_action(
                    context_usage::Action::Compact,
                    reason
                        .unwrap_or(summary)
                        .trim_start_matches("⚠ ")
                        .replace("**", ""),
                    true,
                    cx,
                );
            }
            ApiEvent::BackgroundProgress {
                task_id,
                label,
                summary,
                detail,
                percent,
                done,
                ..
            } => {
                if let Some(Item::BackgroundTask {
                    label: current_label,
                    summary: current_summary,
                    detail: current_detail,
                    percent: current_percent,
                    done: current_done,
                    ..
                }) = self.items.iter_mut().rev().find(|item| {
                    matches!(item, Item::BackgroundTask { task_id: id, .. } if id == task_id)
                }) {
                    *current_label = label.clone();
                    *current_summary = summary.clone();
                    if detail.is_some() {
                        *current_detail = detail.clone();
                    }
                    *current_percent = *percent;
                    *current_done = *done;
                } else {
                    self.items.push(Item::BackgroundTask {
                        task_id: task_id.clone(),
                        label: label.clone(),
                        summary: summary.clone(),
                        detail: detail.clone(),
                        percent: *percent,
                        done: *done,
                    });
                }
            }
            ApiEvent::SidePaneImages { images, .. } => {
                self.note_session_image_ordinal(images.iter().filter_map(|image| image.session_image.as_ref()).map(|image| image.ordinal).max(), cx);
                for image in images.iter().cloned() {
                    self.insert_rendered_image(image);
                }
            }
            ApiEvent::TurnDone { .. } => {
                self.refresh_context_usage(cx);
                self.reconcile_reconnect_response();
                self.flush_reasoning();
                self.flush_streaming();
                let outcome = self.finished_turn_outcome();
                self.settle_assistant_output(if outcome == turn::TurnOutcome::Done {
                    AssistantOutputPhase::Final
                } else {
                    AssistantOutputPhase::Update
                });
                self.finish_response();
                self.turn_outcome = Some(outcome);
            }
            ApiEvent::ProviderGuardrailRetry {
                turn_id,
                attempt,
                max_attempts,
                ..
            } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                let text = format!(
                    "Provider stopped the response. Jcode retried the request (attempt {attempt} of {max_attempts})."
                );
                if let Some(Item::ProviderGuardrailRetry { text: current, .. }) =
                    self.items.iter_mut().rev().find(|item| {
                        matches!(item, Item::ProviderGuardrailRetry { turn_id: id, .. } if id == turn_id)
                    })
                {
                    *current = text;
                } else {
                    self.items.push(Item::ProviderGuardrailRetry {
                        turn_id: turn_id.clone(),
                        text,
                    });
                }
            }
            ApiEvent::TurnStopped {
                reason,
                message,
                provider_stop_reason,
                ..
            } => {
                self.record_stop(turn::StopNotice::from_event(
                    reason,
                    message,
                    provider_stop_reason.as_deref(),
                ));
                self.dismiss_stopped_asks(cx);
            }
            ApiEvent::SessionStatus { status, .. }
                if status == "attached" || (status == "connected" && self.activity_active()) =>
            {
                // Transport bookkeeping is not a turn transition. In particular,
                // a late attach notification must not resurrect a completed turn.
            }
            ApiEvent::SessionStatus { status, .. } => {
                if let Some(notice) = turn::StopNotice::from_status(status) {
                    // New runtimes send a richer event before their legacy status.
                    if !matches!(self.items.last(), Some(Item::Stopped(notice)) if !notice.provisional)
                    {
                        self.record_stop(notice);
                    }
                    self.dismiss_stopped_asks(cx);
                    cx.notify();
                    return;
                }
                self.status = if status == "processing" {
                    "running".into()
                } else {
                    status.clone()
                };
                // The attachment can report idle before the daemon claims
                // recovery. A terminal event, not that snapshot, ends recovery.
                if status == "idle" && !self.continuing_after_reload {
                    self.reconcile_reconnect_response();
                    self.flush_reasoning();
                    self.settle_idle_output();
                    self.finish_response();
                }
            }
            ApiEvent::ConnectionPhase { phase, .. } => {
                self.connection_phase = phase.clone();
            }
            ApiEvent::ModelSwitch {
                state,
                model,
                error,
                ..
            } => self.apply_model_switch(*state, model.as_deref(), error.as_deref(), cx),
            ApiEvent::ModelInfo {
                provider,
                model,
                reasoning_effort,
                auth_method,
                account_label,
                ..
            } => {
                let previous_route = (
                    self.provider.clone(),
                    self.model.clone(),
                    self.auth_method.clone(),
                    self.account_label.clone(),
                );
                if provider.is_some() {
                    self.provider = provider.clone();
                }
                // A known model makes an absent effort authoritative. A wholly
                // empty identity does not erase the persisted fallback.
                if model.is_some() || reasoning_effort.is_some() {
                    self.identity_effort(reasoning_effort.clone());
                }
                // Only an actual switch invalidates the auth method: the route
                // catalog keyed it by model, but effort broadcasts repeat the
                // current model and must not wipe a still-correct label.
                if model.is_some() && *model != self.model {
                    self.model = model.clone();
                    self.auth_method = None;
                    self.account_label = None;
                }
                // The daemon's resolved credential is authoritative, including
                // an OAuth<->API key switch that keeps the same model id.
                if let Some(method) = auth_method.as_deref() {
                    self.auth_method = Some(human_auth_method(method));
                    // A resolved credential report is authoritative even when
                    // it clears a previous named OAuth account for an API key.
                    self.account_label = account_label.clone();
                } else if account_label.is_some() {
                    self.account_label = account_label.clone();
                }
                if previous_route
                    != (
                        self.provider.clone(),
                        self.model.clone(),
                        self.auth_method.clone(),
                        self.account_label.clone(),
                    )
                {
                    self.invalidate_context_meter();
                    self.refresh_context_usage(cx);
                    self.invalidate_speed_route(cx);
                    let update = |input: &mut PromptInput, cx: &mut Context<PromptInput>| {
                        input.set_serving_model(
                            self.model.as_deref(),
                            self.provider.as_deref(),
                            self.auth_method.as_deref(),
                            cx,
                        );
                    };
                    self.input.update(cx, update);
                    if let Some((PickerKind::Model, search)) = &self.picker_search {
                        search.update(cx, update);
                    }
                }
                self.sync_effort_menu(cx);
            }
            ApiEvent::RuntimeInfo {
                provider,
                model,
                reasoning_effort,
                auth_method,
                account_label,
                routes,
                speed_mode,
                speed_options,
                ..
            } => {
                if provider.is_some() {
                    self.provider = provider.clone();
                }
                // An empty runtime snapshot carries no identity yet. Keep the
                // persisted effort until the runtime actually reports a model.
                if model.is_some() || reasoning_effort.is_some() {
                    self.identity_effort(reasoning_effort.clone());
                }
                if model.is_some() {
                    self.model = model.clone();
                }
                self.account_label = account_label.clone();
                self.auth_method = auth_method
                    .as_deref()
                    .map(human_auth_method)
                    .or_else(|| auth_method_for_model(self.model.as_deref(), routes));
                // A runtime that answers with no routes (a catalog too big for a
                // live push arrives without them) says nothing about its models:
                // keep the list this panel already has.
                if !routes.is_empty() {
                    cx.set_global(KnownModelRoutes(routes.clone()));
                    let models = available_model_names(routes);
                    self.available_models = models.clone();
                    self.models_loaded = true;
                    self.models_error = None;
                    let current = self
                        .model
                        .as_ref()
                        .and_then(|model| {
                            let mut matching = routes.iter().filter(|route| {
                                route.available
                                    && route.model == *model
                                    && self
                                        .provider
                                        .as_ref()
                                        .is_none_or(|provider| route.provider == *provider)
                                    && self.auth_method.as_ref().is_none_or(|auth| {
                                        human_auth_method(&route.api_method) == *auth
                                    })
                            });
                            let route = matching.next()?;
                            matching
                                .next()
                                .is_none()
                                .then(|| crate::input::model_menu::route_spec(route))
                        })
                        .or_else(|| self.model.clone());
                    let update = |input: &mut PromptInput, cx: &mut Context<PromptInput>| {
                        input.set_model_routes(models.clone(), routes, current.clone(), cx);
                    };
                    self.input.update(cx, update);
                    if let Some((PickerKind::Model, search)) = &self.picker_search {
                        search.update(cx, update);
                    }
                }
                self.sync_effort_menu(cx);
                self.update_speed(*speed_mode, speed_options, cx);
            }
            ApiEvent::SpeedModeChanged { speed_mode, speed_options, .. } => {
                self.update_speed(*speed_mode, speed_options, cx);
            }
            ApiEvent::SpeedModeReported { requested, actual, .. } => {
                self.record_speed_report(*requested, *actual, cx);
            }
            ApiEvent::TokenUsage {
                input,
                cache_read_input,
                cache_creation_input,
                ..
            } => {
                self.context_tokens =
                    Some(jcode_base::compaction::effective_context_tokens_from_usage(
                        self.provider.as_deref().unwrap_or_default(),
                        *input,
                        *cache_read_input,
                        *cache_creation_input,
                    ));
            }
            // The daemon replays the persisted prompt occupancy when a session
            // attaches. It describes a request that completed before this
            // attachment, so it is context state only: no turn stats, no cost,
            // and no effect on the live stream counters.
            ApiEvent::ContextOccupancy { prompt_tokens, .. } => {
                self.context_tokens = Some(*prompt_tokens);
            }
            ApiEvent::ContextCapChanged { .. } => self.refresh_context_usage(cx),
            ApiEvent::RequestUsage { .. } | ApiEvent::CacheKeepalive { .. } => {
                self.cache_telemetry.observe(event)
            }
            ApiEvent::KvCacheMiss { .. } => {
                if let Some(notice) = cache_miss::CacheMissNotice::from_event(event) {
                    self.cache_telemetry.miss(&notice);
                    // Keep the notice in order: settle text streamed so far.
                    self.flush_reasoning();
                    self.flush_streaming();
                    self.items.push(Item::CacheMiss(notice));
                }
            }
            ApiEvent::SessionRenamed { display_title, .. } => {
                self.title = display_title.clone().into();
            }
            ApiEvent::Error { code, message } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.finish_response();
                self.turn_outcome = Some(turn::TurnOutcome::Error);
                if !matches!(self.items.last(), Some(Item::Stopped(notice)) if notice.detail == *message)
                {
                    self.items.push(Item::RuntimeError {
                        code: *code,
                        message: message.clone(),
                    });
                }
            }
            ApiEvent::SoftInterruptInjected { .. } => {
                self.settle_assistant_output(AssistantOutputPhase::Update);
            }
            _ => {}
        }
        self.observe_prompt_queue(event, cx);
        // Follow only during render, after pending user input. Installing an
        // end sentinel here replaces the painted scroll position before layout,
        // so an intervening upward wheel delta is clamped back to the bottom.
        cx.notify();
    }

    /// A turn that finishes out of view leaves a green "done" glow on its
    /// tab until the session is seen. A new turn clears it right away.
    pub(crate) fn note_turn_transition(&mut self, was_active: bool, cx: &mut Context<Self>) {
        let active = self.activity_active();
        let finished_unseen = match (was_active, active) {
            (false, true) => false,
            (true, false) => {
                !self.surface_focused && self.minimap_state() != MinimapSessionState::Error
            }
            _ => return,
        };
        if self.finished_unseen != finished_unseen {
            self.finished_unseen = finished_unseen;
            cx.notify();
        }
    }

    /// Advance the paced reveal of live reasoning and response text. Only the
    /// visible prefix is rendered, so bursts flow in instead of jumping.
    /// Returns true when motion is instant this frame (reduced motion, tests).
    pub(crate) fn tick_stream_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Render tests assert on painted text from a single frame.
        #[cfg(test)]
        let test_snap = !self.animate_stream_in_tests;
        #[cfg(not(test))]
        let test_snap = false;
        let instant = test_snap
            || self.preview_state.is_some()
            || cx.reduce_motion()
            || crate::config::get().appearance.reduce_motion;
        let now = cx.background_executor().now();
        let text = self.text_reveal.tick(&self.streaming_text, now, instant);
        let reasoning = self
            .reasoning_reveal
            .tick(&self.streaming_reasoning, now, instant);
        if text || reasoning {
            window.request_animation_frame();
        }
        instant
    }

    /// Raw streamed input of a tool call in this transcript.
    pub(crate) fn tool_input(&self, call_id: &str) -> Option<&str> {
        self.items.iter().rev().find_map(|item| match item {
            Item::Tool {
                call_id: id, input, ..
            } if id == call_id => Some(input.as_str()),
            _ => None,
        })
    }

    pub(crate) fn find_tool(&mut self, call_id: &str) -> Option<&mut Item> {
        // The legacy harness protocol streams `tool_input` without an id. The
        // bridge preserves that fact as an empty call_id, so associate those
        // deltas with the most recent active call. Exact ids still win for
        // modern providers and for overlapping completed calls.
        if call_id.is_empty() {
            return self
                .items
                .iter_mut()
                .rev()
                .find(|item| matches!(item, Item::Tool { done: false, .. }));
        }
        self.items.iter_mut().rev().find(
            |item| matches!(item, Item::Tool { call_id: existing, .. } if existing == call_id),
        )
    }

    pub fn message_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.prompt_queue.paused = true;
        self.sound_events.cancel();
        if self.preview_state.is_none() {
            crate::sounds::play(crate::sounds::Cue::Error, cx);
        }
        self.finish_response();
        self.turn_outcome = Some(turn::TurnOutcome::Error);
        self.items.push(Item::Error(message));
        self.status = "idle".into();
        self.connection_phase.clear();
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        cx.notify();
    }

    pub(super) fn insert_rendered(&mut self, image: jcode_sdk::RenderedImage) {
        // Side-pane events have no user-message boundary. Pasted images arrive
        // with the local echo or as part of history, never as tool side effects.
        if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
            return;
        }
        if self.items.iter().any(|item| {
            matches!(item, Item::Image(existing)
                if existing.anchor == image.anchor && existing.source == image.source
                    && match (&existing.session_image, &image.session_image) {
                        (Some(existing), Some(incoming)) => existing.image_id == incoming.image_id,
                        _ => existing.media_type == image.media_type && existing.data == image.data,
                    }
                    && (!matches!(&image.source, jcode_sdk::RenderedImageSource::ToolResult { tool_name } if tool_name == "ask") || existing.label == image.label))
        }) {
            return;
        }
        let insertion = match image.anchor.as_ref() {
            Some(jcode_sdk::RenderedImageAnchor::ToolCall { id }) => self
                .items
                .iter()
                .rposition(|item| matches!(item, Item::Tool { call_id, .. } if call_id == id))
                .map(|index| {
                    // A batch may return several images. Preserve their order
                    // instead of inserting each one directly after the tool.
                    index + 1
                        + self.items[index + 1..]
                            .iter()
                            .take_while(|item| {
                                matches!(item, Item::Image(existing) if existing.anchor == image.anchor)
                            })
                            .count()
                }),
            _ => None,
        };
        let insertion = insertion.unwrap_or_else(|| {
            // Unanchored images still belong after already-streamed text,
            // not before it when the stream is eventually flushed.
            self.flush_reasoning();
            self.flush_streaming();
            self.items.len()
        });
        self.insert_transcript_image(insertion, TranscriptImage::from_rendered(image));
    }

    pub(super) fn insert_transcript(&mut self, insertion: usize, image: TranscriptImage) {
        for index in &mut self.pending_users {
            if *index >= insertion {
                *index += 1;
            }
        }
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(index, at)| (index + usize::from(index >= insertion), at))
            .collect();
        self.user_times = std::mem::take(&mut self.user_times)
            .into_iter()
            .map(|(index, at)| (index + usize::from(index >= insertion), at))
            .collect();
        let shift = |index| index + usize::from(index >= insertion);
        self.offscreen_prompt = self.offscreen_prompt.map(shift);
        self.expanded_pinned_prompt = self.expanded_pinned_prompt.map(shift);
        if let Some(selected) = &mut self.image_pane_selected
            && *selected >= insertion
        {
            *selected += 1;
        }
        self.items.insert(insertion, Item::Image(image));
    }
}
