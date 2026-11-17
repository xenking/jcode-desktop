//! Completed ask results are user decisions, not tool output.
use super::*;

struct DecisionQuestion {
    title: String,
    answer: String,
    choices: Vec<String>,
}

pub(super) fn answered(output: &str, error: Option<&str>) -> bool {
    error.is_none() && (output.starts_with("User answers:") || output.contains(" -> "))
}

fn questions(input: &str, output: &str, error: Option<&str>) -> Vec<DecisionQuestion> {
    let parsed = serde_json::from_str::<serde_json::Value>(input).ok();
    let list = parsed
        .as_ref()
        .and_then(|v| v.get("questions"))
        .and_then(|v| v.as_array());
    let shown = ask::result_text(input, output, error);
    let Some(list) = list else {
        return vec![DecisionQuestion {
            title: "Decision".into(),
            answer: shown,
            choices: Vec::new(),
        }];
    };
    list.iter()
        .enumerate()
        .map(|(i, q)| {
            let full = q
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Question");
            let title = q
                .get("header")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(full);
            let prefix = format!("{}. {} -> ", i + 1, full);
            let answer = shown
                .find(&prefix)
                .map(|start| {
                    let rest = &shown[start + prefix.len()..];
                    let next = list
                        .get(i + 1)
                        .and_then(|q| q.get("question"))
                        .and_then(|q| q.as_str())
                        .map(|question| format!("\n{}. {} -> ", i + 2, question));
                    next.and_then(|next| rest.find(&next))
                        .map_or(rest, |end| &rest[..end])
                        .trim()
                        .to_owned()
                })
                .unwrap_or_else(|| shown.clone());
            let choices = q
                .get("options")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("label").and_then(|v| v.as_str()))
                .map(str::to_owned)
                .collect();
            DecisionQuestion {
                title: title.into(),
                answer,
                choices,
            }
        })
        .collect()
}

// The stored model-facing result predates structured decision metadata. Split
// only top-level separators so comments containing semicolons remain intact.
fn marked_answer(entry: &DecisionQuestion, answered: bool) -> String {
    if !answered {
        return entry.answer.clone();
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, ch) in entry.answer.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 && entry.answer[index..].starts_with("; ") => {
                parts.push(&entry.answer[start..index]);
                start = index + 2;
            }
            _ => {}
        }
    }
    parts.push(&entry.answer[start..]);
    parts
        .into_iter()
        .map(|part| {
            let selected = entry.choices.iter().any(|label| {
                part == label
                    || part
                        .strip_prefix(label)
                        .is_some_and(|tail| tail.starts_with(" (comment: "))
            });
            if selected {
                format!("✓ {part}")
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Panel {
    pub(super) fn render_decision(
        &self,
        index: usize,
        call_id: &str,
        input: &str,
        output: &str,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let entries = questions(input, output, error);
        let expanded = self.expanded_regions.contains(call_id);
        let toggle_key = call_id.to_string();
        let keyboard_key = toggle_key.clone();
        let focus = self
            .swarm_row_focus
            .borrow_mut()
            .entry(call_id.to_string())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let click_focus = focus.clone();
        let answered = answered(output, error);
        let summary = if answered {
            format!(
                "{} question{} answered",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" }
            )
        } else if output.to_lowercase().contains("declin") {
            "Declined".into()
        } else if error.unwrap_or(output).to_lowercase().contains("timeout") {
            "Timed out".into()
        } else if error.unwrap_or(output).to_lowercase().contains("cancel") {
            "Cancelled".into()
        } else {
            "No response".into()
        };
        let mut card = div()
            .id(("decision-card", index))
            .debug_selector(|| "decision-card".into())
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_xl()
            .bg(Theme::global().USER_BG)
            .text_color(Theme::global().TEXT_USER)
            .child(
                div()
                    .id(("decision-header", index))
                    .debug_selector(|| "decision-header".into())
                    .cursor_pointer()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Decision"))
                    .child(div().text_color(Theme::global().TEXT_DIM).child(summary))
                    .track_focus(&focus)
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" && !event.is_held {
                            this.toggle_region(&keyboard_key, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&click_focus, cx);
                        this.toggle_region(&toggle_key, cx);
                        cx.stop_propagation();
                    })),
            );
        if expanded {
            let count = entries.len();
            let mut image_offset = 0;
            for (i, entry) in entries.into_iter().enumerate() {
                let marker_count = crate::input::image_marker_ranges(&entry.answer).len();
                let answer = marked_answer(&entry, answered);
                card = card.child(
                    div()
                        .debug_selector(|| "decision-detail".into())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_weight(FontWeight::MEDIUM).child(entry.title))
                        .child(
                            div()
                                .debug_selector(|| "decision-choice".into())
                                .text_color(Theme::global().USER_ACCENT)
                                .child(self.render_ask_answer_part(
                                    call_id,
                                    answer,
                                    image_offset,
                                    i + 1 == count,
                                    cx,
                                )),
                        ),
                );
                image_offset += marker_count;
            }
        } else if let Some(entry) = entries.first() {
            card = card.child(
                div()
                    .debug_selector(|| "decision-choice".into())
                    .text_color(Theme::global().USER_ACCENT)
                    .truncate()
                    .child(marked_answer(entry, answered).replace('\n', "  ")),
            );
        }
        div()
            .flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .items_start()
            .gap(px(6.))
            .when_some(
                self.absolute_prompt_number(index),
                |row, number| {
                    row.child(
                        div()
                            .debug_selector(move || format!("decision-number-{number}"))
                            .mt(px(4.))
                            .px_2()
                            .h(px(20.))
                            .rounded_full()
                            .bg(Theme::global().USER_BG)
                            .text_size(px(10.))
                            .line_height(px(20.))
                            .text_color(Theme::global().TEXT_DIM)
                            .child(number.to_string()),
                    )
                },
            )
            .child(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_selected_options_and_preserves_multiline_comments() {
        let input = r#"{"questions":[{"header":"Storage","question":"Which?","options":[{"label":"A"},{"label":"B"}]},{"question":"When?","options":[{"label":"Now"},{"label":"Later"}]}]}"#;
        let output = "User answers:\n1. Which? -> A (comment: first; second\nthird); B; Other: custom\n2. When? -> Later\n";
        let entries = questions(input, output, None);
        assert_eq!(entries[0].title, "Storage");
        assert_eq!(
            marked_answer(&entries[0], true),
            "✓ A (comment: first; second\nthird)\n✓ B\nOther: custom"
        );
        assert_eq!(marked_answer(&entries[1], true), "✓ Later");
        assert!(
            !marked_answer(
                &questions(input, "User declined to answer the questions.", None)[0],
                false
            )
            .contains('✓')
        );
    }
}
