//! Inline card for the `ask` tool: the agent blocks on the user's answers.
//!
// Each question owns its free-answer editor and each selected option its comment.
use super::*;
use jcode_sdk::{AskAnswer, AskQuestion, AskResponse};

/// Completed asks remain readable in the transcript, including after history reload.
pub(super) fn result_text(input: &str, output: &str, error: Option<&str>) -> String {
    let result = error
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(output)
        .trim();
    if let Some(answers) = result.strip_prefix("User answers:\n") {
        return answers.to_string();
    }
    if result.contains(" -> ") {
        return result.to_string();
    }
    let status = if result.to_lowercase().contains("declin") {
        "declined"
    } else if result.to_lowercase().contains("cancel") {
        "cancelled"
    } else if result.is_empty() {
        "no answer"
    } else {
        result
    };
    let questions = serde_json::from_str::<serde_json::Value>(input).ok();
    let lines: Vec<String> = questions
        .as_ref()
        .and_then(|v| v.get("questions"))
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, q)| {
            q.get("question")
                .and_then(|q| q.as_str())
                .map(|q| format!("{}. {} -> {}", i + 1, q, status))
        })
        .collect();
    if lines.is_empty() {
        status.to_string()
    } else {
        lines.join("\n")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AskDraft {
    request_id: String,
    questions: Vec<AskQuestion>,
    chosen: Vec<Vec<usize>>,
    other: Vec<Option<String>>,
    comments: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AskCard {
    pub request_id: String,
    pub questions: Vec<AskQuestion>,
    pub chosen: Vec<Vec<usize>>,
    pub other: Vec<Option<String>>,
    pub comments: Vec<Vec<String>>,
    pub other_inputs: Vec<Entity<PromptInput>>,
    pub comment_inputs: Vec<Vec<Entity<PromptInput>>>,
    pub cursor_question: usize,
    pub cursor_option: usize,
    pub voice_target: Option<(usize, Option<usize>)>,
    pub submitting: bool,
    pub submit_error: Option<String>,
}

impl AskCard {
    pub fn new(request_id: String, questions: Vec<AskQuestion>) -> Self {
        let n = questions.len();
        Self {
            request_id,
            comments: questions
                .iter()
                .map(|q| vec![String::new(); q.options.len()])
                .collect(),
            questions,
            chosen: vec![Vec::new(); n],
            other: vec![None; n],
            other_inputs: Vec::new(),
            comment_inputs: Vec::new(),
            cursor_question: 0,
            cursor_option: 0,
            voice_target: (n > 0).then_some((0, None)),
            submitting: false,
            submit_error: None,
        }
    }

    pub fn draft(&self) -> AskDraft {
        AskDraft {
            request_id: self.request_id.clone(),
            questions: self.questions.clone(),
            chosen: self.chosen.clone(),
            other: self.other.clone(),
            comments: self.comments.clone(),
        }
    }

    pub fn restore_draft(&mut self, draft: AskDraft, cx: &mut Context<Panel>) {
        if draft.questions != self.questions
            || draft.chosen.len() != self.questions.len()
            || draft.other.len() != self.questions.len()
            || draft.comments.len() != self.questions.len()
            || draft
                .chosen
                .iter()
                .zip(&self.questions)
                .any(|(selected, q)| selected.iter().any(|&i| i >= q.options.len()))
            || draft
                .comments
                .iter()
                .zip(&self.questions)
                .any(|(comments, q)| comments.len() != q.options.len())
        {
            return;
        }
        self.chosen = draft.chosen;
        self.other = draft.other;
        self.comments = draft.comments;
        for (q, input) in self.other_inputs.iter().enumerate() {
            let text = self.other[q].clone().unwrap_or_default();
            input.update(cx, |input, cx| input.set_content(text, cx));
        }
        for (q, inputs) in self.comment_inputs.iter().enumerate() {
            for (o, input) in inputs.iter().enumerate() {
                let text = self.comments[q][o].clone();
                input.update(cx, |input, cx| input.set_content(text, cx));
            }
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
                    comments: self.chosen[q]
                        .iter()
                        .filter_map(|&i| {
                            let text = self.comments[q].get(i)?.trim();
                            let label = &question.options.get(i)?.label;
                            (!text.is_empty()).then(|| (label.clone(), text.to_owned()))
                        })
                        .collect(),
                }
            })
            .collect();
        AskResponse {
            answers: Some(answers),
        }
    }
}

