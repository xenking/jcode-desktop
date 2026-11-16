//! Syntax treatment for selectable expanded tool output.
use super::*;
use gpui::{HighlightStyle, StyledText};
use std::ops::Range;

fn language(name: &str, input: &str, output: &str) -> Option<String> {
    if matches!(name, "edit" | "apply_patch" | "diff")
        && output.lines().any(|line| {
            line.starts_with("@@") || line.starts_with("+++ ") || line.starts_with("--- ")
        })
    {
        return Some("diff".into());
    }
    if name != "read" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let path = value
        .get("file_path")
        .or_else(|| value.get("path"))?
        .as_str()?;
    std::path::Path::new(path)
        .extension()?
        .to_str()
        .map(str::to_owned)
}

pub(super) fn selector(name: &str, input: &str, output: &str) -> &'static str {
    match language(name, input, output).as_deref() {
        Some("diff") => "tool-detail-syntax-diff",
        Some(_) => "tool-detail-syntax-read",
        None => "tool-detail-plain",
    }
}

pub(super) fn bash_exit_code(name: &str, output: &str) -> Option<i32> {
    if name != "bash" {
        return None;
    }
    output.lines().rev().find_map(|line| {
        let line = line.trim();
        let value = line.strip_prefix("Exit code: ").or_else(|| {
            line.strip_prefix("--- Command finished with exit code: ")
                .and_then(|value| value.strip_suffix(" ---"))
        })?;
        value.trim().parse().ok()
    })
}

pub(super) fn bash_sections(
    name: &str,
    input: &str,
    output: &str,
    error: Option<&str>,
) -> Option<(String, String)> {
    if name != "bash" {
        return None;
    }
    let input_value: serde_json::Value = serde_json::from_str(input).ok()?;
    input_value.get("command")?.as_str()?;
    let mut args = tool_detail(name, input, "");
    let mut result = strip_ansi(output);
    if let Some(error) = error {
        result.push_str("\n\nError\n");
        result.push_str(&strip_ansi(error));
    }
    if !result.is_empty() {
        args.push('\n');
    }
    Some((args, result))
}

pub(super) struct BackgroundOutput {
    pub fields: Vec<String>,
    pub preview: Option<String>,
    pub exit_code: Option<i32>,
}

pub(super) fn background_output(name: &str, output: &str) -> Option<BackgroundOutput> {
    if name != "bg" {
        return None;
    }
    let clean = strip_ansi(output);
    let body = clean
        .strip_prefix("Background task finished.\n\n")
        .or_else(|| clean.strip_prefix("Background task was already finished.\n\n"))?;
    let (metadata, preview) = match body.split_once("\n\nOutput preview:\n") {
        Some((metadata, preview)) => {
            let preview = preview
                .strip_prefix("```text\n")
                .and_then(|text| text.trim_end().strip_suffix("```"))
                .unwrap_or(preview);
            (metadata, Some(preview.trim_end().to_string()))
        }
        None => (body, None),
    };
    let fields = metadata.lines().map(str::to_string).collect::<Vec<_>>();
    let exit_code = fields
        .iter()
        .find_map(|line| line.strip_prefix("Exit code: ")?.trim().parse().ok());
    Some(BackgroundOutput {
        fields,
        preview,
        exit_code,
    })
}

/// Prose-bearing tool responses can contain markdown; command and file output
/// must stay literal even when their content resembles markup.
pub(super) fn markdown_output(name: &str, output: &str) -> Option<String> {
    if matches!(
        name,
        "bash" | "bg" | "read" | "edit" | "apply_patch" | "diff"
    ) {
        return None;
    }
    let output = strip_ansi(output);
    if serde_json::from_str::<serde_json::Value>(&output).is_ok() {
        return None;
    }
    let looks_like_markdown = output.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("# ")
            || line.starts_with("## ")
            || line.starts_with("```")
            || line.starts_with("- ")
            || line.starts_with("> ")
    }) || output.contains("**");
    looks_like_markdown.then_some(output)
}

pub(super) fn markdown_row(index: usize) -> usize {
    usize::MAX.saturating_sub(index.saturating_add(1))
}

pub(super) struct JsonLine {
    pub text: String,
    pub toggle: Option<String>,
    pub collapsed: bool,
}

pub(super) fn json_prefix(call_id: &str) -> String {
    format!("tool-json:{call_id}:")
}

/// Visible JSON lines are shared by painting and the logical selection document.
pub(super) fn json_lines(
    name: &str,
    output: &str,
    call_id: &str,
    collapsed: &HashSet<String>,
) -> Option<Vec<JsonLine>> {
    if matches!(
        name,
        "bash" | "bg" | "read" | "edit" | "apply_patch" | "diff"
    ) || output.len() > 200_000
    {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&strip_ansi(output)).ok()?;
    if !value.is_object() && !value.is_array() {
        return None;
    }
    fn walk(
        value: &serde_json::Value,
        label: &str,
        path: String,
        depth: usize,
        comma: bool,
        collapsed: &HashSet<String>,
        lines: &mut Vec<JsonLine>,
    ) {
        let indent = "  ".repeat(depth);
        let suffix = if comma { "," } else { "" };
        let children: Vec<(String, &serde_json::Value)> = match value {
            serde_json::Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    (
                        format!("{}: ", serde_json::to_string(k).unwrap_or_default()),
                        v,
                    )
                })
                .collect(),
            serde_json::Value::Array(items) => items.iter().map(|v| (String::new(), v)).collect(),
            _ => Vec::new(),
        };
        let (open, close) = if value.is_object() {
            ("{", "}")
        } else {
            ("[", "]")
        };
        if !children.is_empty() {
            let is_collapsed = collapsed.contains(&path);
            let text = if is_collapsed {
                format!("{indent}{label}{open}…{close}{suffix}")
            } else {
                format!("{indent}{label}{open}")
            };
            lines.push(JsonLine {
                text,
                toggle: Some(path.clone()),
                collapsed: is_collapsed,
            });
            if !is_collapsed {
                let count = children.len();
                for (i, (child_label, child)) in children.into_iter().enumerate() {
                    walk(
                        child,
                        &child_label,
                        format!("{path}{i}/"),
                        depth + 1,
                        i + 1 != count,
                        collapsed,
                        lines,
                    );
                }
                lines.push(JsonLine {
                    text: format!("{indent}{close}{suffix}"),
                    toggle: None,
                    collapsed: false,
                });
            }
        } else {
            lines.push(JsonLine {
                text: format!("{indent}{label}{}{suffix}", value),
                toggle: None,
                collapsed: false,
            });
        }
    }
    let mut lines = Vec::new();
    walk(
        &value,
        "",
        json_prefix(call_id),
        0,
        false,
        collapsed,
        &mut lines,
    );
    (lines.len() <= 500).then_some(lines)
}

