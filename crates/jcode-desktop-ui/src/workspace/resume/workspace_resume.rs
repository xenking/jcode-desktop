//! A transient, keyboard-first session browser and search. It never becomes a
//! runtime slot.
//!
//! One screen answers both "which session do I want back" and "where did I
//! talk about X": typing filters sessions by title, folder and ID locally, and
//! asks the daemon's full-text index for matching messages. Message hits are
//! listed under their session, the preview scrolls to the selected hit with
//! the query marked, and opening a hit lands the session on that message.
use super::screens::{Filter, Snapshot, State};
use super::*;
use crate::global_search::{self, Effect, PickRow};
use crate::panel::PreviewScroll;
use std::time::Instant;

pub(super) const RESUME_ARROW: &[u8] = include_bytes!("../../../../../assets/icons/arrow-up.svg");
pub(super) const RESUME_STACKED_WIDTH: f32 = 640.;

/// How long an opened session's transcript is polled for the message to
/// reveal. History arrives asynchronously; this covers a slow daemon without
/// leaving a timer running forever.
pub(super) const REVEAL_WINDOW: Duration = Duration::from_secs(6);
pub(super) const REVEAL_INTERVAL: Duration = Duration::from_millis(60);

pub(super) const PRESENCE_REFRESH: Duration = Duration::from_millis(1500);

/// How a row relates to live work, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Liveness {
    Working,
    Open,
    /// Not running, and its last owner died without a clean shutdown.
    Crashed,
    Inactive,
}

pub(super) fn status_is_crashed(status: &str) -> bool {
    status == "crashed"
}

pub(super) fn status_is_working(status: &str) -> bool {
    matches!(
        status,
        "running"
            | "working"
            | "busy"
            | "streaming"
            | "generating"
            | "thinking"
            | "running_tools"
            | "compacting"
            | "processing"
    )
}

/// Streaming markers written by every live jcode process, filtered to
/// sessions whose owning process is still alive.
pub(super) fn streaming_sessions() -> HashSet<String> {
    if cfg!(test) || harness::screenshot_mode() {
        return HashSet::new();
    }
    jcode_base::session::session_presence()
        .into_iter()
        .filter(|presence| presence.streaming)
        .map(|presence| presence.session_id)
        .collect()
}

#[cfg(test)]
use crate::commands::is_resume_command;

