//! What a transcript row or tool call is. Pure predicates on items and tool
//! names, shared by the cards that draw them and the logic that scrolls,
//! pins and pages them, so none of those modules has to reach into another's
//! card code to ask a question about a row.
use super::*;
use jcode_sdk::api::{CodemodeStatus, CodemodeSummary};

pub(super) const EXTERNAL_DM: &str = "Cross-swarm DM from ";

pub(super) fn is_external(message: &str) -> bool {
    message.starts_with(EXTERNAL_DM)
}

pub(super) fn answered(ask: Option<&jcode_session_types::AskRecord>, error: Option<&str>) -> bool {
    error.is_none() && ask.is_some_and(|record| !record.declined())
}

pub(super) fn is_pinnable_prompt(item: &Item) -> bool {
    let Item::User(text) = item else {
        return false;
    };
    let text = text.trim();
    // Background notifications can arrive in restored history as user-role
    // markdown. Keep them in the transcript, never in the pinned reminder.
    // Auto-poke and quality-gate continuations are not prompts either.
    !text.is_empty()
        && !jcode_sdk::todo::is_auto_poke_message(text)
        && ![
            "**Background task** `",
            "**Background task started** `",
            "**Background task progress** `",
            "**Background task stalled** `",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
}


pub(super) fn is_user_turn(item: &Item) -> bool {
    is_pinnable_prompt(item)
        || matches!(item, Item::Tool { name, done: true, ask: Some(_), error: None, .. } if name == "ask")
}

pub(super) fn is_swarm(name: &str) -> bool {
    name.trim_start_matches("functions.") == "swarm"
}

pub(super) fn is_codemode(name: &str) -> bool {
    name.trim_start_matches("functions.") == "codemode"
}

pub(super) fn wrapper_failed(
    summary: Option<&CodemodeSummary>,
    error: Option<&str>,
    output: &str,
) -> bool {
    error.is_some()
        || summary.map_or_else(
            || output.starts_with("error: "),
            |summary| summary.status == CodemodeStatus::Error,
        )
}

pub(super) fn is_retry_phase(phase: &str) -> bool {
    phase.trim().to_ascii_lowercase().starts_with("retrying")
}

