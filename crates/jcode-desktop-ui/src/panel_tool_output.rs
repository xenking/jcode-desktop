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

#[allow(clippy::too_many_arguments, reason = "rendering needs tool data and GPUI selection context")]
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
    let mut highlights = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
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