fn syntax_ranges(name: &str, input: &str, output: &str) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(lang) = language(name, input, output) else {
        return Vec::new();
    };
    if lang == "diff" {
        let mut start = 0;
        return output
            .split_inclusive('\n')
            .filter_map(|line| {
                let end = start + line.len();
                let color = if line.starts_with('+') && !line.starts_with("+++") {
                    Some(Theme::global().OK)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Some(Theme::global().ERROR)
                } else {
                    None
                };
                let range = start..end;
                start = end;
                color.map(|color| {
                    (
                        range,
                        HighlightStyle {
                            color: Some(crate::theme::to_hsla(color)),
                            ..Default::default()
                        },
                    )
                })
            })
            .collect();
    }
    markdown::highlight_code(output, &lang).1
}

fn argument_ranges(name: &str, input: &str, detail: &str) -> Vec<(Range<usize>, HighlightStyle)> {
    let Ok(serde_json::Value::Object(args)) = serde_json::from_str::<serde_json::Value>(input)
    else {
        return Vec::new();
    };
    let formatted = tool_detail(name, input, "");
    if !detail.starts_with(&formatted) {
        return Vec::new();
    }
    let mut offset = 0;
    let mut ranges = Vec::new();
    for (key, value) in args {
        let value = match value {
            serde_json::Value::String(text) => text,
            other => serde_json::to_string_pretty(&other).unwrap_or_default(),
        };
        ranges.push((
            offset..offset + key.len() + 1,
            HighlightStyle {
                color: Some(crate::theme::to_hsla(Theme::global().TEXT_FAINT)),
                ..Default::default()
            },
        ));
        offset += key.len() + 2 + value.len() + 1;
    }
    ranges
}

#[allow(
    clippy::too_many_arguments,
    reason = "rendering needs tool data and GPUI selection context"
)]
pub(super) fn selectable_detail(
    name: &str,
    input: &str,
    output: &str,
    detail: String,
    key: String,
    selection: Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    let output = strip_ansi(output);
    let mut highlights = argument_ranges(name, input, &detail);
    if !output.is_empty()
        && let Some(offset) = detail.rfind(&output)
    {
        highlights.extend(
            syntax_ranges(name, input, &output)
                .into_iter()
                .map(|(range, style)| ((range.start + offset)..(range.end + offset), style)),
        );
    }
    if let Some(highlight) = selection.read(cx).highlight(&key, detail.len()) {
        highlights.push(highlight);
    }
    let mut style = window.text_style();
    style.font_family = Theme::global().FONT_MONO.into();
    style.color = crate::theme::to_hsla(Theme::global().CODE_TEXT);
    let styled = StyledText::new(detail.clone())
        .with_default_highlights(&style, markdown::flatten_highlights(&highlights));
    let layout = styled.layout().clone();
    text_selection::selectable(selection, key, detail, layout, styled, cx)
}

pub(super) fn selectable_arguments(
    name: &str,
    input: &str,
    args: String,
    index: usize,
    selection: Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .debug_selector(|| "tool-argument-table".into())
        .min_w_0()
        .child(selectable_detail(
            name,
            input,
            "",
            args,
            format!("tool-detail-{index}"),
            selection,
            window,
            cx,
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_keys_are_highlighted_even_with_multiline_values() {
        let input = r#"{"command":"echo\nhello","task_id":"abc"}"#;
        let detail = tool_detail("bash", input, "");
        let ranges = argument_ranges("bash", input, &detail);
        assert_eq!(&detail[ranges[0].0.clone()], "command:");
        assert_eq!(&detail[ranges[1].0.clone()], "task_id:");
    }
    #[test]
    fn background_parser_retains_fields_and_unfenced_preview() {
        let result = background_output("bg", "Background task was already finished.\n\nTask: abc\nStatus: failed\nDuration: 3s\nExit code: 101\n\nOutput preview:\n```text\nerror details\n```").unwrap();
        assert_eq!(result.exit_code, Some(101));
        assert!(result.fields.contains(&"Task: abc".into()));
        assert_eq!(result.preview.as_deref(), Some("error details"));
        assert!(background_output("bg", "unrecognized result").is_none());
    }
    #[test]
    fn colors_diff_and_read_code() {
        assert_eq!(
            syntax_ranges("edit", "{}", "@@ -1 +1 @@\n-old\n+new\n").len(),
            2
        );
        assert!(
            !syntax_ranges("read", r#"{"file_path":"src/lib.rs"}"#, "fn main() {}\n").is_empty()
        );
        assert!(syntax_ranges("bash", "{}", "plain output").is_empty());
    }
}
