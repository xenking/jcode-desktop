//! A full-height global search screen over every stored session.
//!
//! Unlike the resume picker, this is not a browser: it answers one question
//! ("where did I write/read X?") and gets out of the way. Enter opens the
//! session at the matching message, Esc returns to the workspace.
//!
//! The pure state machine lives in `jcode_desktop_model::global_search`; this
//! file is the GPUI shell: a query field, a debounce timer, the harness round
//! trip, and the drawing.

use super::*;
use crate::global_search::{
    self, Effect, Highlight, OpenTarget, Results, Role, Row, Screen, SearchHit, Status,
};
use std::time::{Duration, Instant};

/// How long the opened session's transcript is polled for the message to
/// reveal. History arrives asynchronously; this covers a slow daemon without
/// leaving a timer running forever.
const REVEAL_WINDOW: Duration = Duration::from_secs(6);
const REVEAL_INTERVAL: Duration = Duration::from_millis(60);

pub(super) struct State {
    pub(super) input: Entity<PromptInput>,
    screen: Screen,
    scroll: ScrollHandle,
    /// Set while a `set_query` result should be sent once typing settles.
    _debounce: Option<gpui::Task<()>>,
    /// The last latency the daemon reported, shown next to the counts.
    elapsed_ms: Option<f64>,
}

impl Workspace {
    pub(super) fn open_search_screen(
        &mut self,
        _: &GlobalSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_screen.is_some() {
            if let Some(state) = &self.search_screen {
                window.focus(&state.input.focus_handle(cx), cx);
            }
            return;
        }
        let submit = cx.weak_entity();
        let change = submit.clone();
        let input = cx.new(|cx| {
            PromptInput::new(
                cx,
                "Search all sessions…",
                move |_, _, window, app| {
                    let _ = submit.update(app, |this, cx| this.search_open_selected(window, cx));
                },
            )
            .with_on_change(move |query, app| {
                let _ = change.update(app, |this, cx| this.search_query_changed(query, cx));
            })
        });
        window.focus(&input.focus_handle(cx), cx);
        self.search_screen = Some(State {
            input,
            screen: Screen::new(),
            scroll: ScrollHandle::new(),
            _debounce: None,
            elapsed_ms: None,
        });
        cx.notify();
    }

    pub(super) fn close_search_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_screen = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Gate for the keybinding: the screen owns Esc/Enter/arrows while open.
    pub(super) fn search_screen_open(&self) -> bool {
        self.search_screen.is_some()
    }

