//! Applying runtime updates to the workspace.
use super::*;

/// Let the updater show its blocked-update dialog in a workspace window.
#[cfg(target_os = "macos")]
pub(super) fn install_update_dialog_host<T: 'static>(cx: &mut App) {
    cx.set_global(updates::DialogHost(std::rc::Rc::new(
        |app, preferred, sessions| {
            let window = app
                .windows()
                .into_iter()
                .filter_map(|window| window.downcast::<T>())
                .min_by_key(|window| window.window_id() != preferred)?;
            window
                .update(app, |_, window, cx| {
                    updates::show_pair_dialog(sessions, window, cx)
                })
                .ok()
        },
    )));
}

impl Workspace {
    pub(super) fn dispatch_bridge_batch(
        workspace: &Entity<Self>,
        updates: Vec<Update>,
        cx: &mut App,
    ) {
        let mut changed = false;
        for update in updates {
            match &update {
                Update::Event { session_id, event } => {
                    crate::render_stats::note_variant("event:", event);
                    if matches!(
                        event,
                        jcode_sdk::ApiEvent::TextDelta { .. }
                            | jcode_sdk::ApiEvent::ReasoningDelta { .. }
                            | jcode_sdk::ApiEvent::ToolInputDelta { .. }
                    ) {
                        let workspace = workspace.read(cx);
                        let owner = workspace.side_question_owner(session_id, cx);
                        let panel = workspace
                            .slots
                            .iter()
                            .find(|slot| {
                                (owner.is_none() || !slot.closing)
                                    && slot.panel.session_id(cx) == *session_id
                            })
                            .and_then(|slot| slot.panel.chat().cloned());
                        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
                        if let Some(owner) = owner {
                            owner
                                .update(cx, |panel, cx| panel.apply_side_question_event(event, cx));
                        }
                        if let Some(panel) = panel {
                            panel.update(cx, |panel, cx| panel.apply(event, cx));
                        }
                        continue;
                    }
                }
                other => crate::render_stats::note_variant("update:", other),
            }
            changed |= workspace.update(cx, |workspace, cx| workspace.apply(update, cx));
        }
        if changed {
            crate::render_stats::note("workspace-notify:bridge");
            cx.notify(workspace.entity_id());
        }
    }

