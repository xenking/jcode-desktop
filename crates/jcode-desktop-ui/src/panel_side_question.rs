//! `/btw` asks a side question about the conversation the user is looking at.
//!
//! The question runs in a hidden fork of the current session: it inherits the
//! transcript as context, so the answer is about this conversation, but the
//! source session is never touched. That keeps the parent's prompt cache and
//! transcript intact. The answer streams into a compact card above the
//! composer; "Open as chat" promotes the answer to a real chat and "Dismiss"
//! hides it. The fork itself stays archived on disk, so nothing is lost.
use super::*;
use jcode_sdk::{ApiEvent, SessionInfo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SideQuestionStatus {
    Working,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SideQuestionCard {
    /// The hidden fork that answers this question.
    pub child_session_id: String,
    pub question: String,
    pub answer: String,
    pub reasoning: String,
    pub status: SideQuestionStatus,
    /// Kept so the answer can be promoted without another session list read.
    pub session: SessionInfo,
    pub identity: Option<ApiEvent>,
}

impl SideQuestionCard {
    fn new(session: SessionInfo, question: String) -> Self {
        Self {
            child_session_id: session.session_id.clone(),
            question,
            answer: String::new(),
            reasoning: String::new(),
            status: SideQuestionStatus::Working,
            session,
            identity: None,
        }
    }

    pub(crate) fn is_working(&self) -> bool {
        self.status == SideQuestionStatus::Working
    }
}

impl Panel {
    pub(crate) fn start_side_question(
        &mut self,
        session: SessionInfo,
        question: String,
        cx: &mut Context<Self>,
    ) {
        // One side question at a time: the previous card is gone, so its fork
        // connection must not stay attached and stream into nothing.
        if let Some(previous) = self.side_question.take() {
            self.bridge.send(Command::DismissSideQuestion {
                session_id: previous.child_session_id,
            });
        }
        self.side_question = Some(SideQuestionCard::new(session, question));
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        cx.notify();
    }

    pub(crate) fn side_question_child(&self) -> Option<&str> {
        self.side_question
            .as_ref()
            .map(|card| card.child_session_id.as_str())
    }

    /// The open `/btw` card, if any.
    pub(crate) fn side_question_card_opt(&self) -> Option<&SideQuestionCard> {
        self.side_question.as_ref()
    }

    /// True when the event belonged to this panel's side question.
    pub(crate) fn apply_side_question_event(
        &mut self,
        event: &ApiEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(card) = self.side_question.as_mut() else {
            return false;
        };
        let mut changed = false;
        match event {
            ApiEvent::ModelInfo { model: Some(_), .. }
            | ApiEvent::RuntimeInfo { model: Some(_), .. } => {
                card.identity = Some(event.clone());
            }
            ApiEvent::TextDelta { text, .. }
                if card.is_working() => {
                    card.answer.push_str(text);
                    changed = true;
                }
            ApiEvent::ReasoningDelta { text, .. }
                if card.is_working() => {
                    card.reasoning.push_str(text);
                    changed = true;
                }
            ApiEvent::TurnStopped { message, .. }
                if card.is_working() => {
                    if !message.trim().is_empty() {
                        if !card.answer.trim().is_empty() {
                            card.answer.push_str("\n\n");
                        }
                        card.answer.push_str(message);
                    }
                    card.status = SideQuestionStatus::Failed;
                    changed = true;
                }
            ApiEvent::Error { message, .. }
                if card.is_working() => {
                    if !card.answer.trim().is_empty() {
                        card.answer.push_str("\n\n");
                    }
                    card.answer.push_str(message);
                    card.status = SideQuestionStatus::Failed;
                    changed = true;
                }
            ApiEvent::TurnDone { .. }
                if card.is_working() => {
                    card.status = SideQuestionStatus::Done;
                    changed = true;
                }
            _ => {}
        }
        if changed {
            cx.notify();
        }
        changed
    }

    /// Take the card so the workspace can promote it to a real chat.
    pub(crate) fn take_side_question(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<SideQuestionCard> {
        let card = self.side_question.take();
        if card.is_some() {
            cx.notify();
        }
        card
    }

    pub(crate) fn dismiss_side_question(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let card = self.take_side_question(cx)?;
        Some(card.child_session_id)
    }

    pub(super) fn render_side_question_card(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let card = self.side_question.as_ref()?;
        let theme = Theme::global();
        let status = match card.status {
            SideQuestionStatus::Working => "thinking…",
            SideQuestionStatus::Done => "done",
            SideQuestionStatus::Failed => "failed",
        };
        let mut body = div()
            .id("side-question-card")
            .debug_selector(|| "side-question-card".into())
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
                    .flex()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.ACCENT)
                            .child("Side question"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.TEXT_DIM)
                            .child(status),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .text_color(theme.TEXT_DIM)
                    .child(card.question.clone()),
            );
        if !card.answer.trim().is_empty() {
            body = body.child(div().min_w_0().child(card.answer.clone()));
        } else if card.is_working() {
            body = body.child(
                div()
                    .text_color(theme.TEXT_FAINT)
                    .child("Reading the conversation above…"),
            );
        }
        Some(
            body.child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        div()
                            .id("side-question-dismiss")
                            .debug_selector(|| "side-question-dismiss".into())
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.QUOTE_BG))
                            .child("Dismiss")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(child) = this.dismiss_side_question(cx) {
                                    this.bridge
                                        .send(Command::DismissSideQuestion { session_id: child });
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("side-question-promote")
                            .debug_selector(|| "side-question-promote".into())
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .bg(theme.ACCENT_DIM)
                            .text_color(theme.TEXT)
                            .child("Open as chat")
                            .children(crate::fixture_control_targets::marker(
                                || "side-question-promote".into(),
                                || Some(self.session_id.clone()),
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let Some(session) = this
                                    .side_question_card_opt()
                                    .map(|card| card.session.clone())
                                else {
                                    return;
                                };
                                window.dispatch_action(
                                    Box::new(crate::ui_actions::PromoteSideQuestion {
                                        source: cx.entity().entity_id(),
                                        session,
                                    }),
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element(),
        )
    }
}
