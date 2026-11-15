//! Global search across every stored session: screen state and hit layout.
//!
//! The screen is a full-height view, not a popover, so its state is more than a
//! string and a list. It owns the query, the debounce generation, the results,
//! the flattened row list the keyboard walks, and the selection. Keeping that
//! here (GPUI-free) means the whole interaction can be tested without a window:
//! typing, debouncing, out-of-order replies, section order, grouping and what
//! Enter opens are all plain functions over this type.
//!
//! Layout rules the rows encode:
//! - `Titles` section first: a title match is the session itself, not a line
//!   inside it, and burying it under transcript hits makes recall worse.
//! - `Messages` section second, grouped by session, sessions ordered by their
//!   best-ranked hit. The group header is not selectable, so `Down` walks hits
//!   and never lands on a heading.

use std::time::Duration;

/// How long typing must settle before a query goes to the daemon.
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// Cap on the part of a snippet used to find the message again after opening
/// the session. Long enough to be unique in a conversation, short enough that
/// whitespace differences in the snippet cannot defeat the match.
const NEEDLE_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Title,
    User,
    Assistant,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::User => "you",
            Self::Assistant => "jcode",
        }
    }
}

/// A highlighted character range inside a hit's snippet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: u32,
    pub end: u32,
}

/// One result row's data.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    pub role: Role,
    pub snippet: String,
    pub highlights: Vec<Highlight>,
    pub timestamp_ms: i64,
    pub message_id: Option<String>,
}

/// A daemon reply, in the screen's terms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    pub query: String,
    pub titles: Vec<SearchHit>,
    pub messages: Vec<SearchHit>,
    pub indexed_sessions: u64,
    pub truncated: bool,
}

impl Results {
    pub fn is_empty(&self) -> bool {
        self.titles.is_empty() && self.messages.is_empty()
    }
}

/// What the screen is currently doing, for the status line and empty states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No query typed yet.
    Idle,
    /// Typed, waiting for the debounce to expire.
    Waiting,
    /// Sent, reply outstanding.
    Searching,
    /// Reply applied.
    Ready,
    /// The daemon could not answer.
    Failed,
}

/// A row in the rendered list. Headers are separators, not targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Section(&'static str),
    Group {
        session_id: String,
        title: String,
    },
    Hit {
        /// Index into the section's hit vector.
        hit: usize,
        /// Where the hit lives, so a row can be resolved to its data.
        role: Role,
    },
}

impl Row {
    pub fn is_selectable(&self) -> bool {
        matches!(self, Self::Hit { .. })
    }
}

/// What Enter on the current row should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTarget {
    pub session_id: String,
    /// Text to reveal in the opened transcript. `None` for a title hit, which
    /// opens at the session's normal position.
    pub reveal: Option<String>,
}

/// The screen's whole state.
#[derive(Debug, Default)]
pub struct Screen {
    query: String,
    /// Identifies the newest query; replies carry the value they answered.
    generation: u64,
    /// Generation whose reply is outstanding, if any.
    awaiting: Option<u64>,
    status: Status,
    results: Option<Results>,
    rows: Vec<Row>,
    /// Index into `rows` of the selected row.
    selected: Option<usize>,
    error: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self::Idle
    }
}

/// What a query change obliges the caller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to send: the query is blank, so the results are cleared.
    Cleared,
    /// Send `Screen::pending_query()` after `Screen::debounce()`.
    Debounce(u64),
}

impl Screen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn results(&self) -> Option<&Results> {
        self.results.as_ref()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The query a pending debounce should send, and the generation it must
    /// carry so a late reply can be recognised as stale.
    pub fn pending_query(&self) -> (&str, u64) {
        (&self.query, self.generation)
    }

    pub fn debounce(&self) -> Duration {
        DEBOUNCE
    }

    /// Apply a keystroke's worth of query text.
    pub fn set_query(&mut self, query: &str) -> Effect {
        if query == self.query {
            return Effect::Cleared;
        }
        self.query = query.to_string();
        self.generation += 1;
        self.error = None;
        if query.trim().is_empty() {
            self.awaiting = None;
            self.status = Status::Idle;
            self.results = None;
            self.rows.clear();
            self.selected = None;
            return Effect::Cleared;
        }
        self.status = Status::Waiting;
        // Keep the previous results on screen while the new query is debounced:
        // a list that blanks on every keystroke flickers, and the user is still
        // reading the old rows.
        Effect::Debounce(self.generation)
    }

    /// Mark the query as sent. Returns false if a newer query has been typed,
    /// in which case the caller must not send this one.
    pub fn mark_sent(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.awaiting = Some(generation);
        self.status = Status::Searching;
        true
    }

    /// Apply a daemon reply. Stale replies are dropped, which is the whole
    /// point of the generation: threads finish out of order.
    pub fn apply_results(&mut self, generation: u64, results: Results) -> bool {
        if generation != self.generation {
            return false;
        }
        self.awaiting = None;
        self.status = Status::Ready;
        self.error = None;
        self.rows = layout(&results);
        self.results = Some(results);
        self.selected = self
            .rows
            .iter()
            .position(Row::is_selectable);
        true
    }