    /// Apply a bridge update and report whether it changed visible workspace state.
    ///
    /// Session refreshes arrive every two seconds. Most contain the exact same
    /// SDK snapshot, so treating them as changes would rebuild the complete
    /// sidebar and every open panel for no user-visible result.
    pub(crate) fn apply(&mut self, mut update: Update, cx: &mut Context<Self>) -> bool {
        let _no_blocking_sdk_read = jcode_sdk::forbid_blocking_local_io();
        if let Update::Sessions { sessions } = &mut update {
            // A desktop window can disappear while its daemon-owned sessions keep
            // running. Those panels are restored from the workspace snapshot before
            // the asynchronous session catalog arrives. Older runtimes and bounded
            // catalogs can omit them, so never let a refresh erase an open session
            // from the sidebar.
            for slot in &self.slots {
                let Some(panel) = slot.panel.read_chat(cx) else {
                    continue;
                };
                if !panel.is_persisted_session()
                    || sessions
                        .iter()
                        .any(|session| session.session_id == panel.session_id)
                {
                    continue;
                }
                if let Some(known) = self
                    .sessions
                    .iter()
                    .find(|s| s.session_id == panel.session_id)
                {
                    sessions.push(known.clone());
                    continue;
                }
                sessions.push(jcode_sdk::SessionInfo {
                    user_connected: false,
                    session_id: panel.session_id.clone(),
                    working_dir: panel.working_dir.clone(),
                    title: Some(panel.title_text()),
                    status: "active".into(),
                    transcript_bytes: None,
                    saved: false,
                    updated_at_ms: None,
                    last_active_at_ms: None,
                    archived: false,
                    archived_at_ms: None,
                    save_label: None,
                    parent_session_id: None,
                    agent_label: None,
                    swarm_status: None,
                    edit_stats: None,
                    context: None,
                });
            }
        }
        if let Update::Sessions { sessions } = &mut update {
            self.live_swarm_status.merge_catalog(sessions);
            if same_session_catalog(&self.sessions, sessions) {
                // Keep exact recency metadata for future changes without
                // redrawing a catalog whose rows and order are unchanged.
                std::mem::swap(&mut self.sessions, sessions);
                return false;
            }
        }
        if let Update::WorkerFacts { workers } = &mut update {
            self.live_swarm_status.merge_worker_routes(workers);
            if workers == &self.worker_meta {
                return false;
            }
        }

        match update {
            Update::DraftModels { current, routes } => {
                Panel::publish_draft_models(current, routes, cx);
                for slot in &self.slots {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.seed_draft_models(cx));
                }
            }
            Update::Status(status) => {
                if self.status == status {
                    return false;
                }
                self.status = status;
            }
            Update::RemoteStatus {
                host,
                message,
                request_id,
                failed,
                failure,
            } => {
                if failed {
                    self.invalidate_cloud_connection(&host);
                }
                let label = machines::machine_label(&host);
                let key = machines::new_session_host(&host).to_owned();
                // A typed paired failure carries its own one-line sentence. Other
                // failures show the runtime's message, prefixed by the machine.
                let (line, notice) = match (&failure, failed) {
                    (Some(failure), true) => {
                        let summary = failure.summary(&label);
                        let notice = machines::RowNotice {
                            summary: summary.clone(),
                            action: Some(failure.action()),
                            detail: Some(failure.detail.clone()),
                        };
                        (summary, Some(notice))
                    }
                    (None, true) => {
                        let raw = message
                            .strip_prefix("Remote session failed: ")
                            .unwrap_or(&message);
                        let notice = machines::RowNotice {
                            summary: crate::friendly_error::summarize(raw),
                            action: Some(crate::connect_failure::FailureAction::Retry),
                            detail: crate::friendly_error::summary_hides_details(raw)
                                .then(|| raw.to_owned()),
                        };
                        (format!("{label}: {message}"), Some(notice))
                    }
                    _ => (format!("{label}: {message}"), None),
                };
                self.update_pending_remote_status(request_id.as_deref(), &line, failed, cx);
                self.remotes.failed = failed;
                self.remotes.status = Some(line);
                self.remotes.status_host = Some(key.clone());
                match notice {
                    Some(notice) => {
                        self.remotes.reachable.remove(&key);
                        self.remotes.row_notices.insert(key, notice);
                    }
                    None => {
                        self.remotes.row_notices.remove(&key);
                        if message.starts_with("Connected") {
                            self.remotes.reachable.insert(key);
                        } else {
                            self.remotes.reachable.remove(&key);
                        }
                    }
                }
            }
            Update::RemoteSessions { request_id, result } => {
                self.apply_machine_sessions(request_id, result);
            }
            Update::SessionSearch {
                query,
                generation,
                result,
                elapsed_ms,
            } => self.apply_session_search(query, generation, result, elapsed_ms, cx),
            Update::Connected => {
                self.connected = true;
                self.status = "connected".into();
                if let Some(state) = self.resume.as_mut() {
                    state.start_on_close = true;
                } else if self.slots.get(self.active).is_some_and(|slot| {
                    slot.panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_startup_draft)
                }) {
                    self.start_default_startup(cx);
                }
            }
            Update::WorkerFacts { workers } => {
                self.worker_meta = workers;
                // Rows read the records, so the strip and the sidebar rebuild;
                // panels take the model their own record names when the
                // runtime has not sent an identity yet.
                self.sync_swarm_strips(cx);
            }
            Update::CatalogTodos(todos) => {
                let progress_changed = self.sidebar_todo_progress != todos.progress;
                let catalog = crate::applet_view::CatalogTasks {
                    rows: std::rc::Rc::new(todos.rows),
                    progress: std::rc::Rc::new(todos.progress.clone()),
                };
                if cx.try_global::<crate::applet_view::CatalogTasks>() != Some(&catalog) {
                    cx.set_global(catalog.clone());
                }
                if self.catalog_tasks != catalog {
                    // Another window may already have written the global.
                    // Compare with what this window last displayed.
                    for slot in &self.slots {
                        let Some(panel) = slot.panel.read_chat(cx) else {
                            continue;
                        };
                        let id = &panel.session_id;
                        if self.catalog_tasks.session(id) != catalog.session(id)
                            && panel.uses_catalog_todos()
                        {
                            App::notify(cx, slot.panel.entity_id());
                        }
                    }
                    self.catalog_tasks = catalog;
                }
                self.sidebar_todo_progress = todos.progress;
                if self.unfinished_sessions == todos.unfinished {
                    return progress_changed;
                }
                self.unfinished_sessions = todos.unfinished;
                crate::example_prompts::set(
                    crate::example_prompts::from_unfinished(&self.unfinished_sessions),
                    cx,
                );
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == "unfinished-work" {
                        let unfinished = self.unfinished_sessions.clone();
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.set_unfinished_work(unfinished, cx));
                    }
                }
                return true;
            }
            Update::Sessions { sessions } => {
                if self.pinned_working_dir.is_none()
                    && let Some(directory) = most_used_working_dir(&sessions)
                {
                    let persisted = directory.clone();
                    cx.background_executor()
                        .spawn(async move {
                            if let Err(error) =
                                crate::config::persist_pinned_working_dir(&persisted)
                            {
                                eprintln!("could not persist fixed directory shortcut: {error}");
                            }
                        })
                        .detach();
                    self.pinned_working_dir = Some(directory);
                }
                for session in &sessions {
                    for slot in &self.slots {
                        let Some(panel) = slot.panel.read_chat(cx) else {
                            continue;
                        };
                        if panel.session_id != session.session_id {
                            continue;
                        }
                        let title = session
                            .title
                            .as_ref()
                            .filter(|title| !panel.is_titled(title));
                        // Restored panels can predate directory metadata. Refresh it
                        // along with the title so the identity footer stays current.
                        // Missing catalog metadata must not erase a known directory.
                        let directory = session
                            .working_dir
                            .as_ref()
                            .filter(|dir| !dir.trim().is_empty() && !panel.is_in(dir));
                        if title.is_some() || directory.is_some() {
                            slot.panel.update_chat(cx, |panel, cx| {
                                if let Some(title) = title {
                                    panel.set_title(title.clone(), cx);
                                }
                                if let Some(directory) = directory {
                                    panel.set_working_dir(Some(directory.clone()), cx);
                                }
                            });
                        }
                        break;
                    }
                }
                self.sessions = sessions;
                self.sync_swarm_strips(cx);
                if !cfg!(test) {
                    // Swarm cards show each named worker's own model and total
                    // spend, read from its session files off the UI thread.
                    let wanted = self
                        .slots
                        .iter()
                        .filter_map(|slot| {
                            let panel = slot.panel.chat()?;
                            Some((panel.clone(), panel.read(cx).swarm_card_worker_ids()))
                        })
                        .filter(|(_, ids)| !ids.is_empty())
                        .collect::<Vec<_>>();
                    if !wanted.is_empty()
                        && let Some(home) = crate::harness::jcode_home()
                    {
                        let ids = wanted
                            .iter()
                            .flat_map(|(_, ids)| ids.clone())
                            .collect::<Vec<_>>();
                        cx.spawn(async move |_, cx| {
                            let usage = cx
                                .background_executor()
                                .spawn(async move {
                                    jcode_desktop_harness::swarm_usage::usage_for(&home, &ids)
                                })
                                .await;
                            for (panel, ids) in wanted {
                                let own = ids
                                    .iter()
                                    .filter_map(|id| Some((id.clone(), usage.get(id)?.clone())))
                                    .collect();
                                panel.update(cx, |panel, cx| panel.set_swarm_usage(own, cx));
                            }
                        })
                        .detach();
                    }
                }
            }
            Update::SessionCreated {
                session,
                request_id,
            } => {
                if request_id
                    .as_deref()
                    .is_some_and(Panel::is_pending_session_id)
                {
                    if let Some(slot) = self.slots.iter().find(|slot| {
                        !slot.closing && Some(slot.panel.session_id(cx)) == request_id.as_deref()
                    }) {
                        let session_id = session.session_id.clone();
                        let working_dir = session.working_dir.clone();
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.attach_startup_session(session, cx));
                        if let Some(request_id) = request_id.as_deref() {
                            self.resolve_collection_draft(request_id, &session_id, cx);
                        }
                        if request_id
                            .as_deref()
                            .is_some_and(crate::publish::is_publish_draft)
                        {
                            self.start_publish_session(session_id, working_dir.as_deref());
                        } else if request_id
                            .as_deref()
                            .is_some_and(|id| id.starts_with("startup://draft/help/"))
                        {
                            self.bridge.send(Command::Send {
                                session_id,
                                content: HELP_SESSION_PROMPT.into(),
                                images: Vec::new(),
                                attachments: Vec::new(),
                            });
                        }
                    } else {
                        // The user closed the draft during startup. Do not
                        // reopen it or steal focus when creation finishes.
                        self.pending_collection_drafts
                            .retain(|(draft, _)| Some(draft.as_str()) != request_id.as_deref());
                        self.bridge.send(Command::Unwatch {
                            session_id: session.session_id,
                        });
                    }
                    return true;
                }
                // A brand-new panel is only a local draft until its first prompt.
                // The persisted/runtime session refresh adds it after activity.
                let inserted = self.open_session(session, cx);
                self.set_active(inserted, cx);
                self.focus_pending = true;
            }
            Update::SessionForked { session, prompt } => {
                let session_id = session.session_id.clone();
                if !self
                    .sessions
                    .iter()
                    .any(|known| known.session_id == session_id)
                {
                    self.sessions.push(session.clone());
                }
                if self.single_panel {
                    self.open_fork_window(&session_id);
                    return true;
                }
                let inserted = self.open_session(session, cx);
                self.set_active(inserted, cx);
                self.focus_pending = true;
                // The harness already delivered the prompt over the child's
                // connection. Paint it here so the fork never opens looking
                // empty while the answer streams in.
                if let Some(prompt) = prompt {
                    self.slots[inserted]
                        .panel
                        .update_chat(cx, |panel, cx| panel.echo_external_prompt(prompt, cx));
                }
            }
            Update::SideQuestionStarted {
                parent_session_id,
                session,
                question,
            } => {
                // A side question answers inside the source chat's card. It is
                // never a tab of its own and never touches the source session.
                if let Some(index) = self.slots.iter().position(|slot| {
                    !slot.closing && slot.panel.session_id(cx) == parent_session_id
                }) {
                    self.slots[index].panel.update_chat(cx, |panel, cx| {
                        panel.start_side_question(session, question, cx)
                    });
                }
            }
            Update::SideQuestionHistory {
                parent_session_id,
                request_id,
                result,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| !slot.closing && slot.panel.session_id(cx) == parent_session_id)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.load_side_question_history(request_id, result, cx)
                    });
                }
            }
            Update::SideQuestionOpened {
                parent_session_id,
                request_id,
                result,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| !slot.closing && slot.panel.session_id(cx) == parent_session_id)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.load_saved_side_question(request_id, result, cx)
                    });
                }
            }
            Update::History {
                session_id,
                messages,
                images,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.load_history(messages, images, cx);
                        });
                        break;
                    }
                }
            }
            Update::HistoryPage { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_older_history(result, cx));
                        break;
                    }
                }
            }
            Update::BoundedHistory { session_id, page } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_bounded_history(page, cx));
                        break;
                    }
                }
            }
            Update::RewoundHistory {
                session_id,
                page,
                bounded,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.replace_rewound_history(page, bounded, cx)
                        });
                        break;
                    }
                }
            }
            Update::BoundedHistoryPage { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.load_bounded_history_page(result, cx)
                        });
                        break;
                    }
                }
            }
            Update::HistoryImage {
                session_id,
                key,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.load_history_image(key, result, cx));
                        break;
                    }
                }
            }
            Update::Event { session_id, event } => {
                if let jcode_sdk::ApiEvent::SwarmStatus {
                    session_id: source,
                    updated_at_unix_ms,
                    members,
                } = &event
                {
                    if self
                        .live_swarm_status
                        .apply(source, *updated_at_unix_ms, members)
                    {
                        let unknown_worker = members.iter().any(|member| {
                            member.parent_session_id.is_some()
                                && !self
                                    .sessions
                                    .iter()
                                    .any(|session| session.session_id == member.session_id)
                        });
                        let previous_routes = self.worker_meta.clone();
                        self.live_swarm_status.merge(&mut self.sessions);
                        self.live_swarm_status
                            .merge_worker_routes(&mut self.worker_meta);
                        // A worker panel may have retained the model reported on
                        // attach. A newer live route must update that identity too,
                        // not just the sidebar's polled worker record.
                        for slot in &self.slots {
                            let id = slot.panel.session_id(cx);
                            let Some(route) = self.worker_meta.get(id) else {
                                continue;
                            };
                            let Some(previous) = previous_routes.get(id) else {
                                continue;
                            };
                            if route.model.is_some()
                                && (route.model != previous.model
                                    || route.provider != previous.provider
                                    || route.effort != previous.effort)
                            {
                                let event = jcode_sdk::ApiEvent::ModelInfo {
                                    session_id: id.to_owned(),
                                    model: route.model.clone(),
                                    provider: route.provider.clone(),
                                    reasoning_effort: route.effort.clone(),
                                    auth_method: None,
                                    account_label: None,
                                };
                                slot.panel
                                    .update_chat(cx, |panel, cx| panel.apply(&event, cx));
                            }
                        }
                        self.sync_swarm_strips(cx);
                        if unknown_worker {
                            self.bridge.send(Command::RefreshSessions);
                        }
                        return true;
                    }
                    return false;
                }
                if self.route_side_question_event(&session_id, &event, cx) {
                    return false;
                }
                if let jcode_sdk::ApiEvent::FocusRequested { .. } = &event {
                    // Another session (an agent) asked the user to look at this
                    // one: select its tab and bring the app forward.
                    return self.raise_session(&session_id, cx);
                }
                if let jcode_sdk::ApiEvent::SidePanelState { snapshot, .. } = &event {
                    return self.apply_side_panel(&session_id, snapshot, cx);
                }
                if let jcode_sdk::ApiEvent::AppletState { snapshot, .. } = &event {
                    // Keyed by the panel's session id, which for remote
                    // hosts is namespaced, so instances land in that panel.
                    return crate::applet_runtime::get(cx).sync_agent(&session_id, snapshot);
                }
                self.forward_tool_call(&session_id, &event, cx);
                if sidebar_edits::refresh_after(&event) {
                    self.bridge.send(Command::RefreshSessions);
                }
                let mut changed = false;
                if matches!(
                    event,
                    jcode_sdk::ApiEvent::MessageAccepted { .. }
                        | jcode_sdk::ApiEvent::PeerUserMessage { .. }
                        | jcode_sdk::ApiEvent::ToolDone { .. }
                        | jcode_sdk::ApiEvent::TurnDone { .. }
                        | jcode_sdk::ApiEvent::TurnStopped { .. }
                ) && let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|session| session.session_id == session_id)
                {
                    // Keep active work near the top like the TUI picker, but
                    // use message, tool and turn boundaries instead of tokens.
                    let now = unix_now_ms();
                    if session.updated_at_ms.is_none_or(|previous| previous < now) {
                        session.updated_at_ms = Some(now);
                        changed = true;
                    }
                }
                let updated_title = match &event {
                    jcode_sdk::ApiEvent::SessionRenamed { display_title, .. } => {
                        Some(display_title.clone())
                    }
                    jcode_sdk::ApiEvent::ToolDone { name, error, .. }
                        if name == "todo" && error.is_none() =>
                    {
                        // Todo-derived titles are SDK session metadata rather
                        // than explicit RenameSession events. Refresh the list
                        // as soon as the SDK reports a successful todo write.
                        self.bridge.send(Command::RefreshSessions);
                        None
                    }
                    _ => None,
                };
                if let Some(title) = updated_title.as_deref()
                    && self.sessions.iter().any(|session| {
                        session.session_id == session_id && session.parent_session_id.is_some()
                    })
                {
                    self.live_swarm_status.note_title(&session_id, title);
                    self.bridge.send(Command::RefreshSessions);
                }
                if let Some(display_title) = updated_title
                    && let Some(session) = self
                        .sessions
                        .iter_mut()
                        .find(|session| session.session_id == session_id)
                    && session.title.as_ref() != Some(&display_title)
                {
                    // SessionInfo carries the effective display title, so keep
                    // the sidebar's cached copy in sync with the live panel.
                    session.title = Some(display_title);
                    changed = true;
                    if session.parent_session_id.is_some() {
                        self.sync_swarm_strips(cx);
                    }
                }
                let swarm_tool_finished =
                    matches!(&event, jcode_sdk::ApiEvent::ToolDone { name, .. } if name == "swarm");
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.apply(&event, cx);
                            if swarm_tool_finished {
                                panel.refresh_local_pending_awaits(cx);
                            }
                        });
                        if matches!(event, jcode_sdk::ApiEvent::TurnDone { .. })
                            && let Some(panel) = slot.panel.read_chat(cx)
                            && let Some(provider) = panel.provider.as_deref()
                        {
                            let id =
                                accounts::credential_id(provider, panel.auth_method.as_deref());
                            if self.recent_accounts.first() != Some(&id) {
                                accounts::record_use(&mut self.recent_accounts, id);
                                changed = true;
                            }
                        }
                        break;
                    }
                }
                return changed;
            }
            Update::SendFailed { session_id, reason } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.message_failed(format!("message failed: {reason}"), cx);
                        });
                        break;
                    }
                }
            }
            Update::MessageSubmitted { .. } => {}
            Update::SessionSaved {
                session_id,
                saved,
                label,
            } => {
                let label = label
                    .map(|label| label.trim().to_string())
                    .filter(|label| !label.is_empty());
                if let Some(session) = self
                    .sessions
                    .iter_mut()
                    .find(|session| session.session_id == session_id)
                {
                    session.saved = saved;
                    if !saved {
                        session.save_label = None;
                    } else if let Some(label) = &label {
                        session.save_label = Some(label.clone());
                        session.title = Some(label.clone());
                    }
                }
                if saved && let Some(label) = &label {
                    for slot in &self.slots {
                        if slot.panel.session_id(cx) == session_id {
                            slot.panel
                                .update_chat(cx, |panel, cx| panel.set_title(label.clone(), cx));
                        }
                    }
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if panel.session_saved_settled(saved, label.as_deref(), None) {
                                cx.notify();
                            }
                        });
                    }
                }
                self.bridge.send(Command::RefreshSessions);
            }
            Update::SessionSaveFailed {
                session_id,
                saved,
                error,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if !panel.session_saved_settled(saved, None, Some(&error)) {
                                let verb = if saved { "save" } else { "unsave" };
                                panel
                                    .report_error(format!("Failed to {verb} session: {error}"), cx);
                            }
                            crate::sounds::play(crate::sounds::Cue::Error, cx);
                            cx.notify();
                        });
                        break;
                    }
                }
                // Undo the optimistic sidebar state from a pin click.
                self.bridge.send(Command::RefreshSessions);
            }
            Update::TodoState { session_id, state } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.todo_state_received(state, cx);
                        });
                        break;
                    }
                }
            }
            Update::TabWaitingState {
                session_id,
                waiting,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| slot.panel.session_id(cx) == session_id)
                {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.set_tab_waiting(waiting, cx));
                }
            }
            Update::EffortSettled {
                session_id,
                effort,
                error,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.effort_settled(&effort, error.as_deref(), cx);
                        });
                        break;
                    }
                }
            }
            Update::SpeedSettled {
                session_id,
                serial,
                result,
            } => {
                if let Some(slot) = self
                    .slots
                    .iter()
                    .find(|slot| slot.panel.session_id(cx) == session_id)
                {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.speed_settled(serial, result, cx));
                }
            }
            Update::AskAttachment {
                session_id,
                attachment_id,
                pending_ask_ids,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.reconcile_asks(attachment_id, pending_ask_ids.as_deref(), cx);
                        });
                    }
                }
            }
            Update::AskResponseSettled {
                session_id,
                request_id,
                attempt_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.ask_response_settled(&request_id, attempt_id, result.clone(), cx);
                        });
                    }
                }
            }
            Update::RoutingGateAnswerSettled {
                session_id,
                gate_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.routing_gate_answer_settled(&gate_id, result.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::RoutingIntentReply { session_id, result } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.routing_intent_reply(result.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::QueueMutationCapability {
                session_id,
                supported,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_mutations_supported(supported, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueueEditingCapability {
                session_id,
                supported,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_editing_supported(supported, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueuedPromptEdited {
                session_id,
                prompt_id,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queued_prompt_edited(&prompt_id, result.clone(), cx)
                        });
                        break;
                    }
                }
            }
            Update::QueueRemovalSettled {
                session_id,
                prompt,
                edit,
                result,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queue_removal_settled(prompt, edit, result, cx)
                        });
                        break;
                    }
                }
            }
            Update::QueuePromptFailed {
                session_id,
                client_id,
                reason,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.queue_prompt_failed(&client_id, reason.clone(), cx);
                        });
                        break;
                    }
                }
            }
            Update::CommandFailed { session_id, reason } => {
                if self.route_side_question_event(
                    &session_id,
                    &jcode_sdk::ApiEvent::Error {
                        code: jcode_sdk::api::ErrorCode::Internal,
                        message: reason.clone(),
                    },
                    cx,
                ) {
                    return true;
                }
                if session_id == Panel::STARTUP_SESSION_ID {
                    self.remotes.startup_failed = true;
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            if panel.is_pending_session() {
                                panel.set_status("Session creation failed · draft retained", cx);
                            }
                            panel.report_error(reason.clone(), cx);
                            crate::sounds::play(crate::sounds::Cue::Error, cx);
                            cx.notify();
                        });
                        break;
                    }
                }
            }
            Update::Disconnected { reason } => {
                self.connected = false;
                self.status = format!(
                    "{} Reconnecting…",
                    crate::friendly_error::summarize(&reason)
                );
            }
            Update::SessionLost { session_id, reason } => {
                self.set_cloud_transport_connected(&session_id, false);
                if let Some(host) = harness::remote_host(&session_id) {
                    self.invalidate_cloud_connection(&host);
                }
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_queue_mutations_supported(false, cx);
                            panel.set_tab_waiting(false, cx);
                            panel.connection_lost(&reason, cx);
                        });
                        break;
                    }
                }
            }
            Update::SessionAttaching { session_id } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_status("attaching…", cx);
                        });
                        break;
                    }
                }
            }
            Update::OpenrigHostIdentity {
                session_id,
                self_host_id,
            } => {
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.set_rig_host_id(self_host_id.clone(), cx);
                        });
                    }
                }
            }
            Update::SessionConnected { session_id } => {
                self.set_cloud_transport_connected(&session_id, true);
                for slot in &self.slots {
                    if slot.panel.session_id(cx) == session_id {
                        slot.panel.update_chat(cx, |panel, cx| {
                            // A subsequent SessionStatus event will replace this
                            // with idle/running. Clear the stale lost banner now.
                            panel.set_status("connected", cx);
                            panel.refresh_local_pending_awaits(cx);
                            panel.prime_context_meter(cx);
                            cx.notify();
                        });
                        break;
                    }
                }
            }
        }
        true
    }
}
