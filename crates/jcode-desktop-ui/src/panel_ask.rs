//! Inline card for the `ask` tool: the agent blocks on the user's answers.
//!
//! Options are click targets. Free-text "Other" answers come from the
//! composer: type, then press "Use typed text" on that question.
use super::*;
use jcode_sdk::{AskAnswer, AskQuestion, AskResponse};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AskCard {
    pub request_id: String,
    pub questions: Vec<AskQuestion>,
    pub chosen: Vec<Vec<usize>>,
    pub other: Vec<Option<String>>,
}

impl AskCard {
    pub fn new(request_id: String, questions: Vec<AskQuestion>) -> Self {
        let n = questions.len();
        Self {
            request_id,
            questions,
            chosen: vec![Vec::new(); n],
            other: vec![None; n],
        }
    }

    pub fn toggle(&mut self, q: usize, option: usize) {
        let multi = self.questions[q].multi_select;
        let chosen = &mut self.chosen[q];
        if let Some(pos) = chosen.iter().position(|&i| i == option) {
            chosen.remove(pos);
        } else if multi {
            chosen.push(option);
        } else {
            *chosen = vec![option];
            self.other[q] = None;
        }
    }

    pub fn set_other(&mut self, q: usize, text: &str) {
        let text = text.trim();
        self.other[q] = (!text.is_empty()).then(|| text.to_string());
        if self.other[q].is_some() && !self.questions[q].multi_select {
            self.chosen[q].clear();
        }
    }

    pub fn complete(&self) -> bool {
        (0..self.questions.len()).all(|q| !self.chosen[q].is_empty() || self.other[q].is_some())
    }

    pub fn response(&self) -> AskResponse {
        let answers = self
            .questions
            .iter()
            .enumerate()
            .map(|(q, question)| {
                let mut idx = self.chosen[q].clone();
                idx.sort_unstable();
                AskAnswer {
                    selected: idx
                        .into_iter()
                        .filter_map(|i| question.options.get(i).map(|o| o.label.clone()))
                        .collect(),
                    other: self.other[q].clone(),
                }
            })
            .collect();
        AskResponse {
            answers: Some(answers),
        }
    }
}

impl Panel {
    pub(super) fn answer_ask(&mut self, response: AskResponse, cx: &mut Context<Self>) {
        let Some(card) = self.ask_card.take() else {
            return;
        };
        self.bridge.send(Command::AskResponse {
            session_id: self.session_id.clone(),
            request_id: card.request_id,
            response,
        });
        cx.notify();
    }