    pub fn apply_error(&mut self, generation: u64, error: String) -> bool {
        if generation != self.generation {
            return false;
        }
        self.awaiting = None;
        self.status = Status::Failed;
        self.error = Some(error);
        self.rows.clear();
        self.selected = None;
        true
    }

    /// Move the selection by `delta` selectable rows, clamped at both ends.
    pub fn move_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.is_selectable())
            .map(|(index, _)| index)
            .collect();
        if selectable.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|selected| selectable.iter().position(|row| *row == selected));
        let next = match (current, delta.cmp(&0)) {
            (Some(current), std::cmp::Ordering::Less) => {
                current.saturating_sub(delta.unsigned_abs())
            }
            (Some(current), std::cmp::Ordering::Greater) => {
                (current + delta as usize).min(selectable.len() - 1)
            }
            (None, std::cmp::Ordering::Less) => selectable.len() - 1,
            (None, _) => 0,
            (Some(current), std::cmp::Ordering::Equal) => current,
        };
        self.selected = Some(selectable[next]);
    }

    /// Select a row directly (a click).
    pub fn select_row(&mut self, index: usize) {
        if self.rows.get(index).is_some_and(Row::is_selectable) {
            self.selected = Some(index);
        }
    }

    /// Resolve the selected row to its hit data.
    pub fn selected_hit(&self) -> Option<&SearchHit> {
        let results = self.results.as_ref()?;
        let row = self.rows.get(self.selected?)?;
        let Row::Hit { hit, role } = row else {
            return None;
        };
        match role {
            Role::Title => results.titles.get(*hit),
            Role::User | Role::Assistant => results.messages.get(*hit),
        }
    }

    /// What Enter or a click on the selected row opens.
    pub fn open_target(&self) -> Option<OpenTarget> {
        let hit = self.selected_hit()?;
        Some(OpenTarget {
            session_id: hit.session_id.clone(),
            reveal: (hit.role != Role::Title).then(|| reveal_needle(hit)),
        })
    }

    /// The status line for the current state.
    pub fn status_line(&self) -> String {
        match self.status {
            Status::Idle => "Search every session's titles and messages".into(),
            Status::Waiting => "…".into(),
            Status::Searching => "Searching…".into(),
            Status::Failed => self
                .error
                .clone()
                .unwrap_or_else(|| "Search failed".to_string()),
            Status::Ready => {
                let Some(results) = &self.results else {
                    return String::new();
                };
                if results.is_empty() {
                    return format!("No matches in {} sessions", results.indexed_sessions);
                }
                let more = if results.truncated { "+" } else { "" };
                format!(
                    "{}{more} title{} · {}{more} message{}",
                    results.titles.len(),
                    plural(results.titles.len()),
                    results.messages.len(),
                    plural(results.messages.len())
                )
            }
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Build the rendered rows: titles, then message hits grouped by session.
fn layout(results: &Results) -> Vec<Row> {
    let mut rows = Vec::new();
    if !results.titles.is_empty() {
        rows.push(Row::Section("Titles"));
        for index in 0..results.titles.len() {
            rows.push(Row::Hit {
                hit: index,
                role: Role::Title,
            });
        }
    }
    if !results.messages.is_empty() {
        rows.push(Row::Section("Messages"));
        let mut order: Vec<&str> = Vec::new();
        for hit in &results.messages {
            if !order.contains(&hit.session_id.as_str()) {
                order.push(&hit.session_id);
            }
        }
        for session_id in order {
            let title = results
                .messages
                .iter()
                .find(|hit| hit.session_id == session_id)
                .map(|hit| hit.title.clone())
                .unwrap_or_default();
            rows.push(Row::Group {
                session_id: session_id.to_string(),
                title,
            });
            for (index, hit) in results.messages.iter().enumerate() {
                if hit.session_id == session_id {
                    rows.push(Row::Hit {
                        hit: index,
                        role: hit.role,
                    });
                }
            }
        }
    }
    rows
}

/// The longest fragment of a snippet that is verbatim message text.
///
/// FTS5 marks dropped text with `…`, so a fragment between two ellipses is
/// contiguous and can be searched for in the opened transcript. The fragment
/// holding the first highlight is the best one: it contains the match.
pub(crate) fn reveal_needle(hit: &SearchHit) -> String {
    let segments: Vec<&str> = hit
        .snippet
        .split('…')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return hit.snippet.trim().to_string();
    }
    // Character offset of the first highlight, translated to a segment index.
    let first_highlight = hit.highlights.first().map(|highlight| highlight.start).unwrap_or(0);
    let mut consumed = 0u32;
    let mut chosen = segments.len() - 1;
    for (index, segment) in segments.iter().enumerate() {
        let length = segment.chars().count() as u32;
        if consumed + length > first_highlight {
            chosen = index;
            break;
        }
        consumed += length + 1; // the ellipsis separator
    }
    segments[chosen]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NEEDLE_CHARS)
        .collect()
}
