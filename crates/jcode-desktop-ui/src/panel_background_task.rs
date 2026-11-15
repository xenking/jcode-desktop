//! Running work stays quiet. Finished work leaves a compact, readable result card.
use crate::text_selection::{self, TextSelection};
use crate::theme::Theme;
use gpui::{prelude::*, *};

struct TaskTooltip(String);
impl Render for TaskTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("TaskTooltip");
        div()
            .max_w(px(420.0))
            .p_2()
            .rounded_md()
            .bg(Theme::global().HEADER_BG)
            .text_size(px(12.0))
            .text_color(Theme::global().TEXT)
            .child(self.0.clone())
    }
}

pub(super) fn render(
    index: usize,
    task_id: &str,
    label: &str,
    summary: &str,
    percent: Option<f32>,
    done: bool,
    selection: &Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let theme = Theme::global();
    if done {
        return completed_card(index, task_id, label, summary, selection, window, cx)
            .into_any_element();
    }
    let details = format!(
        "{label}\n{}\n{summary}",
        if done {
            "Finished"
        } else {
            "Running in background"
        }
    );
    div()
        .id(("background-task", index))
        .debug_selector(|| "background-task-card".into())
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_w_0()
        .h(px(24.0))
        .px_1()
        .text_size(px(11.0))
        .tooltip(move |_, cx| cx.new(|_| TaskTooltip(details.clone())).into())
        .child(
            div()
                .flex_none()
                .text_color(if done { theme.OK } else { theme.WARN })
                .child(if done { "✓" } else { "●" }),
        )
        .child(
            div()
                .max_w(relative(0.4))
                .min_w_0()
                .truncate()
                .text_color(theme.TOOL_TEXT)
                .child(text_selection::plain(
                    selection.clone(),
                    format!("background-label-{index}"),
                    label.to_owned(),
                    window,
                    cx,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.TEXT_DIM)
                .child(text_selection::plain(
                    selection.clone(),
                    format!("background-summary-{index}"),
                    summary.to_owned(),
                    window,
                    cx,
                )),
        )
        .when_some(percent.filter(|value| value.is_finite()), |el, percent| {
            el.child(
                div()
                    .flex_none()
                    .w(px(40.0))
                    .h(px(2.0))
                    .rounded_full()
                    .bg(theme.TOOL_BORDER)
                    .child(
                        div()
                            .h_full()
                            .w(relative((percent / 100.0).clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(if done { theme.OK } else { theme.ACCENT }),
                    ),
            )
        })
        .into_any_element()
}

fn completion_color(summary: &str) -> Rgba {
    let theme = Theme::global();
    let status = summary.split('·').next().unwrap_or(summary).to_lowercase();
    if status.contains("failed") || status.contains("error") {
        theme.ERROR
    } else if status.contains("cancel") || status.contains("superseded") {
        theme.WARN
    } else if status.contains("completed") {
        theme.OK
    } else {
        theme.TEXT_DIM
    }
}

/// The short trailing text of a finished task: just the duration when it
/// succeeded, the status first when it did not.
fn compact_summary(summary: &str) -> String {
    let parts: Vec<&str> = summary.split('·').map(str::trim).collect();
    let status = parts.first().copied().unwrap_or_default();
    let duration = parts
        .iter()
        .find(|p| p.ends_with('s') && p.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .copied();
    let ok = status.to_lowercase().contains("completed")
        && !parts
            .iter()
            .any(|p| p.starts_with("exit ") && *p != "exit 0");
    match (ok, duration) {
        (true, Some(d)) => d.to_owned(),
        (true, None) => String::new(),
        (false, _) => summary.trim().to_owned(),
    }
}

fn completed_card(
    index: usize,
    _task_id: &str,
    label: &str,
    summary: &str,
    selection: &Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    // An element carries one debug selector, so the card marker sits on a
    // wrapper and the completed marker on the row itself.
    div()
        .debug_selector(|| "background-task-card".into())
        .child(completed_row(index, label, summary, selection, window, cx))
}

fn completed_row(
    index: usize,
    label: &str,
    summary: &str,
    selection: &Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    let theme = Theme::global();
    div()
        .id(("background-task", index))
        .debug_selector(|| "background-task-completed".into())
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_w_0()
        .py_1()
        .child(crate::tool_icon::render("bg"))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(13.0))
                .text_color(theme.TOOL_TEXT)
                .child(text_selection::plain(
                    selection.clone(),
                    format!("background-label-{index}"),
                    label.to_owned(),
                    window,
                    cx,
                )),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(11.0))
                .text_color(match completion_color(summary) {
                    c if c == theme.OK => theme.TEXT_FAINT,
                    c => c,
                })
                .child(text_selection::plain(
                    selection.clone(),
                    format!("background-summary-{index}"),
                    compact_summary(summary),
                    window,
                    cx,
                )),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn compact_summary_keeps_only_what_matters() {
        assert_eq!(compact_summary("✓ completed · 11.0s · exit 0"), "11.0s");
        assert_eq!(
            compact_summary("✗ failed · 2s · exit 1"),
            "✗ failed · 2s · exit 1"
        );
    }

    #[core::prelude::v1::test]
    fn completion_status_does_not_assume_success() {
        let theme = Theme::global();
        assert_eq!(completion_color("✓ completed · 8.2s · exit 0"), theme.OK);
        assert_eq!(completion_color("✗ failed · 2s · exit 1"), theme.ERROR);
        assert_eq!(completion_color("cancelled · 2s"), theme.WARN);
        assert_eq!(completion_color("finished"), theme.TEXT_DIM);
    }
}
