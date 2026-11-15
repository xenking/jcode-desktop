//! Inline prompt autocompletion ("ghost text") for the chat composer.
//!
//! [`GhostCompletion`] is the accept/dismiss state machine. It is free of GPUI
//! so its rules are unit tested here; `PromptInput` drives it from key events
//! and paints [`GhostCompletion::visible`] as dim text after the caret.
//! [`fetch`] calls the backend's `POST /v1/completions/prompt`.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Whether `draft` may be sent for completion at all.
pub fn should_request(draft: &str) -> bool {
    let trimmed = draft.trim_start();
    !draft.trim().is_empty() && !trimmed.starts_with('/') && !draft.ends_with('\n')
}

#[derive(Debug, Default)]
pub struct GhostCompletion {
    generation: u64,
    /// Draft the in-flight request was made for.
    pending: Option<(u64, String)>,
    /// Draft the suggestion continues, and the suggested remainder.
    shown: Option<(String, String)>,
}

impl GhostCompletion {
    /// The draft changed. Returns the generation to request with when a
    /// completion should be fetched (after the caller's debounce), or `None`.
    /// Typing a prefix of the visible suggestion keeps the rest showing.
    pub fn on_edit(&mut self, draft: &str, cursor_at_end: bool) -> Option<u64> {
        self.generation += 1;
        self.pending = None;
        if let Some((base, suffix)) = self.shown.take()
            && cursor_at_end
                && let Some(typed) = draft.strip_prefix(base.as_str())
                    && !typed.is_empty() && suffix.len() > typed.len()
                        && let Some(rest) = suffix.strip_prefix(typed) {
                            self.shown = Some((draft.to_owned(), rest.to_owned()));
                            return None;
                        }
        if !cursor_at_end || !should_request(draft) {
            return None;
        }
        self.pending = Some((self.generation, draft.to_owned()));
        Some(self.generation)
    }

    /// True while `generation` is still the newest request.
    pub fn is_current(&self, generation: u64) -> bool {
        self.pending.as_ref().is_some_and(|(g, _)| *g == generation)
    }

    /// A response arrived. Stale responses and empty completions are dropped.
    /// Returns whether a suggestion became visible.
    pub fn on_response(&mut self, generation: u64, completion: &str) -> bool {
        let Some((g, draft)) = self.pending.take() else {
            return false;
        };
        if g != generation {
            self.pending = Some((g, draft));
            return false;
        }
        let completion = completion.split(['\n', '\r']).next().unwrap_or("");
        if completion.trim().is_empty() {
            return false;
        }
        self.shown = Some((draft, completion.to_owned()));
        true
    }

    /// Remainder to paint after the caret, only while the draft is exactly
    /// the one the suggestion continues and the caret sits at its end.
    pub fn visible(&self, draft: &str, cursor_at_end: bool) -> Option<&str> {
        let (base, suffix) = self.shown.as_ref()?;
        (cursor_at_end && base == draft).then_some(suffix.as_str())
    }

    /// Tab: take the visible remainder, or `None` so Tab keeps its old meaning.
    pub fn accept(&mut self, draft: &str, cursor_at_end: bool) -> Option<String> {
        let suffix = self.visible(draft, cursor_at_end)?.to_owned();
        self.shown = None;
        self.pending = None;
        self.generation += 1;
        Some(suffix)
    }

    /// Esc or any unrelated edit. Returns whether a suggestion was hidden.
    pub fn dismiss(&mut self) -> bool {
        self.generation += 1;
        self.pending = None;
        self.shown.take().is_some()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Turn {
    pub role: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub draft: String,
    pub context: Vec<Turn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    completion: String,
}

#[derive(Debug)]
pub enum FetchError {
    /// The backend does not offer completions. Stop asking this session.
    Unsupported,
    Other(String),
}

/// Blocking: run on a background thread.
pub fn fetch(api_base: &str, api_key: &str, request: &Request) -> Result<String, FetchError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| FetchError::Other(e.to_string()))?;
    let url = format!("{}/completions/prompt", api_base.trim_end_matches('/'));
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .map_err(|e| FetchError::Other(e.to_string()))?;
    match response.status().as_u16() {
        200 => response
            .json::<Response>()
            .map(|r| r.completion)
            .map_err(|e| FetchError::Other(e.to_string())),
        401 | 403 | 404 | 501 => Err(FetchError::Unsupported),
        status => Err(FetchError::Other(format!("HTTP {status}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_empty_slash_and_newline_drafts() {
        assert!(!should_request(""));
        assert!(!should_request("   "));
        assert!(!should_request("/model"));
        assert!(!should_request("  /help"));
        assert!(!should_request("line\n"));
        assert!(should_request("исправь баг"));
    }

    #[test]
    fn response_shows_and_tab_accepts() {
        let mut g = GhostCompletion::default();
        let generation = g.on_edit("fix the", true).unwrap();
        assert!(g.is_current(generation));
        assert!(g.on_response(generation, " bug in input.rs"));
        assert_eq!(g.visible("fix the", true), Some(" bug in input.rs"));
        assert_eq!(g.visible("fix the", false), None);
        assert_eq!(
            g.accept("fix the", true).as_deref(),
            Some(" bug in input.rs")
        );
        assert_eq!(g.visible("fix the", true), None);
        // Nothing visible: Tab falls through.
        assert_eq!(g.accept("fix the", true), None);
    }

    #[test]
    fn stale_responses_are_dropped() {
        let mut g = GhostCompletion::default();
        let first = g.on_edit("fix", true).unwrap();
        let second = g.on_edit("fix t", true).unwrap();
        assert!(!g.is_current(first));
        assert!(!g.on_response(first, "ed"));
        assert!(g.on_response(second, "he bug"));
        assert_eq!(g.visible("fix t", true), Some("he bug"));
    }

    #[test]
    fn typing_through_keeps_the_rest_and_divergence_dismisses() {
        let mut g = GhostCompletion::default();
        let generation = g.on_edit("добавь", true).unwrap();
        g.on_response(generation, " тесты для input");
        assert_eq!(g.on_edit("добавь т", true), None);
        assert_eq!(g.visible("добавь т", true), Some("есты для input"));
        // Diverging text hides the ghost and asks again.
        assert!(g.on_edit("добавь тх", true).is_some());
        assert_eq!(g.visible("добавь тх", true), None);
    }

    #[test]
    fn escape_dismisses_and_cancels_pending() {
        let mut g = GhostCompletion::default();
        let generation = g.on_edit("hello", true).unwrap();
        assert!(!g.dismiss());
        assert!(!g.on_response(generation, " world"));
        let generation = g.on_edit("hello", true).unwrap();
        g.on_response(generation, " world");
        assert!(g.dismiss());
        assert_eq!(g.visible("hello", true), None);
    }

    #[test]
    fn empty_or_multiline_completions() {
        let mut g = GhostCompletion::default();
        let generation = g.on_edit("x", true).unwrap();
        assert!(!g.on_response(generation, "  "));
        let generation = g.on_edit("x", true).unwrap();
        assert!(g.on_response(generation, "yz\nmore"));
        assert_eq!(g.visible("x", true), Some("yz"));
    }

    #[test]
    fn mid_text_caret_does_not_request() {
        let mut g = GhostCompletion::default();
        assert_eq!(g.on_edit("hello", false), None);
    }
}