fn question_without_header<'a>(question: &'a str, header: Option<&str>) -> &'a str {
    let Some(header) = header.map(str::trim).filter(|h| !h.is_empty()) else {
        return question;
    };
    let Some(rest) = question.strip_prefix(header) else {
        return question;
    };
    let rest = rest.trim_start();
    match rest.strip_prefix(':') {
        Some(text) if !text.trim().is_empty() => text.trim_start(),
        _ => question,
    }
}

impl Panel {
    pub(super) fn ask_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.ask_card.as_ref().is_some_and(|card| {
            card.other_inputs
                .iter()
                .chain(card.comment_inputs.iter().flatten())
                .any(|input| input.read(cx).focus_handle.is_focused(window))
        })
    }

    pub(super) fn ask_voice_input(&self) -> Option<Entity<PromptInput>> {
        let card = self.ask_card.as_ref()?;
        let (q, option) = card.voice_target?;
        match option {
            Some(o) => card.comment_inputs.get(q)?.get(o).cloned(),
            None => card.other_inputs.get(q).cloned(),
        }
    }

    fn render_ask_microphone(
        &self,
        q: usize,
        option: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let active = self.voice_active()
            && self.ask_card.as_ref().and_then(|card| card.voice_target) == Some((q, option));
        div()
            .id(("ask-voice", q * 16 + option.map_or(0, |o| o + 1)))
            .debug_selector(move || format!("ask-voice-{q}-{}", option.map_or(0, |o| o + 1)))
            .relative()
            .size(px(28.))
            .rounded_full()
            .cursor_pointer()
            .bg(if active { theme.ACCENT } else { theme.TOOL_BG })
            .child(jcode_desktop_voice::chrome::voice_microphone_face(
                if active {
                    theme.BG.into()
                } else {
                    theme.TEXT_DIM.into()
                },
            ))
            .on_click(cx.listener(move |this, _, window, cx| {
                if !this.voice_active()
                    && let Some(card) = this.ask_card.as_mut()
                {
                    card.voice_target = Some((q, option));
                }
                window.focus(&this.focus_handle, cx);
                window.dispatch_action(Box::new(voice::ToggleVoice), cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn handle_ask_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(card) = self.ask_card.as_ref() else {
            return false;
        };
        if card.submitting {
            return true;
        }
        let editing = card
            .other_inputs
            .iter()
            .chain(card.comment_inputs.iter().flatten())
            .any(|input| input.read(cx).focus_handle.is_focused(window));
        if editing && event.keystroke.key == "enter" && !event.keystroke.modifiers.shift {
            if let Some(response) = card.complete().then(|| card.response()) {
                self.answer_ask(response, cx);
            }
            return true;
        }
        if !self.focus_handle.is_focused(window) {
            return false;
        }
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        if card.questions.is_empty() {
            return false;
        }
        let q = card.cursor_question.min(card.questions.len() - 1);
        let count = card.questions[q].options.len();
        match event.keystroke.key.as_str() {
            "up" | "k" if count > 0 => {
                card.cursor_option = (card.cursor_option + count - 1) % count
            }
            "down" | "j" if count > 0 => card.cursor_option = (card.cursor_option + 1) % count,
            "space" if count > 0 => card.toggle(q, card.cursor_option.min(count - 1)),
            "tab" => {
                card.cursor_question = (q + 1) % card.questions.len();
                card.cursor_option = 0;
            }
            "n" if count > 0 => {
                let o = card.cursor_option.min(count - 1);
                if card.chosen[q].contains(&o)
                    && let Some(input) = card.comment_inputs.get(q).and_then(|inputs| inputs.get(o))
                {
                    let focus = input.read(cx).focus_handle.clone();
                    window.focus(&focus, cx);
                }
            }
            "enter" => {
                if card.complete() {
                    let response = card.response();
                    self.answer_ask(response, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn submit_ask_editor(
        &mut self,
        request_id: &str,
        q: usize,
        option: Option<usize>,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(card) = self
            .ask_card
            .as_mut()
            .filter(|card| card.request_id == request_id)
        else {
            return;
        };
        if let Some(o) = option {
            card.comments[q][o] = text.clone();
        } else {
            card.set_other(q, &text);
        }
        if card.complete() {
            let response = card.response();
            self.answer_ask(response, cx);
        } else {
            let input = match option {
                Some(o) => card.comment_inputs.get(q).and_then(|inputs| inputs.get(o)),
                None => card.other_inputs.get(q),
            }
            .cloned();
            if let Some(input) = input {
                cx.defer(move |cx| input.update(cx, |input, cx| input.set_content(text, cx)));
            }
            cx.notify();
        }
    }

    pub(super) fn prepare_ask_inputs(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.ask_card.as_ref() else {
            return;
        };
        let request_id = card.request_id.clone();
        let counts: Vec<usize> = card.questions.iter().map(|q| q.options.len()).collect();
        let mut other_inputs = Vec::new();
        let mut comment_inputs = Vec::new();
        for (q, count) in counts.into_iter().enumerate() {
            let panel = cx.weak_entity();
            let submit = panel.clone();
            let id = request_id.clone();
            let submit_id = id.clone();
            other_inputs.push(cx.new(|cx| {
                PromptInput::new(cx, "Your own answer…", move |text, _, _, app| {
                    let submit = submit.clone();
                    let id = submit_id.clone();
                    app.defer(move |app| {
                        let _ = submit.update(app, |panel, cx| {
                            panel.submit_ask_editor(&id, q, None, text, cx)
                        });
                    });
                })
                .with_on_change(move |text, app| {
                    let _ = panel.update(app, |panel, cx| {
                        if let Some(card) =
                            panel.ask_card.as_mut().filter(|card| card.request_id == id)
                        {
                            card.set_other(q, text);
                            cx.notify();
                        }
                    });
                })
            }));
            let mut inputs = Vec::new();
            for o in 0..count {
                let panel = cx.weak_entity();
                let submit = panel.clone();
                let id = request_id.clone();
                let submit_id = id.clone();
                inputs.push(cx.new(|cx| {
                    PromptInput::new(cx, "Add a comment…", move |text, _, _, app| {
                        let submit = submit.clone();
                        let id = submit_id.clone();
                        app.defer(move |app| {
                            let _ = submit.update(app, |panel, cx| {
                                panel.submit_ask_editor(&id, q, Some(o), text, cx)
                            });
                        });
                    })
                    .with_on_change(move |text, app| {
                        let _ = panel.update(app, |panel, cx| {
                            if let Some(card) =
                                panel.ask_card.as_mut().filter(|card| card.request_id == id)
                            {
                                card.comments[q][o] = text.to_owned();
                                cx.notify();
                            }
                        });
                    })
                }));
            }
            comment_inputs.push(inputs);
        }
        if let Some(card) = self.ask_card.as_mut() {
            card.other_inputs = other_inputs;
            card.comment_inputs = comment_inputs;
        }
    }

    pub(super) fn answer_ask(&mut self, response: AskResponse, cx: &mut Context<Self>) {
        self.cancel_ask_voice(cx);
        let Some(card) = self.ask_card.as_mut() else {
            return;
        };
        if card.submitting {
            return;
        }
        card.submitting = true;
        card.submit_error = None;
        self.bridge.send(Command::AskResponse {
            session_id: self.session_id.clone(),
            request_id: card.request_id.clone(),
            response,
        });
        cx.notify();
    }

    pub(crate) fn ask_response_settled(
        &mut self,
        request_id: &str,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(card) = self
            .ask_card
            .as_mut()
            .filter(|card| card.request_id == request_id && card.submitting)
        else {
            return;
        };
        if let Some(error) = error {
            card.submitting = false;
            card.submit_error = Some(error);
        } else {
            self.ask_card = None;
            self.pending_ask_draft = None;
        }
        cx.notify();
    }

    pub(super) fn render_ask_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let card = self.ask_card.as_ref()?;
        let theme = Theme::global();
        let mut body = div()
            .id("ask-card")
            .debug_selector(|| "ask-card".into())
            .occlude()
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
            .children(
                card.submit_error
                    .as_ref()
                    .map(|error| div().text_color(theme.TEXT_DIM).child(error.clone())),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.TEXT_DIM)
                    .child("The agent is asking you"),
            );
        let mut questions = div()
            .id("ask-scroll")
            .debug_selector(|| "ask-scroll".into())
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .max_h(px((f32::from(window.viewport_size().height) * 0.4).max(60.)))
            .overflow_y_scroll();
        for (q, question) in card.questions.iter().enumerate() {
            let mut block = div()
                .flex()
                .flex_col()
                .gap_1()
                .min_w_0()
                .p_2()
                .rounded_md()
                .bg(theme.QUOTE_BG)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .text_size(px(11.))
                                .child(div().text_color(theme.TEXT_DIM).child(format!(
                                    "{}/{}",
                                    q + 1,
                                    card.questions.len()
                                )))
                                .when_some(question.header.clone(), |el, header| {
                                    el.child(div().text_color(theme.ACCENT).child(header))
                                }),
                        )
                        .child(
                            div().min_w_0().child(
                                question_without_header(
                                    &question.question,
                                    question.header.as_deref(),
                                )
                                .to_string(),
                            ),
                        ),
                );
            for (o, option) in question.options.iter().enumerate() {
                let selected = card.chosen[q].contains(&o);
                let mark = div()
                    .flex_none()
                    .size(px(18.))
                    .border_1()
                    .border_color(if selected {
                        theme.ACCENT
                    } else {
                        theme.TEXT_DIM
                    })
                    .when(question.multi_select, |el| el.rounded_sm())
                    .when(!question.multi_select, |el| el.rounded_full())
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected && !question.multi_select, |el| {
                        el.child(div().size(px(10.)).bg(theme.ACCENT).rounded_full())
                    })
                    .when(selected && question.multi_select, |el| {
                        let color = theme.ACCENT;
                        el.child(
                            gpui::canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    let mut path = gpui::PathBuilder::stroke(px(2.));
                                    path.move_to(bounds.origin + gpui::point(px(2.), px(7.)));
                                    path.line_to(bounds.origin + gpui::point(px(5.), px(10.)));
                                    path.line_to(bounds.origin + gpui::point(px(12.), px(2.)));
                                    if let Ok(path) = path.build() {
                                        window.paint_path(path, color);
                                    }
                                },
                            )
                            .size(px(14.)),
                        )
                    });
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
                        .when(selected, |el| el.bg(theme.ACCENT_DIM))
                        .when(
                            self.focus_handle.is_focused(window)
                                && card.cursor_question == q
                                && card.cursor_option == o,
                            |el| el.border_1().border_color(theme.ACCENT),
                        )
                        .hover(|el| el.bg(theme.QUOTE_BG))
                        .child(mark)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(option.label.clone())
                                .when_some(option.description.clone(), |el, desc| {
                                    el.child(
                                        div()
                                            .min_w_0()
                                            .debug_selector(move || {
                                                format!("ask-description-{q}-{o}")
                                            })
                                            .text_color(theme.TEXT_DIM)
                                            .child(desc),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(card) = this.ask_card.as_mut() {
                                card.cursor_question = q;
                                card.cursor_option = o;
                                card.toggle(q, o);
                                window.focus(&this.focus_handle, cx);
                                cx.notify();
                            }
                        })),
                );
                if selected
                    && let Some(input) = card.comment_inputs.get(q).and_then(|inputs| inputs.get(o))
                {
                    block = block.child(
                        div()
                            .ml(px(28.))
                            .debug_selector(move || format!("ask-comment-{q}-{o}"))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().flex_1().min_w_0().child(input.clone()))
                            .child(self.render_ask_microphone(q, Some(o), cx)),
                    );
                }
            }
            if let Some(input) = card.other_inputs.get(q) {
                let own_selected = card.other[q].is_some();
                block = block.child(
                    div()
                        .id(("ask-other", q))
                        .debug_selector(move || format!("ask-other-{q}"))
                        .px_2()
                        .rounded_sm()
                        .when(own_selected, |el| el.bg(theme.ACCENT_DIM))
                        .child(
                            div()
                                .id(("ask-other-label", q))
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .size(px(18.))
                                        .border_1()
                                        .border_color(if own_selected {
                                            theme.ACCENT
                                        } else {
                                            theme.TEXT_DIM
                                        })
                                        .when(question.multi_select, |el| el.rounded_sm())
                                        .when(!question.multi_select, |el| el.rounded_full())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(own_selected, |el| {
                                            el.child(
                                                div().size(px(9.)).bg(theme.ACCENT).rounded_full(),
                                            )
                                        }),
                                )
                                .child(div().text_color(theme.TEXT_DIM).child("Your own answer"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let Some(input) = this
                                        .ask_card
                                        .as_ref()
                                        .and_then(|card| card.other_inputs.get(q))
                                        .cloned()
                                    else {
                                        return;
                                    };
                                    let text = input.read(cx).snapshot().content;
                                    let focus = input.read(cx).focus_handle.clone();
                                    if let Some(card) = this.ask_card.as_mut() {
                                        card.set_other(q, &text);
                                    }
                                    window.focus(&focus, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("ask-other-editor-{q}"))
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(div().flex_1().min_w_0().child(input.clone()))
                                .child(self.render_ask_microphone(q, None, cx)),
                        ),
                );
            }
            questions = questions.child(block);
        }
        body = body.child(
            div()
                .relative()
                .child(questions)
                .when(card.submitting, |el| {
                    el.child(div().absolute().inset_0().occlude())
                }),
        );
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
                            .child(if card.submitting {
                                "Sending…"
                            } else {
                                "Submit"
                            })
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
    fn repeated_header_is_removed_only_at_a_boundary() {
        assert_eq!(
            question_without_header("Sidebar: what works?", Some("Sidebar")),
            "what works?"
        );
        assert_eq!(
            question_without_header("Sidebar layout?", Some("Sidebar")),
            "Sidebar layout?"
        );
        assert_eq!(
            question_without_header("Sidebar: ", Some("Sidebar")),
            "Sidebar: "
        );
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
    fn large_card_scrolls_without_moving_footer(cx: &mut gpui::TestAppContext) {
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
            .unwrap();
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
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let mut q = value["questions"][0].clone();
        q["options"][0]["description"] = serde_json::Value::String(
            "This description must wrap cleanly without truncating. ".repeat(16),
        );
        value["questions"] = serde_json::Value::Array(vec![q; 4]);
        let event: jcode_sdk::ApiEvent = serde_json::from_value(value).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        let viewport = vcx.debug_bounds("ask-scroll").unwrap();
        let footer = vcx.debug_bounds("ask-submit").unwrap();
        let description = vcx.debug_bounds("ask-description-0-0").unwrap();
        assert!(
            description.size.height > px(24.),
            "long description must wrap: {description:?}"
        );
        let transcript_before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
        vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6000.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
            transcript_before,
            "wheel over ask must not scroll transcript"
        );
        let last = vcx.debug_bounds("ask-other-3").unwrap();
        assert!(
            last.top() >= viewport.top() && last.bottom() <= viewport.bottom() + px(1.),
            "last question must be reachable: {last:?}, {viewport:?}"
        );
        assert_eq!(
            vcx.debug_bounds("ask-submit"),
            Some(footer),
            "footer must remain fixed"
        );
        let option = vcx.debug_bounds("ask-option-3-2").unwrap();
        vcx.simulate_click(option.center(), gpui::Modifiers::default());
        vcx.simulate_keystrokes("tab");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.ask_card.as_ref().unwrap().cursor_question, 0)
        });
        vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6000.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        let own = vcx.debug_bounds("ask-other-editor-3").unwrap();
        vcx.simulate_click(own.center(), gpui::Modifiers::default());
        vcx.simulate_input("saved draft");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            let card = panel
                .ask_card
                .as_ref()
                .expect("incomplete card remains open");
            assert_eq!(
                card.other_inputs[3].read(cx).snapshot().content,
                "saved draft"
            );
        });
    }

    #[test]
    fn completed_ask_result_preserves_answers_and_terminal_status() {
        let input = r#"{"questions":[{"question":"Which?"}]}"#;
        assert_eq!(
            result_text(
                input,
                "User answers:\n1. Which? -> B (comment: why); Other: text\n",
                None
            ),
            "1. Which? -> B (comment: why); Other: text"
        );
        assert_eq!(
            result_text(input, "User declined to answer the questions.", None),
            "1. Which? -> declined"
        );
        assert_eq!(
            result_text(input, "", Some("cancelled by the user")),
            "1. Which? -> cancelled"
        );
        assert_eq!(result_text(input, "", None), "1. Which? -> no answer");
    }

    #[gpui::test]
    fn ask_result_is_visible_and_survives_history_reload(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        let input = serde_json::json!({"questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]});
        let history: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
            {"role":"user","content":"Choose"},
            {"role":"tool","content":"1. Which? -> B (comment: reason)","tool":{"id":"ask-1","name":"ask","input":input}}
        ])).unwrap();
        panel.update(vcx, |panel, cx| panel.load_history(history, vec![], cx));
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| assert!(panel.items.iter().any(|item| matches!(item, Item::Tool { name, output, .. } if name == "ask" && output.contains("reason")))));
        assert!(
            vcx.debug_bounds("ask-result").is_some(),
            "reopened history must render ask result"
        );
    }

    #[gpui::test]
    fn ask_submission_remains_visible_until_confirmed(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.update(vcx, |panel, cx| {
            panel.apply(&event, cx);
            let card = panel.ask_card.as_mut().unwrap();
            card.toggle(0, 1);
            let response = card.response();
            panel.answer_ask(response, cx);
        });
        panel.read_with(vcx, |panel, _| {
            assert_eq!(
                panel
                    .ask_card
                    .as_ref()
                    .expect("must wait for server acknowledgment")
                    .chosen[0],
                vec![1]
            );
        });
        let request_id = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().request_id.clone()
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id: request_id.clone(),
                    error: Some("offline".into()),
                },
                cx,
            )
        });
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            assert!(!card.submitting);
            assert_eq!(card.submit_error.as_deref(), Some("offline"));
            assert_eq!(card.chosen[0], vec![1]);
        });
        panel.update(vcx, |panel, cx| {
            let response = panel.ask_card.as_ref().unwrap().response();
            panel.answer_ask(response, cx);
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id,
                    error: None,
                },
                cx,
            )
        });
        panel.read_with(vcx, |panel, _| assert!(panel.ask_card.is_none()));
    }

    #[gpui::test]
    fn ask_draft_survives_snapshot_and_matching_request(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace.push_test_panel("session-b", cx);
            workspace
        });
        vcx.run_until_parked();
        let first = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        let second = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(1))
            .unwrap();
        let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        first.update(vcx, |panel, cx| {
            panel.apply(&event, cx);
            let card = panel.ask_card.as_mut().unwrap();
            card.toggle(0, 1);
            card.comments[0][1] = "important note".into();
        });
        let bytes = first.read_with(vcx, |panel, cx| {
            serde_json::to_vec(&panel.snapshot(cx)).unwrap()
        });
        let snapshot: PanelSnapshot = serde_json::from_slice(&bytes).unwrap();
        second.update(vcx, |panel, cx| {
            panel.restore_snapshot(snapshot, cx);
            panel.apply(&event, cx);
        });
        second.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(card.comments[0][1], "important note");
            assert_eq!(
                card.comment_inputs[0][1].read(cx).snapshot().content,
                "important note"
            );
        });
        second.update(vcx, |panel, cx| panel.apply(&event, cx));
        second.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(card.comments[0][1], "important note");
        });
    }

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

        panel.update(vcx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("valuable draft".into(), cx)
            });
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input_focus_handle(cx);
            window.focus(&focus, cx);
        });
        let event: jcode_sdk::ApiEvent =
            serde_json::from_str(REAL_ASK_REQUEST).expect("real ask_request frame parses");
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();

        assert!(vcx.debug_bounds("ask-card").is_some(), "ask card paints");
        assert!(
            vcx.debug_bounds("composer").is_none(),
            "composer is hidden during ask"
        );
        assert!(
            vcx.debug_bounds("ask-scroll").is_some(),
            "questions have their own scroll area"
        );
        assert!(
            vcx.debug_bounds("ask-other-0").is_some(),
            "free answers are always offered"
        );
        for selector in ["ask-option-0-0", "ask-option-0-1", "ask-option-0-2"] {
            assert!(vcx.debug_bounds(selector).is_some(), "{selector} paints");
        }
        assert!(vcx.debug_bounds("ask-decline").is_some());
        vcx.simulate_keystrokes("down space");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![1]);
            assert_eq!(panel.input.read(cx).snapshot().content, "valuable draft");
        });

        let sqlite = vcx.debug_bounds("ask-option-0-0").unwrap();
        vcx.simulate_click(sqlite.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().expect("card still open");
            assert_eq!(card.chosen[0], vec![0]);
            let answers = card.response().answers.unwrap();
            assert_eq!(answers[0].selected, vec!["Postgres"]);
        });

        vcx.simulate_keystrokes("down space");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![1])
        });
        vcx.simulate_keystrokes("n");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-comment-0-1").is_some());
        vcx.simulate_input("fast local development");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
            assert_eq!(
                answer.comments.get("SQLite").map(String::as_str),
                Some("fast local development")
            );
        });
        let other = vcx.debug_bounds("ask-other-editor-0").unwrap();
        vcx.simulate_click(other.center(), gpui::Modifiers::default());
        vcx.simulate_input("my own answer");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
            assert_eq!(answer.other.as_deref(), Some("my own answer"));
            assert!(answer.selected.is_empty());
        });
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        let request_id = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().request_id.clone()
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id,
                    error: None,
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-card").is_none(),
            "card closes only after server acknowledgment"
        );
        panel.read_with(vcx, |panel, cx| {
            assert!(panel.ask_card.is_none());
            assert_eq!(panel.input.read(cx).snapshot().content, "valuable draft");
        });
        assert!(
            vcx.debug_bounds("composer").is_some(),
            "composer returns after answering"
        );
    }
}
