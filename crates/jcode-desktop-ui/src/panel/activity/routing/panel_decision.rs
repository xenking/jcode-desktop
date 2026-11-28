//! Completed ask results are user decisions, not tool output.
use super::classify::answered;
use super::*;
use jcode_session_types::{AskAnswer, AskRecord};

struct DecisionQuestion {
    title: String,
    answer: String,
}

fn questions(
    ask: Option<&AskRecord>,
    input: &str,
    output: &str,
    error: Option<&str>,
) -> Vec<DecisionQuestion> {
    if let Some(record) = ask.filter(|record| !record.declined()) {
        let answers = record.response.answers.as_deref().unwrap_or_default();
        let skipped = AskAnswer::default();
        return record
            .questions
            .iter()
            .enumerate()
            .map(|(i, question)| DecisionQuestion {
                title: question.question.clone(),
                answer: answer_text(answers.get(i).unwrap_or(&skipped)),
            })
            .collect();
    }
    // Nothing was answered: every question shows how the ask ended.
    let status = terminal_status(ask, output, error);
    let asked: Vec<String> = match ask.filter(|record| !record.questions.is_empty()) {
        Some(record) => record
            .questions
            .iter()
            .map(|q| q.question.clone())
            .collect(),
        None => serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|v| v.get("questions")?.as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|q| Some(q.get("question")?.as_str()?.to_owned()))
            .collect(),
    };
    if asked.is_empty() {
        return vec![DecisionQuestion {
            title: "Decision".into(),
            answer: status,
        }];
    }
    asked
        .into_iter()
        .map(|title| DecisionQuestion {
            title,
            answer: status.clone(),
        })
        .collect()
}

fn terminal_status(ask: Option<&AskRecord>, output: &str, error: Option<&str>) -> String {
    let result = error
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(output)
        .trim();
    if ask.is_some_and(AskRecord::declined) || result.to_lowercase().contains("declin") {
        "declined".into()
    } else if result.to_lowercase().contains("cancel") {
        "cancelled".into()
    } else if result.is_empty() {
        "no answer".into()
    } else {
        result.to_owned()
    }
}

pub(super) fn selection_segments(
    index: usize,
    ask: Option<&AskRecord>,
    input: &str,
    output: &str,
    error: Option<&str>,
) -> Vec<(String, String)> {
    questions(ask, input, output, error)
        .into_iter()
        .enumerate()
        .flat_map(|(i, entry)| {
            [
                (format!("decision-question-{index}-{i}"), entry.title),
                (format!("decision-answer-{index}-{i}"), entry.answer),
            ]
        })
        .collect()
}

/// One question's answer as shown on the card: chosen options marked, their
/// comments and the free text labelled, or a skip.
fn answer_text(answer: &AskAnswer) -> String {
    let mut lines: Vec<String> = Vec::new();
    for label in &answer.selected {
        lines.push(format!("✓ {label}"));
        if let Some(comment) = answer
            .comments
            .get(label)
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
        {
            lines.push(format!("User comment: {comment}"));
        }
    }
    match answer
        .other
        .as_deref()
        .map(str::trim)
        .filter(|o| !o.is_empty())
    {
        Some(other) => lines.push(format!("User answer: {other}")),
        None if !answer.other_images.is_empty() => {
            lines.push("User answer: (image attached)".into())
        }
        None => {}
    }
    if lines.is_empty() {
        "Unanswered (user skipped)".into()
    } else {
        lines.join("\n")
    }
}