/// Extract a session ID from a pasted query such as `session_x_123_abc` or
/// `jcode --resume session_x_123_abc`.
pub(super) fn pasted_session_id(query: &str) -> Option<String> {
    let mut words = query.split_whitespace().peekable();
    if words.peek() == Some(&"jcode") {
        words.next();
    }
    if matches!(words.peek(), Some(&"--resume") | Some(&"-r")) {
        words.next();
    }
    let id = words
        .next()?
        .trim_matches(|c| c == '`' || c == '"' || c == '\'' || c == '.');
    (words.next().is_none()
        && id.starts_with("session_")
        && id.len() > "session_".len()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then(|| id.to_owned())
}

/// `content` lists sessions the conversation search matched, best first.
pub(super) fn filtered_sessions(
    mut sessions: Vec<jcode_sdk::SessionInfo>,
    query: &str,
    filter: Filter,
    working: &HashSet<String>,
    content: &[String],
    preferences: sidebar_session_catalog::Preferences,
) -> Vec<jcode_sdk::SessionInfo> {
    let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut metadata_hits = HashSet::new();
    sessions.retain(|session| {
        let text = format!(
            "{} {} {} {} {} {}",
            session.session_id,
            session.title.as_deref().unwrap_or_default(),
            session.save_label.as_deref().unwrap_or_default(),
            session.working_dir.as_deref().unwrap_or_default(),
            session.status,
            session.agent_label.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        !session.archived
            && preferences.origin.matches(session)
            && match filter {
                Filter::All => true,
                Filter::Active => {
                    working.contains(&session.session_id) || status_is_working(&session.status)
                }
                Filter::Saved => session.saved,
                Filter::Crashed => {
                    status_is_crashed(&session.status) && !working.contains(&session.session_id)
                }
            }
            && if words.iter().all(|word| text.contains(word)) {
                metadata_hits.insert(session.session_id.clone());
                true
            } else {
                content.contains(&session.session_id)
            }
    });
    let rank = |id: &str| {
        content
            .iter()
            .position(|hit| hit == id)
            .unwrap_or(usize::MAX)
    };
    // Title, folder, and ID matches outrank sessions found only by transcript,
    // which keep the index's relevance order.
    sessions.sort_by(|a, b| {
        metadata_hits
            .contains(&b.session_id)
            .cmp(&metadata_hits.contains(&a.session_id))
            .then_with(|| {
                if metadata_hits.contains(&a.session_id) {
                    std::cmp::Ordering::Equal
                } else {
                    rank(&a.session_id).cmp(&rank(&b.session_id))
                }
            })
            .then_with(|| match preferences.sort {
                sidebar_session_catalog::Sort::Recent => std::cmp::Ordering::Equal,
                sidebar_session_catalog::Sort::AgentActivity => {
                    resume_activity_at_ms(b).cmp(&resume_activity_at_ms(a))
                }
            })
            .then_with(|| b.saved.cmp(&a.saved))
            .then_with(|| {
                b.last_active_at_ms
                    .or(b.updated_at_ms)
                    .cmp(&a.last_active_at_ms.or(a.updated_at_ms))
            })
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    let mut seen = HashSet::new();
    sessions.retain(|session| seen.insert(session.session_id.clone()));
    sessions
}

impl Workspace {
    pub(super) fn resume_snapshot(&self, cx: &App) -> Snapshot {
        match &self.resume {
            None => Snapshot::Closed,
            Some(state) => Snapshot::Open {
                search: state.search.read(cx).snapshot(),
                query: state.query.clone(),
                selected: state.selected.clone(),
                filter: state.filter,
                start_on_close: state.start_on_close,
            },
        }
    }

    pub(super) fn restore_resume(
        &mut self,
        snapshot: Option<Snapshot>,
        resume_requested: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resume = None;
        match snapshot {
            Some(Snapshot::Open {
                search,
                query,
                selected,
                filter,
                start_on_close,
            }) => {
                self.open_resume(&OpenResume, window, cx);
                let state = self.resume.as_mut().unwrap();
                state
                    .search
                    .update(cx, |input, cx| input.restore(search, cx));
                state.query = query.clone();
                state.selected = selected;
                state.filter = filter;
                state.start_on_close = start_on_close;
                self.resume_query_changed(&query, cx);
            }
            None | Some(Snapshot::Legacy)
                if resume_requested
                // Old linked hosts omitted picker state. Recover only while the
                // active slot is still the unstarted startup draft, not a chat.
                && (snapshot.is_none()
                    || self
                        .slots
                        .get(self.active)
                        .is_some_and(|slot| slot.panel.read_chat(cx).is_some_and(Panel::is_startup_draft))) =>
            {
                self.open_resume(&OpenResume, window, cx);
                self.resume.as_mut().unwrap().start_on_close = true;
            }
            _ => {}
        }
    }

    pub(super) fn resume_sessions(&self, cx: &App) -> Vec<jcode_sdk::SessionInfo> {
        let mut sessions = self.sessions.clone();
        append_resume_fixture(&mut sessions);
        // Keep local drafts and already-open chats reachable when switching the
        // standalone window's primary conversation. Never discard their editors.
        for slot in &self.slots {
            let Some(panel) = slot.panel.read_chat(cx) else {
                continue;
            };
            if !slot.closing
                && panel.supports_voice()
                && panel.listable_conversation(cx)
                && !sessions.iter().any(|s| s.session_id == panel.session_id)
            {
                sessions.push(jcode_sdk::SessionInfo {
                    user_connected: false,
                    session_id: panel.session_id.clone(),
                    title: Some(panel.title_text()),
                    working_dir: panel.working_dir.clone(),
                    status: "open".into(),
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
        // A conversation match can name a session the loaded catalog does not
        // hold yet. It is still a real stored session, so list it.
        if let Some(state) = &self.resume {
            for (session_id, title) in state.daemon.matched_sessions() {
                if !sessions.iter().any(|s| s.session_id == session_id) {
                    sessions.push(jcode_sdk::SessionInfo {
                        user_connected: false,
                        title: (!title.trim().is_empty()).then_some(title),
                        session_id,
                        working_dir: None,
                        status: "idle".into(),
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
        }
        sessions
    }

    /// Sessions open in this window, and whether each is mid-turn.
    pub(super) fn resume_open_activity(&self, cx: &App) -> HashMap<String, bool> {
        self.slots
            .iter()
            .filter(|slot| !slot.closing)
            .filter_map(|slot| slot.panel.read_chat(cx))
            .map(|panel| (panel.session_id.clone(), panel.sidebar_activity().is_some()))
            .collect()
    }

    pub(super) fn resume_working(&self, cx: &App) -> HashSet<String> {
        let mut working = self
            .resume
            .as_ref()
            .map(|state| state.streaming.clone())
            .unwrap_or_default();
        working.extend(
            self.resume_open_activity(cx)
                .into_iter()
                .filter_map(|(id, active)| active.then_some(id)),
        );
        working
    }

    pub(super) fn resume_matches(&self, cx: &App) -> Vec<jcode_sdk::SessionInfo> {
        let Some(state) = &self.resume else {
            return Vec::new();
        };
        let content: Vec<String> = state
            .daemon
            .matched_sessions()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        filtered_sessions(
            self.resume_sessions(cx),
            &state.query,
            state.filter,
            &self.resume_working(cx),
            &content,
            self.session_catalog,
        )
    }

    /// Every list row: each matching session, then its message hits.
    pub(super) fn resume_rows(&self, matches: &[jcode_sdk::SessionInfo]) -> Vec<PickRow> {
        let Some(state) = &self.resume else {
            return Vec::new();
        };
        let ids: Vec<String> = matches.iter().map(|s| s.session_id.clone()).collect();
        state.daemon.rows(&ids)
    }

    pub(super) fn resume_selected_row(&self) -> Option<PickRow> {
        let state = self.resume.as_ref()?;
        let session_id = state.selected.clone()?;
        Some(match state.selected_hit {
            Some(hit) => PickRow::Hit { session_id, hit },
            None => PickRow::Session(session_id),
        })
    }

    pub(super) fn set_resume_selection(&mut self, row: Option<PickRow>) {
        if let Some(state) = &mut self.resume {
            state.selected_hit = row.as_ref().and_then(PickRow::hit);
            state.selected = row.map(|row| row.session_id().to_owned());
        }
    }

    /// Apply new query text: the local filter reacts at once, the
    /// conversation search after the debounce.
    pub(super) fn resume_query_changed(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(state) = self.resume.as_mut() else {
            return;
        };
        let generation = match state.daemon.set_query(query) {
            Effect::Cleared => {
                if state.daemon.status() == global_search::Status::Idle {
                    state._debounce = None;
                }
                return;
            }
            Effect::Debounce(generation) => generation,
        };
        // A pasted session ID opens directly; it is not a conversation query.
        if pasted_session_id(query).is_some() {
            state._debounce = None;
            return;
        }
        // Each keystroke replaces the pending timer, so only the last one sends.
        state._debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(global_search::DEBOUNCE)
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.resume.as_mut() else {
                    return;
                };
                if !state.daemon.mark_sent(generation) {
                    return;
                }
                let query = state.daemon.query().to_string();
                cx.notify();
                this.bridge
                    .send(Command::SearchSessions { query, generation });
            });
        }));
    }

    pub(super) fn apply_session_search(
        &mut self,
        _query: String,
        generation: u64,
        result: Result<jcode_sdk::SessionSearchResults, String>,
        elapsed_ms: f64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.resume.as_mut() else {
            return;
        };
        let applied = match result {
            Ok(results) => {
                state
                    .daemon
                    .apply_results(generation, convert_results(results), elapsed_ms)
            }
            Err(error) => state.daemon.apply_error(generation, error),
        };
        if applied {
            // A delayed search reply must not undo a selection made while it ran.
            state.preview_revealed = None;
            cx.notify();
        }
    }

    /// Mark the query in a transcript panel and scroll it to `row`'s message.
    pub(super) fn reveal_in_preview(
        &mut self,
        panel: &Entity<Panel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.resume.as_ref() else {
            return;
        };
        let terms = global_search::query_terms(&state.query);
        let row = self.resume_selected_row();
        let needle = row.as_ref().and_then(|row| state.daemon.reveal_for(row));
        let exact_hit = row.as_ref().is_some_and(|row| row.hit().is_some());
        let reveal = state.preview_revealed != row && !terms.is_empty();
        panel.update(cx, |panel, cx| panel.set_search_terms(&terms, cx));
        if reveal {
            let panel = panel.clone();
            window.defer(cx, move |_, cx| {
                panel.update(cx, |panel, cx| {
                    if panel.search_terms == terms {
                        panel.reveal_transcript_match(needle.as_deref(), exact_hit, cx);
                    }
                });
            });
        }
        if let Some(state) = self.resume.as_mut() {
            state.preview_revealed = row;
        }
    }

    /// Poll the opened transcript until the searched message appears, then
    /// scroll to it. Opened sessions load history asynchronously, so the
    /// first frames have nothing to scroll to.
    pub(super) fn reveal_after_load(
        &mut self,
        needle: Option<String>,
        exact_hit: bool,
        terms: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self
            .slots
            .get(self.active)
            .and_then(|slot| slot.panel.chat().cloned())
            .filter(|panel| !panel.read(cx).is_startup_draft())
        else {
            return;
        };
        panel.update(cx, |panel, cx| panel.set_search_terms(&terms, cx));
        if terms.is_empty() {
            return;
        }
        cx.spawn(async move |_, cx| {
            let deadline = Instant::now() + REVEAL_WINDOW;
            loop {
                let done = panel.update(cx, |panel, cx| {
                    panel.search_terms != terms
                        || (panel.history_loaded()
                            && panel.reveal_transcript_match(needle.as_deref(), exact_hit, cx))
                });
                if done || Instant::now() >= deadline {
                    return;
                }
                cx.background_executor().timer(REVEAL_INTERVAL).await;
            }
        })
        .detach();
    }

    /// Poll the daemon's streaming markers while the picker stays open.
    /// Dropping the picker state drops this task and stops the loop.
    pub(super) fn watch_resume_presence(cx: &mut Context<Self>) -> gpui::Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                let streaming = cx
                    .background_executor()
                    .spawn(async { streaming_sessions() })
                    .await;
                let open = this
                    .update(cx, |this, cx| {
                        let Some(state) = this.resume.as_mut() else {
                            return false;
                        };
                        if state.streaming != streaming {
                            state.streaming = streaming;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !open {
                    return;
                }
                cx.background_executor().timer(PRESENCE_REFRESH).await;
            }
        })
    }

    pub(super) fn open_resume(
        &mut self,
        _: &OpenResume,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = &self.resume {
            window.focus(&state.search.focus_handle(cx), cx);
            return;
        }
        let submit = cx.weak_entity();
        let change = submit.clone();
        let search = cx.new(|cx| {
            PromptInput::new(
                cx,
                "Search sessions and conversations…",
                move |_, _, window, app| {
                    let _ = submit.update(app, |this, cx| this.resume_selected(window, cx));
                },
            )
            .with_on_change(move |query, app| {
                let _ = change.update(app, |this, cx| {
                    if let Some(state) = &mut this.resume {
                        state.query = query.to_owned();
                        state.selected = None;
                        state.selected_hit = None;
                        state.preview_revealed = None;
                        state.scroll.scroll_to_item(0);
                    }
                    this.resume_query_changed(query, cx);
                    cx.notify();
                });
            })
        });
        window.focus(&search.focus_handle(cx), cx);
        self.focus_pending = false;
        self.resume = Some(State {
            search,
            query: String::new(),
            selected: None,
            filter: Filter::All,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            preview_focus: cx.focus_handle(),
            start_on_close: false,
            preview_id: None,
            preview: None,
            preview_task: None,
            streaming: HashSet::new(),
            _presence_task: None,
            selected_hit: None,
            daemon: global_search::Search::new(),
            _debounce: None,
            preview_revealed: None,
        });
        let presence = Self::watch_resume_presence(cx);
        if let Some(state) = self.resume.as_mut() {
            state._presence_task = Some(presence);
        }
        self.bridge.send(Command::RefreshSessions);
        cx.notify();
    }

    /// Cmd+Shift+F and the sidebar magnifier: the session picker, with its
    /// query field ready for a conversation search.
    pub(super) fn open_session_search(
        &mut self,
        _: &GlobalSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_resume(&OpenResume, window, cx);
    }

    /// Open the picker already searching `query`, as if it had been typed.
    pub(crate) fn search_sessions_for(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_resume(&OpenResume, window, cx);
        if let Some(state) = &self.resume {
            state
                .search
                .update(cx, |input, cx| input.set_content(query.to_owned(), cx));
        }
        if let Some(state) = &mut self.resume {
            state.query = query.to_owned();
            state.selected = None;
            state.selected_hit = None;
        }
        self.resume_query_changed(query, cx);
        cx.notify();
    }

    pub(super) fn resume_open(&self) -> bool {
        self.resume.is_some()
    }

    pub(super) fn control_search_state(
        &self,
        window: &Window,
        cx: &App,
    ) -> serde_json::Result<Option<serde_json::Value>> {
        self.resume.as_ref().map(|state| {
            let preview = crate::fixture_control_targets::resume_preview()
                .then(|| state.preview.as_ref()?.as_ref().ok())
                .flatten()
                .map(|panel| panel.read(cx).control_state(window, cx))
                .transpose()?;
            Ok(serde_json::json!({
                "query": state.search.read(cx).snapshot().content,
                "status": state.daemon.status_line(),
                "selected": state.selected,
                "preview_focus": state.preview_focus.contains_focused(window, cx),
                "fixture_preview": preview.map(|state| state["visible_transcript_items"].clone()),
            }))
        }).transpose()
    }

    pub(super) fn close_session_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_resume(window, cx);
    }

    pub(super) fn close_resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start_on_close = self.resume.take().is_some_and(|state| state.start_on_close);
        if start_on_close && (self.connected || self.remotes.default_host.is_some()) {
            self.start_default_startup(cx);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn resume_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.resume_matches(cx);
        let selected = self.resume.as_ref().and_then(|s| s.selected.as_deref());
        // A pasted session ID (or `jcode --resume <id>`) opens directly, even
        // when that session is outside the loaded catalog.
        if let Some(id) = self
            .resume
            .as_ref()
            .and_then(|s| pasted_session_id(&s.query))
        {
            if let Some(session) = matches.iter().find(|s| s.session_id == id).cloned() {
                self.resume = None;
                self.resume_session(session, window, cx);
            } else {
                self.resume = None;
                self.open_requested_session(id, window, cx);
            }
            return;
        }
        let Some(session) = matches
            .iter()
            .find(|s| Some(s.session_id.as_str()) == selected)
            .or(matches.first())
            .cloned()
        else {
            return;
        };
        let row = self
            .resume_selected_row()
            .filter(|row| row.session_id() == session.session_id)
            .unwrap_or_else(|| PickRow::Session(session.session_id.clone()));
        let state = self.resume.take();
        let reveal = state
            .as_ref()
            .and_then(|state| state.daemon.reveal_for(&row));
        let terms = state
            .as_ref()
            .map(|state| global_search::query_terms(&state.query))
            .unwrap_or_default();
        self.resume_session(session, window, cx);
        self.reveal_after_load(reveal, row.hit().is_some(), terms, cx);
    }

    /// Open a session named on the command line. The catalog has not loaded
    /// yet, so the title and directory arrive with the next session list.
    pub(super) fn open_requested_session(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .cloned()
            .unwrap_or(jcode_sdk::SessionInfo {
                user_connected: false,
                edit_stats: None,
                context: None,
                session_id,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                working_dir: None,
                title: None,
                status: "idle".into(),
                transcript_bytes: None,
                saved: false,
                save_label: None,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
            });
        self.resume_session(session, window, cx);
    }

    pub(super) fn resume_session(
        &mut self,
        session: jcode_sdk::SessionInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fixture_history = (crate::fixture_control_targets::resume_preview()
            && session.session_id.starts_with("resume-fixture-"))
        .then(|| load_preview_history(&session.session_id).ok())
        .flatten();
        self.activate_session(session, window, cx);
        if let Some((messages, images)) = fixture_history {
            self.slots[self.active].panel.update_chat(cx, |panel, cx| {
                if !panel.history_loaded() {
                    panel.load_history(messages, images, cx);
                }
            });
        }
        if self.slots[self.active]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::is_startup_draft)
            && (self.connected || self.remotes.default_host.is_some())
        {
            self.start_default_startup(cx);
        }
        if self.single_panel {
            // Slot zero is the standalone chat anchor. Utilities must return to
            // the resumed chat, not resurrect the previous chat as a back page.
            self.slots.swap(0, self.active);
            self.active = 0;
            self.active_row = 0;
            self.show_sidebar = false;
            self.overview = false;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn click_resume_row(
        &mut self,
        row: &PickRow,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_resume_selection(Some(row.clone()));
        if open {
            self.resume_selected(window, cx);
        } else {
            cx.notify();
        }
    }

    /// Page keys and Ctrl+1 to 4. Escape, Enter and the arrows arrive as
    /// `menu` actions.
    pub(super) fn resume_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        match key {
            "pageup" | "pagedown" => {
                let forward = key == "pagedown";
                let scroll = if forward {
                    PreviewScroll::PageDown
                } else {
                    PreviewScroll::PageUp
                };
                if !self.scroll_resume_preview(scroll, window, cx) {
                    self.step_resume_selection(if forward { 8 } else { -8 }, window, cx);
                }
            }
            "1" | "2" | "3" | "4" if event.keystroke.modifiers.control && self.resume.is_some() => {
                self.set_resume_filter(
                    match key {
                        "2" => Filter::Active,
                        "3" => Filter::Saved,
                        "4" => Filter::Crashed,
                        _ => Filter::All,
                    },
                    cx,
                );
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Scroll the session preview when it holds focus. False otherwise.
    pub(super) fn scroll_resume_preview(
        &mut self,
        scroll: PreviewScroll,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self.resume.as_ref() else {
            return false;
        };
        if !state.preview_focus.contains_focused(window, cx) {
            return false;
        }
        if let Some(Ok(panel)) = &state.preview {
            panel.update(cx, |panel, cx| panel.scroll_search_preview(scroll, cx));
        }
        true
    }

    /// Move the list selection by `delta` rows, returning focus to the search field.
    pub(super) fn step_resume_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.resume.as_ref() else {
            return;
        };
        window.focus(&state.search.focus_handle(cx), cx);
        self.move_resume_selection(delta, cx);
    }

    pub(super) fn move_resume_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let matches = self.resume_matches(cx);
        let rows = self.resume_rows(&matches);
        let current = self
            .resume_selected_row()
            .filter(|row| rows.contains(row))
            .or_else(|| rows.first().cloned());
        let next = global_search::step(&rows, current.as_ref(), delta);
        let index = next
            .as_ref()
            .and_then(|next| rows.iter().position(|row| row == next));
        self.set_resume_selection(next);
        if let (Some(state), Some(index)) = (&self.resume, index) {
            state.scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    pub(super) fn set_resume_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.resume {
            state.filter = filter;
            state.selected = None;
            state.selected_hit = None;
            state.scroll.scroll_to_item(0);
        }
        cx.notify();
    }

    pub(super) fn load_resume_preview(&mut self, selected: Option<&str>, cx: &mut Context<Self>) {
        let Some(id) = selected else { return };
        let state = self.resume.as_mut().expect("resume picker is open");
        if state.preview_id.as_deref() == Some(id) {
            return;
        }
        state.preview_id = Some(id.to_owned());
        state.preview = None;
        state.preview_revealed = None;
        let id = id.to_owned();
        let requested_id = id.clone();
        let request = cx
            .background_executor()
            .spawn(async move { load_preview_history(&id) });
        // Replacing this task cancels the previous selection's UI delivery.
        state.preview_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                let current = this
                    .resume
                    .as_ref()
                    .is_some_and(|state| state.preview_id.as_ref() == Some(&requested_id));
                if !current {
                    return;
                }
                let preview = result.map(|(messages, images)| {
                    preview_panel(requested_id.clone(), messages, images, cx)
                });
                if let Some(state) = this.resume.as_mut() {
                    state.preview = Some(preview);
                }
                cx.notify();
            });
        }));
    }
}

pub(super) fn append_resume_fixture(sessions: &mut Vec<jcode_sdk::SessionInfo>) {
    if !harness::screenshot_mode()
        || std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RESUME_FIXTURE").is_none()
    {
        return;
    }
    let now = now_ms();
    for (index, name) in ["alpha", "beta"].into_iter().enumerate() {
        sessions.push(jcode_sdk::SessionInfo {
            session_id: format!("resume-fixture-{name}"),
            title: Some(format!("Resume acceptance {name}")),
            working_dir: Some(format!("/workspace/resume-{name}")),
            saved: index == 0,
            status: if index == 1 { "running" } else { "idle" }.into(),
            transcript_bytes: Some(16_800),
            updated_at_ms: Some(100 - index as i64),
            user_connected: false,
            last_active_at_ms: None,
            archived: false,
            archived_at_ms: None,
            save_label: None,
            parent_session_id: None,
            agent_label: None,
            swarm_status: None,
            edit_stats: None,
            context: Some(jcode_sdk::api::SessionCatalogContext {
                model: Some("gpt-6.2-sol".into()),
                reasoning_effort: Some("high".into()),
                modes: vec!["swarm".into(), "selfdev".into()],
                cache_expires_at_ms: Some(if index == 0 { i64::MAX } else { 1 }),
                activity: Some(jcode_sdk::api::SessionCatalogActivity {
                    origin: if index == 0 {
                        jcode_sdk::api::SessionOrigin::User
                    } else {
                        jcode_sdk::api::SessionOrigin::Agent
                    },
                    last_agent_activity_at_ms: Some(
                        now - if index == 0 { 305_000 } else { 605_000 },
                    ),
                    last_message_at_ms: Some(Some(now - if index == 0 { 305_000 } else { 65_000 })),
                }),
            }),
        });
    }
}

pub(super) fn convert_results(results: jcode_sdk::SessionSearchResults) -> global_search::Results {
    global_search::Results {
        query: results.query,
        titles: results.titles.into_iter().map(convert_hit).collect(),
        messages: results.messages.into_iter().map(convert_hit).collect(),
        indexed_sessions: results.indexed_sessions,
        truncated: results.truncated,
    }
}

pub(super) fn convert_hit(hit: jcode_sdk::SessionSearchHit) -> global_search::SearchHit {
    use global_search::{Highlight, Role};
    // One visual line per hit, with the highlight offsets recomputed for the
    // windowed text the row actually draws.
    let highlights: Vec<Highlight> = hit
        .highlights
        .into_iter()
        .map(|highlight| Highlight {
            start: highlight.start,
            end: highlight.end,
        })
        .collect();
    let (snippet, highlights) = global_search::window_snippet(&hit.snippet, &highlights);
    global_search::SearchHit {
        session_id: hit.session_id,
        title: hit.title,
        role: match hit.role {
            jcode_sdk::SessionSearchRole::Title => Role::Title,
            jcode_sdk::SessionSearchRole::User => Role::User,
            jcode_sdk::SessionSearchRole::Assistant => Role::Assistant,
        },
        model: hit.model,
        snippet,
        highlights,
        timestamp_ms: hit.timestamp_ms,
        message_id: hit.message_id,
    }
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

pub(super) fn resume_activity_at_ms(session: &jcode_sdk::SessionInfo) -> Option<u64> {
    let timestamp = if session.has_message_activity() {
        session.last_message_at_ms()
    } else {
        session.last_agent_activity_at_ms()
    }?;
    (timestamp > 0).then_some(timestamp as u64)
}

pub(super) fn resume_usage(
    session: &jcode_sdk::SessionInfo,
    sort: sidebar_session_catalog::Sort,
    now: i64,
) -> Option<String> {
    match sort {
        sidebar_session_catalog::Sort::Recent => sidebar_session_meta(session),
        sidebar_session_catalog::Sort::AgentActivity => session_meta(
            session,
            resume_activity_at_ms(session).map(|timestamp| format_time_ago(timestamp, now as u64)),
        ),
    }
}

pub(super) fn resume_row_metadata(
    session: &jcode_sdk::SessionInfo,
    sort: sidebar_session_catalog::Sort,
    now: i64,
) -> (String, String) {
    let mut runtime = Vec::new();
    let mut usage = Vec::new();
    if let Some(context) = &session.context {
        runtime.extend(context.model.iter().cloned());
        runtime.extend(context.reasoning_effort.iter().cloned());
        if !context.modes.is_empty() {
            runtime.push(context.modes.join(", "));
        }
        if let Some(expiry) = context.cache_expires_at_ms {
            usage.push(if now < expiry { "warm" } else { "cold" }.into());
        }
    }
    usage.extend(resume_usage(session, sort, now));
    (runtime.join(" · "), usage.join(" · "))
}

pub(super) type PreviewHistory = (
    Vec<jcode_sdk::HistoryMessage>,
    Vec<jcode_sdk::RenderedImage>,
);

/// Build the ordinary chat panel over an inert bridge. It never sends commands
/// or connects its composer, and renders only the transcript.
pub(super) fn preview_panel(
    id: String,
    messages: Vec<jcode_sdk::HistoryMessage>,
    images: Vec<jcode_sdk::RenderedImage>,
    cx: &mut App,
) -> Entity<Panel> {
    cx.new(|cx| {
        let mut panel = Panel::new(
            format!("resume-preview://{id}"),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.show_transcript_only(cx);
        panel.load_history(messages, images, cx);
        panel
    })
}

/// Read only: never attach/watch a runtime just to browse its conversation.
pub(super) fn load_preview_history(id: &str) -> Result<PreviewHistory, String> {
    if harness::screenshot_mode()
        && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RESUME_FIXTURE").is_some()
        && id.starts_with("resume-fixture-")
    {
        let message = |role: &str, content: String| jcode_sdk::HistoryMessage {
            attachments: Vec::new(),
            response_stats: None,
            timestamp_ms: None,
            tool: None,
            role: role.into(),
            content,
        };
        let name = if id.ends_with("alpha") {
            "Alpha"
        } else {
            "Beta"
        };
        if crate::fixture_control_targets::resume_preview() {
            let messages = (0..80)
                .map(|index| {
                    let role = if index % 2 == 0 { "user" } else { "assistant" };
                    let text = if index == 31 {
                        format!("{name} search anchor in the middle of the conversation")
                    } else {
                        format!("History entry {index:02}")
                    };
                    message(role, text)
                })
                .collect();
            return Ok((messages, Vec::new()));
        }
        return Ok((
            vec![
                message("user", "Please recover the standalone conversation.".into()),
                message(
                    "assistant",
                    format!(
                        "**{name}** conversation preview\n\n- rendered with the chat panel\n- `markdown` included"
                    ),
                ),
            ],
            Vec::new(),
        ));
    }
    // Reject path components and remote IDs rather than reading outside local storage.
    if id.is_empty() || id.contains(['/', '\\']) || id == "." || id == ".." {
        return Err(
            "Preview unavailable for this session. Resume to load its conversation.".into(),
        );
    }
    crate::persisted_history::load(id)
}

#[cfg(test)]
#[path = "workspace_resume_tests.rs"]
pub(super) mod tests;
