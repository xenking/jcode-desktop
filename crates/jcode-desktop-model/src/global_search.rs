//! Search across every stored session, as the session picker uses it.
//!
//! The picker (`/resume`, Cmd+Shift+F, the sidebar magnifier) is one screen:
//! a session list with a live preview. Typing filters the list by title and
//! folder locally and asks the daemon's full-text index for matching messages.
//! This module owns the daemon half (query, debounce generation, stale-reply
//! dropping, status line) and the row layout that interleaves message hits
//! under the session they belong to, so all of it is tested without a window.
//!
//! Layout rules:
//! - a session row comes first, then up to [`HITS_PER_SESSION`] of its message
//!   hits, so one busy session cannot bury the others;
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

/// Message hits listed under one session row. More than this is a sign to
/// refine the query, and the preview already shows the session itself.
pub const HITS_PER_SESSION: usize = 3;

/// The daemon half of the picker's search.
#[derive(Debug, Default)]
pub struct Search {
    query: String,
    /// Identifies the newest query; replies carry the value they answered.
    generation: u64,
    status: Status,
    results: Option<Results>,
    error: Option<String>,
    elapsed_ms: Option<f64>,
}

/// What a query change obliges the caller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Nothing to send: the query is blank or unchanged.
    Cleared,
    /// Send `Search::pending_query()` after [`DEBOUNCE`].
    Debounce(u64),
}

/// One row of the picker list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickRow {
    Session(String),
    /// `hit` indexes [`Results::messages`].
    Hit {
        session_id: String,
        hit: usize,
    },
}

impl PickRow {
    pub fn session_id(&self) -> &str {
        match self {
            Self::Session(id) | Self::Hit { session_id: id, .. } => id,
        }
    }

    pub fn hit(&self) -> Option<usize> {
        match self {
            Self::Session(_) => None,
            Self::Hit { hit, .. } => Some(*hit),
        }
    }
}

impl Search {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn status(&self) -> Status {
        self.status
    }

    /// Results that answer the current query. Results for an older query are
    /// kept on screen while the next one is debounced, so the list does not
    /// blank on every keystroke.
    pub fn results(&self) -> Option<&Results> {
        self.results.as_ref()
    }

    /// The query a pending debounce should send, and the generation it must
    /// carry so a late reply can be recognised as stale.
    pub fn pending_query(&self) -> (&str, u64) {
        (&self.query, self.generation)
    }

    /// Apply a keystroke's worth of query text.
    pub fn set_query(&mut self, query: &str) -> Effect {
        if query == self.query {
            return Effect::Cleared;
        }
        self.query = query.to_string();
        self.generation += 1;
        self.error = None;
        if query_terms(query).is_empty() {
            self.status = Status::Idle;
            self.results = None;
            self.elapsed_ms = None;
            return Effect::Cleared;
        }
        self.status = Status::Waiting;
        Effect::Debounce(self.generation)
    }

    /// Mark the query as sent. Returns false if a newer query has been typed,
    /// in which case the caller must not send this one.
    pub fn mark_sent(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.status = Status::Searching;
        true
    }

    /// Apply a daemon reply. Stale replies are dropped: threads finish out of
    /// order, and only the newest query is worth showing.
    pub fn apply_results(&mut self, generation: u64, results: Results, elapsed_ms: f64) -> bool {
        if generation != self.generation || self.status == Status::Idle {
            return false;
        }
        self.status = Status::Ready;
        self.error = None;
        self.results = Some(results);
        self.elapsed_ms = Some(elapsed_ms);
        true
    }

    pub fn apply_error(&mut self, generation: u64, error: String) -> bool {
        if generation != self.generation || self.status == Status::Idle {
            return false;
        }
        self.status = Status::Failed;
        self.error = Some(error);
        self.results = None;
        true
    }

    /// Whether the conversation half of the search is still outstanding.
    pub fn pending(&self) -> bool {
        matches!(self.status, Status::Waiting | Status::Searching)
    }

    /// Sessions the daemon matched, by title or by message, best rank first,
    /// with the title it reported.
    pub fn matched_sessions(&self) -> Vec<(String, String)> {
        let Some(results) = &self.results else {
            return Vec::new();
        };
        let mut seen = std::collections::HashSet::new();
        results
            .titles
            .iter()
            .chain(&results.messages)
            .filter(|hit| seen.insert(hit.session_id.clone()))
            .map(|hit| (hit.session_id.clone(), hit.title.clone()))
            .collect()
    }

