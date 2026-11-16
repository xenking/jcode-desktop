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
//!   best-ranked hit. A group starts collapsed and its header toggles it open,
//!   so one busy session cannot bury the others.
//! - every hit is exactly one visual line: the snippet is whitespace-collapsed
//!   and windowed around the first match, and the window is recomputed with the
//!   highlight offsets so the emphasis stays on the words it belongs to.

use std::time::Duration;

/// How long typing must settle before a query goes to the daemon.
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// Characters of leading context kept before the first match. Enough to read
/// the sentence, small enough that the match is still the eye's landing point.
pub const SNIPPET_LEAD: usize = 40;
/// Characters kept after the window start. One screen row at every text size
/// the app ships, so a snippet never wraps into a second line.
pub const SNIPPET_CHARS: usize = 160;

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
            Self::User => "You",
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
    /// The model that wrote an assistant hit, so its badge names the model
    /// instead of a generic "jcode".
    pub model: Option<String>,
    pub snippet: String,
    pub highlights: Vec<Highlight>,
    pub timestamp_ms: i64,
    pub message_id: Option<String>,
}

impl SearchHit {
    /// The badge text for this hit: "You" for a prompt, the model for an
    /// answer.
    pub fn badge(&self) -> String {
        match self.role {
            Role::Title => self.role.label().to_string(),
            Role::User => self.role.label().to_string(),
            Role::Assistant => match self.model.as_deref().map(model_label) {
                Some(label) if !label.is_empty() => label,
                _ => self.role.label().to_string(),
            },
        }
    }
}

/// A model id as a short badge: `claude-opus-5-5` is "opus 5.5",
/// `deepseek-v4.1-flash` is "ds-4.1f". Unknown ids pass through unchanged
/// rather than being mangled into something unrecognisable.
pub fn model_label(model: &str) -> String {
    let model = model.trim().to_ascii_lowercase();
    if let Some(rest) = model.strip_prefix("claude-") {
        let mut parts = rest.split('-');
        let family = parts.next().unwrap_or_default();
        // Date suffixes (`-20260101`) are not part of how anyone names the
        // model in conversation.
        let versions: Vec<&str> = parts
            .filter(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            .take(2)
            .collect();
        if !family.is_empty() && !versions.is_empty() {
            return format!("{family} {}", versions.join("."));
        }
    }
    if let Some(rest) = model.strip_prefix("deepseek-") {
        let rest = rest.strip_prefix('v').unwrap_or(rest);
        let rest = rest.replace("-flash", "f").replace("-thinking", "");
        return format!("ds-{rest}");
    }
    model
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    /// No query typed yet.
    #[default]
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
        /// How many hits this session contributed, so a collapsed row still
        /// says how much is behind it.
        count: usize,
        expanded: bool,
    },
    Hit {
        /// Index into the section's hit vector.
        hit: usize,
        /// Where the hit lives, so a row can be resolved to its data.
        role: Role,
    },
}

impl Row {
    /// Groups and hits are both reachable: Enter on a group opens it, Enter on
    /// a hit opens the session.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, Self::Section(_))
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
    /// Sessions whose group the user has opened. Collapsed is the default: a
    /// query that matches ten messages in one session should not push every
    /// other session off the screen.
    expanded: std::collections::HashSet<String>,
    /// Index into `rows` of the selected row.
    selected: Option<usize>,
    error: Option<String>,
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
        self.expanded.clear();
        self.rows = layout(&results, &self.expanded);
        self.results = Some(results);
        self.selected = self.rows.iter().position(Row::is_selectable);
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

    /// Whether the selected row is a session header (Enter then expands it
    /// instead of opening a session).
    pub fn selected_group(&self) -> Option<String> {
        let row = self.rows.get(self.selected?)?;
        match row {
            Row::Group { session_id, .. } => Some(session_id.clone()),
            _ => None,
        }
    }

    /// Open or close one session's group. Returns false when `index` is not a
    /// group.
    pub fn toggle_group(&mut self, index: usize) -> bool {
        let Some(Row::Group { session_id, .. }) = self.rows.get(index).cloned() else {
            return false;
        };
        if !self.expanded.remove(&session_id) {
            self.expanded.insert(session_id.clone());
        }
        let Some(results) = self.results.clone() else {
            return true;
        };
        self.rows = layout(&results, &self.expanded);
        // The header survives the re-layout, so keep the caret on it rather
        // than on whatever row happens to occupy the old index.
        self.selected = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Group { session_id: id, .. } if *id == session_id));
        true
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
fn layout(results: &Results, expanded: &std::collections::HashSet<String>) -> Vec<Row> {
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
            let count = results
                .messages
                .iter()
                .filter(|hit| hit.session_id == session_id)
                .count();
            let open = expanded.contains(session_id);
            rows.push(Row::Group {
                session_id: session_id.to_string(),
                title,
                count,
                expanded: open,
            });
            if open {
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
    }
    rows
}

