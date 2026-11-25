//! One-line summaries of tool calls and how long they took.
use super::*;

/// The human-readable intent of a tool call, with a useful argument fallback.
#[cfg(test)]
pub(super) fn tool_summary(input: &str) -> String {
    condense(&tool_summary_full(input), 90)
}

/// The same intent, complete. Collapsing it is a paint decision
/// (`expandable::TITLE`), so the text is never lost on the way in.
pub(super) fn tool_summary_full(input: &str) -> String {
    if let Some(summary) = tool_streaming::summary(input) {
        return summary.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    if serde_json::from_str::<serde_json::Value>(input).is_ok() {
        input.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        // The name already identifies the tool. Do not fill its header with
        // unfinished JSON while waiting for a useful argument to complete.
        String::new()
    }
}

pub(super) fn json_scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub(super) fn tool_row_summary(name: &str, input: &str) -> String {
    if name.trim_start_matches("functions.") == "memory" {
        return memory_tool_summary(input);
    }
    if name == "bg"
        && serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|v| v.get("intent").and_then(|v| v.as_str()).map(str::to_owned))
            .is_none()
    {
        "Background task".into()
    } else {
        tool_summary_full(input)
    }
}

/// What a `memory` call did, in the same words the TUI row uses: the raw
/// arguments would otherwise read as `remember: <the whole note>` or as
/// `recall` with no query, because `action` is not a summary field.
pub(super) fn memory_tool_summary(input: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(input) else {
        return tool_summary_full(input);
    };
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim()
    };
    match text("action") {
        "remember" => {
            let content = text("content");
            if content.is_empty() {
                "remember".into()
            } else {
                format!("remember: {}", condense(content, 90))
            }
        }
        "recall" => {
            let query = text("query");
            if query.is_empty() {
                "recall (recent)".into()
            } else {
                format!("recall '{}'", condense(query, 80))
            }
        }
        "search" => {
            let query = text("query");
            if query.is_empty() {
                "search".into()
            } else {
                format!("search '{}'", condense(query, 80))
            }
        }
        "forget" => {
            let id = text("id");
            if id.is_empty() {
                "forget".into()
            } else {
                format!("forget {id}")
            }
        }
        "tag" => {
            let id = text("id");
            if id.is_empty() {
                "tag".into()
            } else {
                format!("tag {id}")
            }
        }
        "list" => "list memories".into(),
        "link" => "link memories".into(),
        "related" => "related memories".into(),
        action if !action.is_empty() => action.to_string(),
        _ => tool_summary_full(input),
    }
}

pub(super) fn tool_argument_visible(key: &str) -> bool {
    !matches!(
        key,
        "intent" | "label" | "id" | "task_id" | "call_id" | "session_id" | "request_id"
    ) && !key.ends_with("_id")
}

/// Expanded tool detail: the invocation and complete ANSI-free output.
pub(super) fn tool_detail(name: &str, input: &str, output: &str) -> String {
    tool_output::ToolDetail::new(name, input, output, None).into_text()
}

pub(super) type HistoryToolTimes = HashMap<String, (Option<i64>, Option<i64>, Option<Duration>)>;

pub(super) fn history_tool_times(messages: &[jcode_sdk::HistoryMessage]) -> HistoryToolTimes {
    messages
        .iter()
        .filter_map(|message| {
            let tool = message.tool.as_ref()?;
            let finish = message.timestamp_ms;
            let duration = tool.duration_ms.map(Duration::from_millis);
            let start = finish
                .zip(tool.duration_ms.and_then(|ms| i64::try_from(ms).ok()))
                .and_then(|(finish, duration)| finish.checked_sub(duration));
            Some((tool.id.clone(), (start, finish, duration)))
        })
        .collect()
}

pub(super) fn compact_clock() -> gpui::Div {
    div()
        .debug_selector(|| "tool-elapsed".into())
        .flex_none()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .text_size(px(11.0))
        .text_color(Theme::global().TEXT_FAINT)
}

pub(super) fn tool_clock_label(ms: i64) -> Option<String> {
    use chrono::TimeZone;
    Some(
        chrono::Local
            .timestamp_millis_opt(ms)
            .single()?
            .format("%H:%M:%S")
            .to_string(),
    )
}

#[test]
pub(super) fn tool_detail_omits_null_arguments_but_keeps_false_and_zero() {
    let detail = tool_detail(
        "bash",
        r#"{"command":"pwd","notify":null,"wake":false,"timeout":0}"#,
        "",
    );
    assert!(detail.contains("command: pwd"));
    assert!(detail.contains("wake: false"));
    assert!(detail.contains("timeout: 0"));
    assert!(!detail.contains("notify"));
}

#[test]
pub(super) fn history_tool_times_reconstruct_start_and_preserve_missing_data() {
    let rows: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
        {"role":"tool", "content":"", "timestamp_ms":20000,
         "tool":{"id":"a", "name":"bash", "input":{}, "duration_ms":7000}},
        {"role":"tool", "content":"", "timestamp_ms":30000,
         "tool":{"id":"b", "name":"read", "input":{}}}
    ]))
    .unwrap();
    let (items, _) = crate::panel::history_items::build(rows.clone(), Vec::new());
    assert!(matches!(&items[0], Item::Tool { name, .. } if name == "bash"));
    assert!(matches!(&items[1], Item::Tool { name, .. } if name == "read"));
    let times = history_tool_times(&rows);
    assert_eq!(
        times.get("a"),
        Some(&(Some(13000), Some(20000), Some(Duration::from_secs(7))))
    );
    assert_eq!(times.get("b"), Some(&(None, Some(30000), None)));
}

pub(super) fn tool_bg_status(name: &str, output: &str) -> Option<String> {
    if name != "bg" {
        return None;
    }
    output
        .lines()
        .find_map(|line| line.strip_prefix("Status: "))
        .map(str::trim)
        .filter(|status| !status.is_empty())
        .map(str::to_owned)
}

/// Keep a block short from the top, noting how much was hidden.
pub(super) fn acknowledge_next(
    pending: &mut VecDeque<usize>,
    accepted: &mut HashMap<usize, Instant>,
    now: Instant,
) {
    if let Some(index) = pending.pop_front() {
        accepted.insert(index, now);
    }
}