    /// Message hits for one session, in rank order, with their indices.
    pub fn session_hits(&self, session_id: &str) -> Vec<(usize, &SearchHit)> {
        self.results
            .iter()
            .flat_map(|results| results.messages.iter().enumerate())
            .filter(|(_, hit)| hit.session_id == session_id)
            .collect()
    }

    pub fn message(&self, hit: usize) -> Option<&SearchHit> {
        self.results.as_ref()?.messages.get(hit)
    }

    /// The picker list: each session, then its first message hits.
    pub fn rows(&self, sessions: &[String]) -> Vec<PickRow> {
        let mut rows = Vec::with_capacity(sessions.len());
        for session_id in sessions {
            rows.push(PickRow::Session(session_id.clone()));
            for (hit, _) in self
                .session_hits(session_id)
                .into_iter()
                .take(HITS_PER_SESSION)
            {
                rows.push(PickRow::Hit {
                    session_id: session_id.clone(),
                    hit,
                });
            }
        }
        rows
    }

    /// What opening `row` should scroll to: the hit itself, or for a session
    /// row its best hit, so Enter always lands on the conversation that
    /// matched rather than on the newest message.
    pub fn reveal_for(&self, row: &PickRow) -> Option<String> {
        let hit = match row.hit() {
            Some(hit) => self.message(hit)?,
            None => self.session_hits(row.session_id()).first()?.1,
        };
        Some(reveal_needle(hit))
    }

    /// The status line's search half, or `None` when no search is running.
    pub fn status_line(&self) -> Option<String> {
        match self.status {
            Status::Idle => None,
            Status::Waiting | Status::Searching => Some("searching conversations…".into()),
            Status::Failed => Some(format!(
                "conversation search unavailable: {}",
                self.error.as_deref().unwrap_or("unknown error")
            )),
            Status::Ready => {
                let results = self.results.as_ref()?;
                let count = results.messages.len();
                let more = if results.truncated { "+" } else { "" };
                let elapsed = self
                    .elapsed_ms
                    .map(|ms| format!(" · {ms:.0}ms"))
                    .unwrap_or_default();
                Some(format!(
                    "{count}{more} matching message{}{elapsed}",
                    plural(count)
                ))
            }
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Move a selection by `delta` rows, clamped at both ends. A selection that
/// left the list starts over at the first row.
pub fn step(rows: &[PickRow], current: Option<&PickRow>, delta: isize) -> Option<PickRow> {
    if rows.is_empty() {
        return None;
    }
    let index = current
        .and_then(|current| rows.iter().position(|row| row == current))
        .map_or(0, |index| {
            index.saturating_add_signed(delta).min(rows.len() - 1)
        });
    rows.get(index).cloned()
}

/// The words of a query worth highlighting: FTS syntax stripped, and single
/// characters dropped because they match nearly everything.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
        let word = fold(word);
        if word.chars().count() >= 2 && !terms.contains(&word) {
            terms.push(word);
        }
    }
    terms
}

/// Lowercase and fold `ё` to `е`, the same way the search index does.
fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// One char in, one char out, so byte offsets stay recoverable even when
/// lowercasing would change a character's length.
fn fold_char(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    if lower == 'ё' { 'е' } else { lower }
}

/// Byte ranges in `text` where any of `terms` occurs, case-insensitively.
/// Ranges are sorted, disjoint and fall on char boundaries, so they can style
/// rendered text directly.
pub fn term_ranges(text: &str, terms: &[String]) -> Vec<std::ops::Range<usize>> {
    if terms.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text
        .char_indices()
        .map(|(at, c)| (at, fold_char(c)))
        .collect();
    let terms: Vec<Vec<char>> = terms
        .iter()
        .map(|term| term.chars().map(fold_char).collect::<Vec<_>>())
        .filter(|term| !term.is_empty())
        .collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let longest = terms
            .iter()
            .filter(|term| {
                index + term.len() <= chars.len()
                    && chars[index..index + term.len()]
                        .iter()
                        .zip(term.iter())
                        .all(|((_, a), b)| a == b)
            })
            .map(Vec::len)
            .max();
        match longest {
            Some(len) => {
                let start = chars[index].0;
                let end = chars.get(index + len).map_or(text.len(), |(at, _)| *at);
                ranges.push(start..end);
                index += len;
            }
            None => index += 1,
        }
    }
    ranges
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
pub fn reveal_needle(hit: &SearchHit) -> String {
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