/// Turn an FTS snippet into one screen line: newlines and runs of whitespace
/// become single spaces, and the text is windowed around the first match.
///
/// The daemon's snippet is built from the raw message, so it carries the
/// user's line breaks. Rendered as-is, one hit spreads over three to six rows
/// and the highlighted word lands on a different line from the text that
/// introduces it. Highlight ranges are character offsets into the *same*
/// string, so they are remapped here rather than left to drift.
pub fn window_snippet(snippet: &str, highlights: &[Highlight]) -> (String, Vec<Highlight>) {
    let original: Vec<char> = snippet.chars().collect();
    let mut collapsed = String::with_capacity(snippet.len());
    // Collapsed offset of each original character. Whitespace occupies no
    // position of its own: the single space that replaces a run is emitted
    // before the next visible character.
    let mut offsets: Vec<Option<u32>> = Vec::with_capacity(original.len());
    let mut pending_space = false;
    let mut width = 0u32;
    for character in &original {
        if character.is_whitespace() {
            pending_space = !collapsed.is_empty();
            offsets.push(None);
            continue;
        }
        if pending_space {
            collapsed.push(' ');
            width += 1;
            pending_space = false;
        }
        offsets.push(Some(width));
        collapsed.push(*character);
        width += 1;
    }
    let collapsed_chars: Vec<char> = collapsed.chars().collect();
    let total = collapsed_chars.len();
    let first = highlights
        .iter()
        .filter_map(|highlight| {
            let start = *offsets.get(highlight.start as usize)?;
            start.map(|start| (start, highlight))
        })
        .min_by_key(|(start, _)| *start);

    // Window: about SNIPPET_LEAD characters of context before the match, then
    // SNIPPET_CHARS from there, snapped to word boundaries so neither end cuts
    // a word in half.
    let mut start = first
        .map(|(start, _)| start as usize)
        .unwrap_or_default()
        .saturating_sub(SNIPPET_LEAD);
    if start > 0 {
        match collapsed_chars[start..].iter().position(|c| *c == ' ') {
            Some(offset) if offset < SNIPPET_LEAD => start += offset + 1,
            _ => {}
        }
    }
    let mut end = (start + SNIPPET_CHARS).min(total);
    if end < total
        && let Some(offset) = collapsed_chars[start..end].iter().rposition(|c| *c == ' ')
        && offset > SNIPPET_CHARS / 2
    {
        end = start + offset;
    }
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < total { "…" } else { "" };
    let mut text = String::with_capacity(end - start + 2);
    text.push_str(prefix);
    text.extend(collapsed_chars[start..end].iter());
    text.push_str(suffix);

    let shift = prefix.chars().count() as u32;
    let mut remapped = Vec::new();
    for highlight in highlights {
        let Some(Some(old_start)) = offsets.get(highlight.start as usize) else {
            continue;
        };
        let last = (highlight.end as usize).saturating_sub(1);
        let Some(Some(old_end)) = offsets.get(last) else {
            continue;
        };
        let (old_start, old_end) = (*old_start, old_end + 1);
        let (new_start, new_end) = (old_start.max(start as u32), old_end.min(end as u32));
        if new_start >= new_end {
            continue;
        }
        remapped.push(Highlight {
            start: new_start - start as u32 + shift,
            end: new_end - start as u32 + shift,
        });
    }
    (text, remapped)
}

/// Absolute and relative time for one hit: "15 Nov 21:40 · 3h".
///
/// A search result is recalled as "a while ago" but verified as a date, so the
/// row carries both instead of forcing the reader to convert.
pub fn time_label(timestamp_ms: i64, now_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return String::new();
    }
    let ago_seconds = now_ms.saturating_sub(timestamp_ms) / 1_000;
    let relative = match ago_seconds {
        ..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", ago_seconds / 60),
        3_600..=86_399 => format!("{}h", ago_seconds / 3_600),
        86_400..=2_592_000 => format!("{}d", ago_seconds / 86_400),
        _ => format!("{}mo", ago_seconds / 2_592_000),
    };
    format!("{} · {relative}", absolute_label(timestamp_ms))
}

/// `15 Nov 21:40` in the same local-time terms the rest of the app shows.
fn absolute_label(timestamp_ms: i64) -> String {
    match chrono::DateTime::from_timestamp_millis(timestamp_ms) {
        Some(utc) => utc
            .with_timezone(&chrono::Local)
            .format("%-d %b %H:%M")
            .to_string(),
        None => String::new(),
    }
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
    let first_highlight = hit
        .highlights
        .first()
        .map(|highlight| highlight.start)
        .unwrap_or(0);
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