impl Panel {
    #[expect(
        clippy::too_many_arguments,
        reason = "rendering needs decision data and GPUI context"
    )]
    pub(super) fn render_decision(
        &self,
        index: usize,
        call_id: &str,
        ask: Option<&AskRecord>,
        input: &str,
        output: &str,
        error: Option<&str>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let entries = questions(ask, input, output, error);
        let answered = answered(ask, error);
        let summary = if answered {
            ""
        } else if error.unwrap_or(output).to_lowercase().contains("declin") {
            "Declined"
        } else if error.unwrap_or(output).to_lowercase().contains("timeout") {
            "Timed out"
        } else if error.unwrap_or(output).to_lowercase().contains("cancel") {
            "Cancelled"
        } else {
            "No response"
        };
        let mut card = div()
            .id(("decision-card", index))
            .debug_selector(|| "decision-card".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
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
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Decision"))
                    .child(div().text_color(Theme::global().TEXT_DIM).child(summary))
                    .children(self.render_tool_clock(index, call_id, true, cx)),
            );
        card = card.child(
            div()
                .text_size(px(11.0))
                .text_color(Theme::global().TEXT_FAINT)
                .child(self.tool_timing_detail(call_id)),
        );
        {
            let count = entries.len();
            let mut image_offset = 0;
            for (i, entry) in entries.into_iter().enumerate() {
                let marker_count = crate::image_chip::image_marker_ranges(&entry.answer).len();
                let answer = entry.answer;
                let question_selector = format!("decision-question-{}", entry.title);
                let answer_selector = format!("decision-answer-{answer}");
                let question = text_selection::plain(
                    self.transcript_selection.clone(),
                    format!("decision-question-{index}-{i}"),
                    entry.title,
                    window,
                    cx,
                );
                let answer_element =
                    if marker_count == 0 && self.ask_result_images(call_id).is_empty() {
                        text_selection::plain(
                            self.transcript_selection.clone(),
                            format!("decision-answer-{index}-{i}"),
                            answer,
                            window,
                            cx,
                        )
                    } else {
                        self.render_ask_answer_part(
                            call_id,
                            answer,
                            image_offset,
                            i + 1 == count,
                            Some((
                                self.transcript_selection.clone(),
                                format!("decision-answer-{index}-{i}"),
                            )),
                            cx,
                        )
                    };
                card = card.child(
                    div()
                        .debug_selector(|| "decision-detail".into())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .child(
                            div()
                                .debug_selector(move || question_selector.clone())
                                .font_weight(FontWeight::MEDIUM)
                                .child(question),
                        )
                        .child(
                            div()
                                .debug_selector(move || answer_selector.clone())
                                .min_w_0()
                                .text_color(Theme::global().USER_ACCENT)
                                .child(
                                    div()
                                        .debug_selector(|| "decision-choice".into())
                                        .child(answer_element),
                                ),
                        ),
                );
                image_offset += marker_count;
            }
        }
        div()
            .flex()
            .flex_none()
            .w_full()
            .min_w_0()
            .items_start()
            .gap(px(6.))
            .when_some(self.absolute_prompt_number(index), |row, number| {
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
            })
            .child(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_session_types::{AskOption, AskQuestion, AskResponse};

    fn question(text: &str, labels: &[&str]) -> AskQuestion {
        AskQuestion {
            question: text.into(),
            header: None,
            options: labels
                .iter()
                .map(|label| AskOption {
                    label: (*label).into(),
                    description: None,
                })
                .collect(),
            multi_select: true,
            allow_other: true,
        }
    }

    fn typed_record() -> AskRecord {
        let first = AskAnswer {
            selected: vec!["A".into(), "B".into()],
            other: Some("custom".into()),
            comments: [("A".to_owned(), "first; second\nthird".to_owned())].into(),
            ..Default::default()
        };
        let second = AskAnswer {
            selected: vec!["Later".into()],
            ..Default::default()
        };
        AskRecord::new(
            vec![
                question("Which?", &["A", "B"]),
                question("When?", &["Now", "Later"]),
            ],
            AskResponse {
                answers: Some(vec![first, second]),
            },
        )
    }

    #[test]
    fn typed_answer_marks_choices_and_keeps_multiline_comments() {
        let record = typed_record();
        let entries = questions(Some(&record), "", "", None);
        assert_eq!(entries[0].title, "Which?");
        assert_eq!(
            entries[0].answer,
            "✓ A\nUser comment: first; second\nthird\n✓ B\nUser answer: custom"
        );
        assert_eq!(entries[1].answer, "✓ Later");
    }

    #[test]
    fn session_saved_before_records_renders_like_its_typed_twin() {
        // The prose a session stored before answers were typed.
        let prose = "User answers:\n1. Which? -> A (comment: first; second\nthird); B; Other: custom\n2. When? -> Later\n";
        let legacy = ask::resolve_record("ask", None, prose).expect("legacy prose resolves");
        let typed = typed_record();
        assert_eq!(
            questions(Some(&legacy), "", prose, None)
                .into_iter()
                .map(|e| (e.title, e.answer))
                .collect::<Vec<_>>(),
            questions(Some(&typed), "", "", None)
                .into_iter()
                .map(|e| (e.title, e.answer))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn declined_and_unanswered_asks_show_how_they_ended_not_a_choice() {
        let input = r#"{"questions":[{"question":"Which?"}]}"#;
        let declined = ask::resolve_record("ask", None, jcode_session_types::ASK_DECLINED_MESSAGE);
        let entries = questions(
            declined.as_deref(),
            input,
            jcode_session_types::ASK_DECLINED_MESSAGE,
            None,
        );
        assert_eq!(
            (entries[0].title.as_str(), entries[0].answer.as_str()),
            ("Which?", "declined")
        );
        assert!(!answered(declined.as_deref(), None));
        let cancelled = questions(None, input, "", Some("cancelled by the user"));
        assert_eq!(cancelled[0].answer, "cancelled");
        let skipped = AskRecord::new(
            vec![question("Which?", &["A", "B"])],
            AskResponse {
                answers: Some(vec![AskAnswer::default()]),
            },
        );
        assert_eq!(
            questions(Some(&skipped), "", "", None)[0].answer,
            "Unanswered (user skipped)"
        );
    }
}