    fn search_query_changed(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(state) = self.search_screen.as_mut() else {
            return;
        };
        let effect = state.screen.set_query(query);
        let effect_generation = match effect {
            Effect::Cleared => {
                state._debounce = None;
                cx.notify();
                return;
            }
            Effect::Debounce(generation) => generation,
        };
        // Each keystroke replaces the pending timer, so only the last one sends.
        state._debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(global_search::DEBOUNCE)
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.search_screen.as_mut() else {
                    return;
                };
                if !state.screen.mark_sent(effect_generation) {
                    return;
                }
                let (query, generation) = state.screen.pending_query();
                let query = query.to_string();
                cx.notify();
                this.bridge.send(Command::SearchSessions { query, generation });
            });
        }));
        cx.notify();
    }

    pub(super) fn apply_session_search(
        &mut self,
        _query: String,
        generation: u64,
        result: Result<jcode_sdk::SessionSearchResults, String>,
        elapsed_ms: f64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.search_screen.as_mut() else {
            return;
        };
        // A reply for a query the user has already replaced is dropped inside
        // the screen, but a reply that is stale because the field was cleared
        // must not resurrect an old list either.
        match result {
            Ok(results) => {
                if state.screen.apply_results(generation, convert_results(results)) {
                    state.elapsed_ms = Some(elapsed_ms);
                    state.scroll.scroll_to_item(0);
                }
            }
            Err(error) => {
                state.screen.apply_error(generation, error);
            }
        }
        cx.notify();
    }

    pub(super) fn search_move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(state) = self.search_screen.as_mut() {
            state.screen.move_selection(delta);
            if let Some(selected) = state.screen.selected() {
                state.scroll.scroll_to_item(selected);
            }
            cx.notify();
        }
    }

    fn search_select_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(state) = self.search_screen.as_mut() {
            state.screen.select_row(index);
            cx.notify();
        }
    }

    /// Enter, or a click on a row.
    pub(super) fn search_open_selected(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .search_screen
            .as_ref()
            .and_then(|state| state.screen.open_target())
        else {
            return;
        };
        self.search_screen = None;
        self.open_search_target(target, window, cx);
        cx.notify();
    }

    fn open_search_target(
        &mut self,
        target: OpenTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // An already-open panel is brought forward rather than duplicated: the
        // user is looking for the conversation, not for a second copy of it.
        let existing = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.read(cx).session_id == target.session_id);
        match existing {
            Some(index) => {
                self.active = index;
                self.active_row = self.slots[index].row;
            }
            None => {
                let session = self
                    .sessions
                    .iter()
                    .find(|session| session.session_id == target.session_id)
                    .cloned();
                match session {
                    Some(session) => self.activate_session(session, window, cx),
                    None => {
                        self.open_requested_session(target.session_id.clone(), window, cx);
                    }
                }
            }
        }
        self.overview = false;
        self.focus_active(window, cx);
        if let Some(needle) = target.reveal {
            self.reveal_after_load(needle, cx);
        }
    }

    /// Poll the opened transcript until the searched message appears.
    ///
    /// Opened sessions load their history asynchronously, so the first frames
    /// have an empty transcript and nothing to scroll to.
    fn reveal_after_load(&mut self, needle: String, cx: &mut Context<Self>) {
        let Some(panel) = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.clone())
            .filter(|panel| !panel.read(cx).is_startup_draft())
        else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let deadline = Instant::now() + REVEAL_WINDOW;
            loop {
                let revealed = panel
                    .update(cx, |panel, cx| panel.reveal_transcript_text(&needle, cx));
                if revealed || Instant::now() >= deadline {
                    return;
                }
                cx.background_executor().timer(REVEAL_INTERVAL).await;
            }
        })
        .detach();
    }

    pub(super) fn search_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "escape" => self.close_search_screen(window, cx),
            "enter" => self.search_open_selected(window, cx),
            "up" => self.search_move_selection(-1, cx),
            "down" => self.search_move_selection(1, cx),
            "pageup" => self.search_move_selection(-8, cx),
            "pagedown" => self.search_move_selection(8, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_search_screen(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::global();
        let Some(state) = self.search_screen.as_ref() else {
            return div();
        };
        let input = state.input.clone();
        let scroll = state.scroll.clone();
        let status_line = state.screen.status_line();
        let status = state.screen.status();
        let elapsed_ms = state.elapsed_ms;
        let selected = state.screen.selected();
        let results = state.screen.results().cloned();
        let rows = state.screen.rows().to_vec();

        let mut list = div()
            .id("global-search-results")
            .debug_selector(|| "global-search-results".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll);
        for (index, row) in rows.iter().enumerate() {
            match row {
                Row::Section(label) => {
                    list = list.child(
                        div()
                            .debug_selector(move || format!("global-search-section-{index}"))
                            .px_3()
                            .pt_4()
                            .pb_1()
                            .text_size(px(11.0))
                            .text_color(theme.TEXT_FAINT)
                            .child(label.to_uppercase()),
                    );
                }
                Row::Group { title, .. } => {
                    let label = if title.trim().is_empty() {
                        "Untitled session".to_string()
                    } else {
                        title.clone()
                    };
                    list = list.child(
                        div()
                            .debug_selector(move || format!("global-search-group-{index}"))
                            .px_3()
                            .pt_3()
                            .pb_1()
                            .text_size(px(12.0))
                            .text_color(theme.TEXT_DIM)
                            .text_ellipsis()
                            .child(label),
                    );
                }
                Row::Hit { hit, role } => {
                    let Some(data) = results
                        .as_ref()
                        .and_then(|results| match role {
                            Role::Title => results.titles.get(*hit),
                            Role::User | Role::Assistant => results.messages.get(*hit),
                        })
                        .cloned()
                    else {
                        continue;
                    };
                    let is_selected = selected == Some(index);
                    let click_index = index;
                    list = list.child(
                        div()
                            .id(("global-search-row", index))
                            .debug_selector(move || format!("global-search-row-{index}"))
                            .px_3()
                            .py_2()
                            .mx_2()
                            .cursor_pointer()
                            .rounded_md()
                            .when(is_selected, |el| el.bg(theme.TOOL_BG))
                            .hover(|el| el.bg(theme.TOOL_BG))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.search_select_row(click_index, cx);
                                this.search_open_selected(window, cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(role_badge(*role, theme))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(12.0))
                                            .text_ellipsis()
                                            .child(snippet_element(&data, theme)),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(theme.TEXT_FAINT)
                                            .child(hit_time(data.timestamp_ms)),
                                    ),
                            ),
                    );
                }
            }
        }
        if rows.is_empty() {
            let notice = match status {
                Status::Idle => "Type to search every session".to_string(),
                Status::Waiting | Status::Searching => "Searching…".to_string(),
                Status::Failed => status_line.clone(),
                Status::Ready => status_line.clone(),
            };
            list = list.child(
                div()
                    .p_4()
                    .text_size(px(12.0))
                    .text_color(theme.TEXT_DIM)
                    .child(notice),
            );
        }
        let counts = if let Some(elapsed_ms) = elapsed_ms.filter(|_| status == Status::Ready) {
            format!("{status_line} · {elapsed_ms:.0}ms")
        } else {
            status_line
        };
        div()
            .debug_selector(|| "global-search-screen".into())
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.PANEL_BG)
            .text_color(theme.TEXT)
            .font_family(theme.FONT_UI)
            .text_size(px(14.0))
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(Self::search_key_down))
            .capture_action(cx.listener(|this, _: &crate::input::Clear, window, cx| {
                this.close_search_screen(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::Submit, window, cx| {
                this.search_open_selected(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::HistoryPrev, _, cx| {
                this.search_move_selection(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::HistoryNext, _, cx| {
                this.search_move_selection(1, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_size(px(22.0)).child("Search sessions"))
                    .child(
                        div()
                            .id("global-search-close")
                            .debug_selector(|| "global-search-close".into())
                            .cursor_pointer()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .hover(|el| el.bg(theme.TOOL_BG))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_search_screen(window, cx)
                            }))
                            .child("Back · Esc"),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "global-search-input".into())
                    .child(input),
            )
            .child(
                div()
                    .px_1()
                    .text_size(px(11.0))
                    .text_color(theme.TEXT_FAINT)
                    .child(counts),
            )
            .child(list)
    }
}

fn convert_results(results: jcode_sdk::SessionSearchResults) -> Results {
    Results {
        query: results.query,
        titles: results.titles.into_iter().map(convert_hit).collect(),
        messages: results.messages.into_iter().map(convert_hit).collect(),
        indexed_sessions: results.indexed_sessions,
        truncated: results.truncated,
    }
}

fn convert_hit(hit: jcode_sdk::SessionSearchHit) -> SearchHit {
    SearchHit {
        session_id: hit.session_id,
        title: hit.title,
        role: match hit.role {
            jcode_sdk::SessionSearchRole::Title => Role::Title,
            jcode_sdk::SessionSearchRole::User => Role::User,
            jcode_sdk::SessionSearchRole::Assistant => Role::Assistant,
        },
        snippet: hit.snippet,
        highlights: hit
            .highlights
            .into_iter()
            .map(|highlight| Highlight {
                start: highlight.start,
                end: highlight.end,
            })
            .collect(),
        timestamp_ms: hit.timestamp_ms,
        message_id: hit.message_id,
    }
}

fn role_badge(role: Role, theme: &crate::theme::Theme) -> gpui::AnyElement {
    let color = match role {
        Role::Title => theme.TEXT_DIM,
        Role::User => theme.TEXT,
        Role::Assistant => theme.ACCENT,
    };
    div()
        .flex_none()
        .px_2()
        .py_0p5()
        .rounded_sm()
        .bg(theme.TOOL_BG)
        .text_size(px(10.0))
        .text_color(color)
        .child(role.label())
        .into_any_element()
}

/// The snippet with its matched ranges emphasised.
fn snippet_element(hit: &SearchHit, theme: &crate::theme::Theme) -> gpui::AnyElement {
    let mut spans: Vec<(String, bool)> = Vec::new();
    let mut cursor = 0usize;
    for highlight in &hit.highlights {
        let (start, end) = (highlight.start as usize, highlight.end as usize);
        if start < cursor || start >= end {
            continue;
        }
        let before: String = hit.snippet.chars().skip(cursor).take(start - cursor).collect();
        if !before.is_empty() {
            spans.push((before, false));
        }
        let marked: String = hit
            .snippet
            .chars()
            .skip(start)
            .take(end - start)
            .collect();
        spans.push((marked, true));
        cursor = end;
    }
    let rest: String = hit.snippet.chars().skip(cursor).collect();
    if !rest.is_empty() {
        spans.push((rest, false));
    }
    if spans.is_empty() {
        return div().child(hit.snippet.clone()).into_any_element();
    }
    let mut element = div().flex().items_center();
    for (text, marked) in spans {
        element = element.child(div().when(marked, |el| el.text_color(theme.ACCENT)).child(text));
    }
    element.into_any_element()
}

/// When a hit happened, relative to now: a search result is recalled as "a
/// while ago", not as an absolute date.
fn hit_time(timestamp_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return String::new();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    let ago = now_ms.saturating_sub(timestamp_ms) / 1_000;
    match ago {
        ..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", ago / 60),
        3_600..=86_399 => format!("{}h", ago / 3_600),
        86_400..=2_592_000 => format!("{}d", ago / 86_400),
        _ => format!("{}mo", ago / 2_592_000),
    }
}
