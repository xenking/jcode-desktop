use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineQuestion {
    child_session_id: String,
    pub(super) question: String,
    pub(super) answer: String,
    outcome: Option<Outcome>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Outcome {
    Done,
    Failed(String),
    Closed,
}

fn latest_question<'a>(
    items: &'a mut Transcript,
    child_session_id: &str,
) -> Option<&'a mut InlineQuestion> {
    let index = items.iter().rposition(|item| {
        matches!(item, Item::SideQuestion(question) if question.child_session_id == child_session_id)
    })?;
    let Item::SideQuestion(question) = &mut items[index] else {
        return None;
    };
    Some(question)
}

pub(super) fn apply_event(items: &mut Transcript, child_session_id: &str, event: &ApiEvent) {
    let outcome = match event {
        ApiEvent::TextDelta { .. } => None,
        ApiEvent::TurnDone { .. } => Some(Outcome::Done),
        ApiEvent::TurnStopped { message, .. } | ApiEvent::Error { message, .. } => {
            Some(Outcome::Failed(message.clone()))
        }
        _ => return,
    };
    let Some(question) = latest_question(items, child_session_id) else {
        return;
    };
    if let ApiEvent::TextDelta { text, .. } = event {
        question.answer.push_str(text);
    } else {
        question.outcome = outcome;
    }
}

pub(super) fn close(items: &mut Transcript, child_session_id: &str) {
    if let Some(question) = latest_question(items, child_session_id)
        && question.outcome.is_none()
    {
        question.outcome = Some(Outcome::Closed);
    }
}

impl Panel {
    pub(super) fn record_inline_side_question(&mut self, child_session_id: &str, question: &str) {
        self.flush_reasoning();
        self.flush_streaming();
        self.items.push(Item::SideQuestion(Box::new(InlineQuestion {
            child_session_id: child_session_id.into(),
            question: question.into(),
            answer: String::new(),
            outcome: None,
        })));
        self.transcript_measurements.dirty = true;
    }

    pub(super) fn render_inline_side_question(
        &self,
        index: usize,
        question: &InlineQuestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let key = expandable::key::details(index);
        let expanded = self.expanded_regions.contains(&key);
        let preview = question
            .question
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap_2()
            .child(
                crate::keyboard::button(div().id(("inline-btw", index)))
                    .debug_selector(|| "inline-btw".into())
                    .aria_expanded(expanded)
                    .role(gpui::Role::Button)
                    .cursor_pointer()
                    .self_start()
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .text_size(px(12.))
                    .bg(theme.INLINE_CODE_BG)
                    .hover(|el| el.bg(theme.TOOL_BORDER))
                    .child(div().flex_none().text_color(theme.ACCENT).child("BTW:"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.TEXT_DIM)
                            .child(preview),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.TEXT_DIM)
                            .child(if expanded { "▾" } else { "▸" }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_line(&key, cx);
                    })),
            )
            .when(expanded, |el| {
                el.child(
                    div()
                        .debug_selector(|| "inline-btw-content".into())
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .gap_2()
                        .pl_2()
                        .child(
                            div()
                                .debug_selector(|| "inline-btw-question".into())
                                .text_color(theme.TEXT)
                                .font_weight(FontWeight::MEDIUM)
                                .child(text_selection::plain(
                                    self.transcript_selection.clone(),
                                    format!("btw-question-{index}"),
                                    question.question.clone(),
                                    window,
                                    cx,
                                )),
                        )
                        .when(!question.answer.is_empty(), |el| {
                            el.child(
                                div()
                                    .debug_selector(|| "inline-btw-answer".into())
                                    .min_w_0()
                                    .font_family(theme.FONT_AI)
                                    .child(markdown::render_interactive(
                                        &question.answer,
                                        index,
                                        &self.transcript_selection,
                                        window,
                                        cx,
                                        false,
                                        self.media_preview_handler(cx),
                                    )),
                            )
                        })
                        .when_some(
                            match &question.outcome {
                                None => Some("Thinking…"),
                                Some(Outcome::Failed(error)) if error.trim().is_empty() => {
                                    Some("Side question stopped")
                                }
                                Some(Outcome::Failed(error)) => Some(error.as_str()),
                                Some(Outcome::Done) => None,
                                Some(Outcome::Closed) => Some("Side question closed"),
                            },
                            |el, status| {
                                el.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.TEXT_DIM)
                                        .child(status.to_owned()),
                                )
                            },
                        ),
                )
            })
            .into_any_element()
    }
}

pub(super) fn screenshot_fixture() -> Vec<Item> {
    vec![
        Item::User("Review the new import job.".into()),
        Item::Assistant("I’ll trace the happy path and check where retries happen.".into()),
        Item::SideQuestion(Box::new(InlineQuestion {
            child_session_id: "btw-fixture".into(),
            question: "What happens to rows that fail validation?".into(),
            answer: "Invalid rows go to the quarantine table with their failure reason. Valid rows continue through the import.".into(),
            outcome: Some(Outcome::Done),
        })),
        Item::Assistant("The import is safe to retry. It uses a stable source ID to avoid duplicate rows.".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn btw_stays_in_order_and_collapses_only_in_clean_chat(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("source", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        panel.update(vcx, |panel, cx| {
            panel.load_history(vec![], vec![], cx);
            panel.set_clean_chat(Some(true), cx);
            panel.items.push(Item::User("Review the import.".into()));
        });
        let session =
            serde_json::from_value(serde_json::json!({"session_id": "side", "status": "ready"}))
                .unwrap();
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::SideQuestionStarted {
                    parent_session_id: "source".into(),
                    session,
                    question: "What happens to invalid rows?".into(),
                },
                cx,
            );
        });
        panel.update(vcx, |panel, cx| {
            panel
                .items
                .push(Item::Assistant("The main report follows here.".into()));
            cx.notify();
        });
        workspace.update(vcx, |workspace, cx| {
            for event in [
                ApiEvent::TextDelta {
                    session_id: "side".into(),
                    text: "Invalid rows are quarantined.".into(),
                    message_id: None,
                },
                ApiEvent::TurnDone {
                    session_id: "side".into(),
                    pending_soft_interrupts: None,
                },
            ] {
                workspace.apply(
                    crate::harness::Update::Event {
                        session_id: "side".into(),
                        event,
                    },
                    cx,
                );
            }
        });
        vcx.run_until_parked();
        let pill = vcx
            .debug_bounds("inline-btw")
            .expect("inline question pill");
        assert!(vcx.debug_bounds("transcript-row-0").unwrap().bottom() <= pill.top());
        assert!(vcx.debug_bounds("transcript-row-2").unwrap().top() >= pill.bottom());
        assert!(vcx.debug_bounds("inline-btw-content").is_none());
        assert!(vcx.debug_bounds("side-question-card").is_none());

        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inline-btw-question").is_some());
        assert!(vcx.debug_bounds("inline-btw-answer").is_some());
        let pill = vcx.debug_bounds("inline-btw").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inline-btw-content").is_none());

        panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(false), cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inline-btw").is_none());
        assert!(vcx.debug_bounds("side-question-card").is_some());
        assert!(vcx.debug_bounds("side-question-current-prompt").is_some());
        assert!(vcx.debug_bounds("side-question-follow-up").is_some());
    }
}
