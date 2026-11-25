//! Syntax treatment for selectable expanded tool output.
use super::*;
use gpui::{HighlightStyle, StyledText};
use std::ops::Range;

pub(super) struct ToolDetail {
    pub text: String,
    argument_keys: Vec<Range<usize>>,
    output: Range<usize>,
}

impl ToolDetail {
    pub(super) fn arguments(input: &str) -> Self {
        let mut text = String::new();
        let mut argument_keys = Vec::new();
        if let Ok(serde_json::Value::Object(arguments)) = serde_json::from_str(input) {
            for (key, value) in arguments {
                if value.is_null() || !tool_argument_visible(&key) {
                    continue;
                }
                if !text.is_empty() {
                    text.push('\n');
                }
                let start = text.len();
                text.push_str(&key);
                text.push(':');
                argument_keys.push(start..text.len());
                text.push(' ');
                match value {
                    serde_json::Value::String(value) => text.push_str(&value),
                    other => {
                        text.push_str(&serde_json::to_string_pretty(&other).unwrap_or_default())
                    }
                }
            }
        } else {
            text.push_str(input.trim());
        }
        Self {
            text,
            argument_keys,
            output: 0..0,
        }
    }

    pub(super) fn new(name: &str, input: &str, output: &str, error: Option<&str>) -> Self {
        let mut detail = Self::arguments(input);
        if !output.is_empty() {
            let output = strip_ansi(output);
            let output = if name == "bg" {
                output
                    .strip_prefix("Background task finished.\n\n")
                    .or_else(|| output.strip_prefix("Background task was already finished.\n\n"))
                    .unwrap_or(&output)
                    .to_owned()
            } else if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
                serde_json::to_string_pretty(&json).unwrap_or(output)
            } else {
                output
            };
            if !detail.text.is_empty() {
                detail.text.push_str("\n\n");
            }
            let start = detail.text.len();
            detail.text.push_str(&output);
            detail.output = start..detail.text.len();
        }
        if let Some(error) = error {
            detail.text.push_str("\n\nError\n");
            detail.text.push_str(&strip_ansi(error));
        }
        detail
    }

    fn highlights(
        &self,
        name: &str,
        input: &str,
        selection: Option<(Range<usize>, HighlightStyle)>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let key_style = HighlightStyle {
            color: Some(crate::theme::to_hsla(Theme::global().TEXT_FAINT)),
            ..Default::default()
        };
        let mut highlights = self
            .argument_keys
            .iter()
            .cloned()
            .map(|range| (range, key_style))
            .collect::<Vec<_>>();
        if !self.output.is_empty() {
            let offset = self.output.start;
            highlights.extend(
                syntax_ranges(name, input, &self.text[self.output.clone()])
                    .into_iter()
                    .map(|(range, style)| ((offset + range.start)..(offset + range.end), style)),
            );
        }
        highlights.extend(selection);
        markdown::flatten_highlights(&highlights)
    }
}

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
) -> Option<(ToolDetail, String)> {
    if name != "bash" {
        return None;
    }
    let input_value: serde_json::Value = serde_json::from_str(input).ok()?;
    input_value.get("command")?.as_str()?;
    let mut args = ToolDetail::arguments(input);
    let mut result = strip_ansi(output);
    let trimmed = result.trim_end_matches('\n');
    if let Some((before, last)) = trimmed.rsplit_once('\n') {
        if last.starts_with("--- Command finished with exit code: ") && last.ends_with(" ---") {
            result = before.trim_end_matches('\n').to_owned();
        }
    } else if trimmed.starts_with("--- Command finished with exit code: ")
        && trimmed.ends_with(" ---")
    {
        result.clear();
    }
    if let Some(error) = error {
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.push_str("Error\n");
        result.push_str(&strip_ansi(error));
    }
    if !result.is_empty() {
        args.text.push('\n');
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

pub(super) fn selectable_detail(
    name: &str,
    input: &str,
    detail: ToolDetail,
    key: String,
    selection: Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    let selected = selection.read(cx).highlight(&key, detail.text.len());
    let highlights = detail.highlights(name, input, selected);
    let mut style = window.text_style();
    style.font_family = Theme::global().mono_family();
    style.color = crate::theme::to_hsla(Theme::global().CODE_TEXT);
    let styled = StyledText::new(detail.text.clone()).with_default_highlights(&style, highlights);
    let layout = styled.layout().clone();
    text_selection::selectable(selection, key, detail.text, layout, styled, cx)
}

pub(super) fn selectable_arguments(
    args: ToolDetail,
    index: usize,
    selection: Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .debug_selector(|| "tool-argument-table".into())
        .min_w_0()
        .child(selectable_detail(
            "",
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
    fn bash_error_without_output_does_not_start_with_blank_lines() {
        let (_, output) =
            bash_sections("bash", r#"{"command":"false"}"#, "", Some("denied")).unwrap();
        assert_eq!(output, "Error\ndenied");
    }
    #[test]
    fn argument_keys_are_highlighted_even_with_multiline_values() {
        let input = r#"{"command":"echo привет\n世界","notify":null,"task_id":"abc","timeout":0,"wake":false,"éclair":["🦀"]}"#;
        let detail = ToolDetail::arguments(input);
        assert_eq!(
            detail.text,
            "command: echo привет\n世界\ntimeout: 0\nwake: false\néclair: [\n  \"🦀\"\n]"
        );
        let keys: Vec<_> = detail
            .highlights("bash", input, None)
            .into_iter()
            .map(|(range, _)| &detail.text[range])
            .collect();
        assert_eq!(keys, ["command:", "timeout:", "wake:", "éclair:"]);
    }
    #[test]
    fn output_syntax_and_selection_keep_ordered_utf8_ranges() {
        let input = r#"{"file_path":"é.rs","flag":null,"note":"界"}"#;
        let output = "@@ -1 +1 @@\n-é\n+界🦀";
        let detail = ToolDetail::new("edit", input, output, Some(output));
        let spans = detail.highlights("edit", input, None);
        let fragments: Vec<_> = spans
            .iter()
            .map(|(range, _)| &detail.text[range.clone()])
            .collect();
        assert_eq!(fragments, ["file_path:", "note:", "-é\n", "+界🦀"]);
        let start = detail.text.find("note:").unwrap() + "no".len();
        let end = detail.text.find("+界").unwrap() + "+界".len();
        let selection = HighlightStyle {
            color: Some(crate::theme::to_hsla(Theme::global().TEXT_USER)),
            ..Default::default()
        };
        let spans = detail.highlights("edit", input, Some((start..end, selection)));
        assert!(
            spans
                .windows(2)
                .all(|pair| pair[0].0.end <= pair[1].0.start)
        );
        let fragments: Vec<_> = spans
            .iter()
            .map(|(range, _)| &detail.text[range.clone()])
            .collect();
        assert_eq!(
            fragments,
            ["file_path:", "no", "te: 界\n\n@@ -1 +1 @@\n-é\n+界", "🦀"]
        );
        assert_eq!(spans[2].1.color, selection.color);
        assert_eq!(
            spans[3].1.color,
            Some(crate::theme::to_hsla(Theme::global().OK))
        );
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
