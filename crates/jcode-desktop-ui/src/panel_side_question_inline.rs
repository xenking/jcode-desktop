use super::*;

impl Panel {
    pub(in crate::panel) fn record_inline_side_question(&mut self, child_session_id: &str, question: &str) {
        self.push_inline_side_question(InlineQuestion {
            child_session_id: child_session_id.into(),
            first: InlineTurn::pending(question),
            follow_ups: Vec::new(),
        });
    }

    pub(in crate::panel) fn push_inline_side_question(&mut self, question: InlineQuestion) {
        self.flush_reasoning();
        self.flush_streaming();
        self.items.push(Item::SideQuestion(Box::new(question)));
        self.transcript_measurements.invalidate();
    }

    pub(in crate::panel) fn render_inline_side_question(
        &self,
        index: usize,
        question: &InlineQuestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let key = expandable::Key::details(index);
        let expanded = self.expanded_regions.contains(&key);
        let child_session_id = question.child_session_id.clone();
        let preview = question
            .first
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
                        let already_open = this.side_question_child()
                            == Some(child_session_id.as_str())
                            && this.side_question_history.is_none();
                        if already_open || !this.expanded_regions.contains(&key) {
                            this.toggle_line(&key, cx);
                        }
                        if !already_open {
                            this.open_saved_side_question(&child_session_id, cx);
                        }
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
                        .children(question.turns().enumerate().map(|(turn_index, turn)| {
                            let prefix = format!("btw-{index}-{turn_index}");
                            let status = match &turn.outcome {
                                None => Some("Thinking…"),
                                Some(Outcome::Failed(error)) if error.trim().is_empty() => {
                                    Some("Side question stopped")
                                }
                                Some(Outcome::Failed(error)) => Some(error.as_str()),
                                Some(Outcome::Done) => None,
                                Some(Outcome::Closed) => Some("Side question closed"),
                            };
                            div()
                                .id(("inline-btw-turn", turn_index))
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .gap_2()
                                .child(
                                    div()
                                        .debug_selector(move || {
                                            format!("inline-btw-question-{turn_index}")
                                        })
                                        .text_color(theme.TEXT)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(text_selection::plain(
                                            self.transcript_selection.clone(),
                                            format!("{prefix}-question"),
                                            turn.question.clone(),
                                            window,
                                            cx,
                                        )),
                                )
                                .when(!turn.answer.is_empty(), |el| {
                                    el.child(
                                        div()
                                            .debug_selector(move || {
                                                format!("inline-btw-answer-{turn_index}")
                                            })
                                            .min_w_0()
                                            .font_family(theme.FONT_AI)
                                            .child(markdown::render_inline_answer(
                                                &turn.answer,
                                                index,
                                                &prefix,
                                                &self.transcript_selection,
                                                window,
                                                cx,
                                                self.media_preview_handler(cx),
                                            )),
                                    )
                                })
                                .children(status.map(|status| {
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.TEXT_DIM)
                                        .child(status.to_owned())
                                }))
                        })),
                )
            })
            .into_any_element()
    }
}

pub(in crate::panel) fn screenshot_fixture() -> Vec<Item> {
    let thread = screenshot_thread();
    let mut turns = thread.turns.iter().map(InlineTurn::from_saved);
    vec![
        Item::User("Review the new import job.".into()),
        Item::Assistant("I’ll trace the happy path and check where retries happen.".into()),
        Item::SideQuestion(Box::new(InlineQuestion {
            child_session_id: thread.session.session_id,
            first: turns.next().unwrap(),
            follow_ups: turns.collect(),
        })),
        Item::Assistant(
            "The import is safe to retry. It uses a stable source ID to avoid duplicate rows."
                .into(),
        ),
    ]
}

pub(in crate::panel) fn screenshot_thread() -> crate::harness::SideQuestionThread {
    use crate::harness::{SideQuestionThread, SideQuestionTurn};
    SideQuestionThread {
        session: serde_json::from_value(serde_json::json!({
            "session_id": "btw-fixture", "status": "idle",
            "context": {"model": "claude-opus-5-5", "reasoning_effort": "high"}
        }))
        .expect("valid BTW fixture session"),
        turns: vec![
            SideQuestionTurn {
                question: "What happens to rows that fail validation?".into(),
                answer: "Invalid rows go to the quarantine table with their failure reason.".into(),
            },
            SideQuestionTurn {
                question: "Can I retry those rows?".into(),
                answer: "Yes. Correct the validation errors, then retry just those rows.".into(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_turn_snapshot_remains_readable() {
        let snapshot = serde_json::json!({
            "child_session_id": "side", "question": "Original?", "answer": "Yes.", "outcome": "Done"
        });
        let thread: InlineQuestion = serde_json::from_value(snapshot.clone()).unwrap();
        assert_eq!(serde_json::to_value(thread).unwrap(), snapshot);
    }

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
        assert!(vcx.debug_bounds("side-question-card").is_some());

        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inline-btw-question-0").is_some());
        assert!(vcx.debug_bounds("inline-btw-answer-0").is_some());
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