    pub(super) fn render_ask_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let card = self.ask_card.as_ref()?;
        let theme = Theme::global();
        let mut body = div()
            .id("ask-card")
            .debug_selector(|| "ask-card".into())
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .mx_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.ACCENT_DIM)
            .bg(theme.TOOL_BG)
            .text_size(px(13.))
            .text_color(theme.TEXT)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.TEXT_DIM)
                    .child("The agent is asking you"),
            );
        for (q, question) in card.questions.iter().enumerate() {
            let mut block = div().flex().flex_col().gap_1().min_w_0().child(
                div()
                    .flex()
                    .gap_2()
                    .when_some(question.header.clone(), |el, header| {
                        el.child(div().text_color(theme.ACCENT).child(header))
                    })
                    .child(div().min_w_0().child(question.question.clone())),
            );
            for (o, option) in question.options.iter().enumerate() {
                let selected = card.chosen[q].contains(&o);
                let mark = match (question.multi_select, selected) {
                    (true, true) => "☑",
                    (true, false) => "☐",
                    (false, true) => "●",
                    (false, false) => "○",
                };
                block = block.child(
                    div()
                        .id(("ask-option", q * 16 + o))
                        .debug_selector(move || format!("ask-option-{q}-{o}"))
                        .flex()
                        .gap_2()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .when(selected, |el| el.bg(theme.QUOTE_BG))
                        .hover(|el| el.bg(theme.QUOTE_BG))
                        .child(div().flex_none().child(mark))
                        .child(div().flex_none().child(option.label.clone()))
                        .when_some(option.description.clone(), |el, desc| {
                            el.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.TEXT_DIM)
                                    .child(desc),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(card) = this.ask_card.as_mut() {
                                card.toggle(q, o);
                                cx.notify();
                            }
                        })),
                );
            }
            if question.allow_other {
                let label = match &card.other[q] {
                    Some(text) => format!("Other: {text}"),
                    None => "Other: type in the composer, then click here".to_string(),
                };
                block = block.child(
                    div()
                        .id(("ask-other", q))
                        .debug_selector(move || format!("ask-other-{q}"))
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(theme.TEXT_DIM)
                        .hover(|el| el.bg(theme.QUOTE_BG))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let text = this.input.read(cx).snapshot().content;
                            if let Some(card) = this.ask_card.as_mut() {
                                card.set_other(q, &text);
                            }
                            this.input
                                .update(cx, |input, cx| input.set_content(String::new(), cx));
                            cx.notify();
                        })),
                );
            }
            body = body.child(block);
        }
        let complete = card.complete();
        Some(
            body.child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        div()
                            .id("ask-decline")
                            .debug_selector(|| "ask-decline".into())
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.QUOTE_BG))
                            .child("Decline")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.answer_ask(AskResponse::default(), cx)
                            })),
                    )
                    .child(
                        div()
                            .id("ask-submit")
                            .debug_selector(|| "ask-submit".into())
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .when(complete, |el| {
                                el.cursor_pointer()
                                    .bg(theme.ACCENT_DIM)
                                    .text_color(theme.TEXT)
                            })
                            .when(!complete, |el| el.text_color(theme.TEXT_FAINT))
                            .child("Submit")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(response) = this
                                    .ask_card
                                    .as_ref()
                                    .filter(|card| card.complete())
                                    .map(AskCard::response)
                                {
                                    this.answer_ask(response, cx);
                                }
                            })),
                    ),
            )
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_sdk::AskOption;

    fn question(multi: bool) -> AskQuestion {
        AskQuestion {
            question: "Q?".into(),
            header: None,
            options: vec![
                AskOption {
                    label: "A".into(),
                    description: None,
                },
                AskOption {
                    label: "B".into(),
                    description: None,
                },
            ],
            multi_select: multi,
            allow_other: true,
        }
    }

    #[test]
    fn single_select_replaces_and_other_clears_choice() {
        let mut card = AskCard::new("r".into(), vec![question(false), question(true)]);
        card.toggle(0, 0);
        card.toggle(0, 1);
        assert_eq!(card.chosen[0], vec![1]);
        assert!(!card.complete());
        card.toggle(1, 1);
        card.toggle(1, 0);
        card.set_other(1, " extra ");
        let answers = card.response().answers.unwrap();
        assert_eq!(answers[0].selected, vec!["B"]);
        assert_eq!(answers[1].selected, vec!["A", "B"]);
        assert_eq!(answers[1].other.as_deref(), Some("extra"));
        card.set_other(0, "mine");
        assert!(card.chosen[0].is_empty());
        assert!(card.complete());
    }

    /// The wire frame the Desktop harness emits for the real `ask` call a model
    /// made in an isolated jcode session (question text and options verbatim
    /// from the session journal). Proves the card paints and answers end to end.
    const REAL_ASK_REQUEST: &str = r#"{
        "ev": "ask_request", "session_id": "session-a",
        "request_id": "ask-toolu_01J29QR2NCg12HueHJkMWtZU",
        "tool_call_id": "toolu_01J29QR2NCg12HueHJkMWtZU",
        "questions": [{"header": "Database", "question": "Which database should the new web service use?",
            "options": [
                {"description": "Relational, scalable, rich features", "label": "Postgres"},
                {"description": "Embedded, zero-ops, single node", "label": "SQLite"},
                {"description": "Managed NoSQL on AWS", "label": "DynamoDB"}]}]
    }"#;

    #[gpui::test]
    fn real_ask_request_event_renders_card_and_submits(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.run_until_parked();
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .expect("panel exists");
        // The composer (and the ask card above it) docks over a real transcript.
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
        vcx.run_until_parked();
        for _ in 0..120 {
            if vcx.debug_bounds("composer-dock").is_some() {
                break;
            }
            vcx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            vcx.update(|window, cx| window.simulate_next_frame(cx));
            vcx.run_until_parked();
        }
        assert!(vcx.debug_bounds("composer-dock").is_some());
        assert!(vcx.debug_bounds("ask-card").is_none());

        let event: jcode_sdk::ApiEvent =
            serde_json::from_str(REAL_ASK_REQUEST).expect("real ask_request frame parses");
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();

        assert!(vcx.debug_bounds("ask-card").is_some(), "ask card paints");
        for selector in ["ask-option-0-0", "ask-option-0-1", "ask-option-0-2"] {
            assert!(vcx.debug_bounds(selector).is_some(), "{selector} paints");
        }
        assert!(vcx.debug_bounds("ask-decline").is_some());

        let sqlite = vcx.debug_bounds("ask-option-0-1").unwrap();
        vcx.simulate_click(sqlite.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().expect("card still open");
            assert_eq!(card.chosen[0], vec![1]);
            let answers = card.response().answers.unwrap();
            assert_eq!(answers[0].selected, vec!["SQLite"]);
        });

        let submit = vcx.debug_bounds("ask-submit").unwrap();
        vcx.simulate_click(submit.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-card").is_none(), "card closes after submit");
        panel.read_with(vcx, |panel, _| assert!(panel.ask_card.is_none()));
    }
}
