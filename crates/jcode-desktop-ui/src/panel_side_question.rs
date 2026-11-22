//! `/btw` asks a side question about the conversation the user is looking at.
//!
//! The question runs in a hidden fork of the current session: it inherits the
//! transcript as context, so the answer is about this conversation, but the
//! source session is never touched. That keeps the parent's prompt cache and
//! transcript intact. The card can continue the same fork or open it as a
//! full chat. Dismiss leaves the fork on disk for `/btw` history.
use super::ask::{CardActionTooltip, embedded_reply_frame};
use super::*;
use crate::harness::{SideQuestionThread, SideQuestionTopic, SideQuestionTurn};
use crate::input::VerticalResize;
use jcode_sdk::{ApiEvent, SessionInfo};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

pub(super) enum HistoryView {
    Loading(u64),
    Ready(Vec<SideQuestionTopic>),
    Opening(u64),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SideQuestionStatus {
    Working,
    Done,
    Failed,
}

pub(crate) struct SideQuestionCard {
    /// The hidden fork that answers this question.
    pub child_session_id: String,
    pub question: String,
    pub answer: String,
    pub reasoning: String,
    pub status: SideQuestionStatus,
    previous: Vec<SideQuestionTurn>,
    editor: Entity<PromptInput>,
    scroll: gpui::ScrollHandle,
    reveal_question: Rc<Cell<bool>>,
    resize: VerticalResize,
    selection: Entity<TextSelection>,
    error: Option<String>,
    /// Kept so the answer can be promoted without another session list read.
    pub session: SessionInfo,
    pub identity: Option<ApiEvent>,
}

impl SideQuestionCard {
    fn new(
        session: SessionInfo,
        question: String,
        editor: Entity<PromptInput>,
        cx: &mut Context<Panel>,
    ) -> Self {
        Self {
            child_session_id: session.session_id.clone(),
            question,
            answer: String::new(),
            reasoning: String::new(),
            status: SideQuestionStatus::Working,
            previous: Vec::new(),
            editor,
            scroll: gpui::ScrollHandle::new(),
            reveal_question: Rc::default(),
            resize: VerticalResize::default(),
            selection: cx.new(TextSelection::new),
            error: None,
            session,
            identity: None,
        }
    }

    pub(crate) fn is_working(&self) -> bool {
        self.status == SideQuestionStatus::Working
    }

    fn model_and_effort(&self) -> (Option<&str>, Option<&str>) {
        match &self.identity {
            Some(
                ApiEvent::ModelInfo {
                    model,
                    reasoning_effort,
                    ..
                }
                | ApiEvent::RuntimeInfo {
                    model,
                    reasoning_effort,
                    ..
                },
            ) => (model.as_deref(), reasoning_effort.as_deref()),
            _ => self
                .session
                .context
                .as_ref()
                .map_or((None, None), |context| {
                    (
                        context.model.as_deref(),
                        context.reasoning_effort.as_deref(),
                    )
                }),
        }
    }
}

impl Panel {
    fn return_side_question_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.input.read(cx).focus_handle.clone();
        window.defer(cx, move |window, cx| window.focus(&focus, cx));
    }

    pub(crate) fn side_question_control_state(&self) -> serde_json::Value {
        let history = match &self.side_question_history {
            Some(HistoryView::Ready(topics)) => {
                serde_json::json!(topics.iter().map(|topic| {
                serde_json::json!({"session_id":topic.session_id,"question":topic.question})
            }).collect::<Vec<_>>())
            }
            Some(HistoryView::Failed(error)) => serde_json::json!({"error":error}),
            Some(HistoryView::Loading(_) | HistoryView::Opening(_)) => serde_json::json!("loading"),
            None => serde_json::Value::Null,
        };
        serde_json::json!({"history":history,"card":self.side_question.as_ref().map(|card| {
            let mut turns = card.previous.iter().map(|turn| serde_json::json!({"question":turn.question,"answer":turn.answer})).collect::<Vec<_>>();
            turns.push(serde_json::json!({"question":card.question,"answer":card.answer}));
            let (model, effort) = card.model_and_effort();
            serde_json::json!({"session_id":card.child_session_id,"working":card.is_working(),"turns":turns,
                "model":model,"reasoning_effort":effort})
        })})
    }

    pub(crate) fn side_question_test_editor(&self) -> Option<Entity<PromptInput>> {
        self.side_question.as_ref().map(|card| card.editor.clone())
    }

    fn create_side_question_editor(cx: &mut Context<Self>) -> Entity<PromptInput> {
        let panel = cx.weak_entity();
        cx.new(|cx| {
            PromptInput::new(
                cx,
                "Follow-up… (Enter to send)",
                move |question, _, _, cx| {
                    let _ =
                        panel.update(cx, |panel, cx| panel.continue_side_question(question, cx));
                },
            )
            .without_command_completion()
            .text_only()
            .without_chrome()
            .with_compact_padding()
        })
    }

    fn continue_side_question(&mut self, question: String, cx: &mut Context<Self>) {
        let question = question.trim();
        let Some(card) = self
            .side_question
            .as_mut()
            .filter(|card| !card.is_working())
        else {
            return;
        };
        if question.is_empty() || self.side_question_history.is_some() {
            return;
        }
        card.previous.push(SideQuestionTurn {
            question: std::mem::replace(&mut card.question, question.to_string()),
            answer: std::mem::take(&mut card.answer),
        });
        card.reasoning.clear();
        card.error = None;
        card.status = SideQuestionStatus::Working;
        card.reveal_question.set(true);
        let editor = card.editor.clone();
        cx.defer(move |cx| {
            editor.update(cx, |editor, cx| editor.set_submission_enabled(false, cx))
        });
        self.bridge.send(Command::ContinueSideQuestion {
            session_id: card.child_session_id.clone(),
            question: question.to_string(),
        });
        cx.notify();
    }

    pub(crate) fn open_side_question_history(&mut self, cx: &mut Context<Self>) {
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        if self.can_fork() {
            self.side_question_history = Some(HistoryView::Loading(request_id));
            self.bridge.send(Command::ListSideQuestions {
                parent_session_id: self.session_id.clone(),
                request_id,
            });
        } else {
            self.side_question_history = Some(HistoryView::Ready(Vec::new()));
        }
        cx.notify();
    }

    pub(crate) fn load_side_question_history(
        &mut self,
        request_id: u64,
        result: Result<Vec<SideQuestionTopic>, String>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.side_question_history, Some(HistoryView::Loading(id)) if id == request_id)
        {
            self.side_question_history = Some(match result {
                Ok(topics) => HistoryView::Ready(topics),
                Err(error) => HistoryView::Failed(error),
            });
            cx.notify();
        }
    }

    fn open_saved_side_question(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !matches!(self.side_question_history, Some(HistoryView::Ready(_))) {
            return;
        }
        if let Some(card) = self
            .side_question
            .as_ref()
            .filter(|card| card.child_session_id == session_id)
        {
            card.reveal_question.set(true);
            self.side_question_history = None;
            cx.notify();
            return;
        }
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        self.side_question_history = Some(HistoryView::Opening(request_id));
        self.bridge.send(Command::OpenSideQuestion {
            parent_session_id: self.session_id.clone(),
            session_id: session_id.to_string(),
            request_id,
        });
        cx.notify();
    }

    pub(crate) fn load_saved_side_question(
        &mut self,
        request_id: u64,
        result: Result<SideQuestionThread, String>,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.side_question_history, Some(HistoryView::Opening(id)) if id == request_id)
        {
            return;
        }
        match result {
            Ok(SideQuestionThread {
                mut session,
                mut turns,
            }) => {
                let Some(last) = turns.pop() else {
                    self.side_question_history = Some(HistoryView::Failed(
                        "This topic has no saved question yet.".into(),
                    ));
                    cx.notify();
                    return;
                };
                if session.working_dir.is_none() {
                    session.working_dir.clone_from(&self.working_dir);
                }
                let mut card = SideQuestionCard::new(
                    session,
                    last.question,
                    Self::create_side_question_editor(cx),
                    cx,
                );
                card.previous = turns;
                card.answer = last.answer;
                card.status = if card.answer.is_empty() {
                    SideQuestionStatus::Failed
                } else {
                    SideQuestionStatus::Done
                };
                card.reveal_question.set(!card.previous.is_empty());
                self.show_side_question(card, cx);
            }
            Err(error) => {
                self.side_question_history = Some(HistoryView::Failed(error));
                cx.notify();
            }
        }
    }

    pub(crate) fn start_side_question(
        &mut self,
        session: SessionInfo,
        question: String,
        cx: &mut Context<Self>,
    ) {
        let editor = Self::create_side_question_editor(cx);
        editor.update(cx, |editor, cx| editor.set_submission_enabled(false, cx));
        let card = SideQuestionCard::new(session, question, editor, cx);
        self.show_side_question(card, cx);
    }

    fn show_side_question(&mut self, card: SideQuestionCard, cx: &mut Context<Self>) {
        if let Some(previous) = self.side_question.take() {
            self.bridge.send(Command::DismissSideQuestion {
                session_id: previous.child_session_id,
            });
        }
        self.side_question_history = None;
        self.side_question = Some(card);
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
        let was_working = card.is_working();
        let mut changed = false;
        match event {
            ApiEvent::ModelInfo { model: Some(_), .. }
            | ApiEvent::RuntimeInfo { model: Some(_), .. } => {
                changed = card.identity.as_ref() != Some(event);
                card.identity = Some(event.clone());
            }
            ApiEvent::TextDelta { text, .. } if card.is_working() => {
                card.answer.push_str(text);
                changed = true;
            }
            ApiEvent::ReasoningDelta { text, .. } if card.is_working() => {
                card.reasoning.push_str(text);
                changed = true;
            }
            ApiEvent::TurnStopped { message, .. } | ApiEvent::Error { message, .. }
                if card.is_working() =>
            {
                card.error = (!message.trim().is_empty()).then(|| message.clone());
                card.status = SideQuestionStatus::Failed;
                changed = true;
            }
            ApiEvent::TurnDone { .. } if card.is_working() => {
                card.status = SideQuestionStatus::Done;
                changed = true;
            }
            _ => {}
        }
        if changed {
            if was_working && !card.is_working() {
                card.editor
                    .update(cx, |editor, cx| editor.set_submission_enabled(true, cx));
            }
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if let Some(history) = &self.side_question_history {
            return Some(self.render_side_question_history(history, window, cx));
        }
        let card = self.side_question.as_ref()?;
        let theme = Theme::global();
        let (model, effort) = card.model_and_effort();
        let status = match card.status {
            SideQuestionStatus::Working => "Thinking…",
            SideQuestionStatus::Done => "Done",
            SideQuestionStatus::Failed => "Failed",
        };
        let height = window.viewport_size().height;
        let default_height = px((f32::from(height) * 0.32).clamp(56., 220.));
        // Leave room for the follow-up and the main composer when docked.
        let height_bounds = default_height..=(height * 0.55).max(default_height);
        let header =
            div()
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.ACCENT)
                                .child("Side question"),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.TEXT_DIM)
                                .child(status),
                        )
                        .children(model.map(|model| {
                            div().min_w_0().truncate().text_color(theme.TEXT_DIM).child(
                                match effort {
                                    Some(effort) => format!("{model} · {effort}"),
                                    None => model.to_string(),
                                },
                            )
                        })),
                )
                .child(
                    self.side_icon(SideAction::History, true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_side_question_history(cx);
                            this.return_side_question_focus(window, cx);
                        })),
                )
                .child(
                    self.side_icon(SideAction::Copy, !card.answer.trim().is_empty())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(card) = &this.side_question
                                && !card.answer.trim().is_empty()
                            {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    card.answer.clone(),
                                ));
                            }
                        })),
                )
                .child(
                    self.side_icon(SideAction::Promote, true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(card) = &this.side_question {
                                window.dispatch_action(
                                    Box::new(crate::ui_actions::PromoteSideQuestion {
                                        source: cx.entity().entity_id(),
                                        session: card.session.clone(),
                                    }),
                                    cx,
                                );
                            }
                        })),
                )
                .child(
                    self.side_icon(SideAction::Dismiss, true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(child) = this.dismiss_side_question(cx) {
                                this.bridge
                                    .send(Command::DismissSideQuestion { session_id: child });
                                this.return_side_question_focus(window, cx);
                            }
                        })),
                );
        let turns = div()
            .id("side-question-turns")
            .debug_selector(|| "side-question-turns".into())
            .max_h(card.resize.height(height_bounds.clone()))
            .flex()
            .flex_col()
            .min_w_0()
            .gap_3()
            .pr_2()
            .children(card.previous.iter().enumerate().map(|(index, turn)| {
                side_turn(&turn.question, &turn.answer, index, false, card, window, cx)
            }))
            .child(side_turn(
                &card.question,
                &card.answer,
                card.previous.len(),
                true,
                card,
                window,
                cx,
            ));
        let reveal = card.reveal_question.clone();
        let scroll = card.scroll.clone();
        let latest = card.previous.len();
        let entity = cx.entity().entity_id();

        Some(
            side_card("side-question-card")
                .child(card.resize.handle(
                    "side-question-resize",
                    "side-question-resize",
                    &card.scroll,
                    height_bounds,
                    cx,
                ))
                .child(header)
                .child(
                    div()
                        .relative()
                        .child(crate::transcript_scroll::tracked_vertical(
                            turns,
                            &card.scroll,
                        ))
                        .child(crate::scrollbar::vertical(
                            &card.scroll,
                            "side-question-scrollbar",
                        ))
                        .when(reveal.get(), |el| {
                            // GPUI's scroll-to-item needs a measured viewport. Run it once
                            // after the scroll surface has prepainted, not on answer deltas.
                            el.child(
                                gpui::canvas(
                                    |_, _, _| (),
                                    move |_, _, _, cx| {
                                        if reveal.replace(false) {
                                            scroll.scroll_to_top_of_item(latest);
                                            cx.defer(move |cx| cx.notify(entity));
                                        }
                                    },
                                )
                                .absolute()
                                .size_full(),
                            )
                        }),
                )
                .child(
                    div()
                        .id("side-question-follow-up")
                        .debug_selector(|| "side-question-follow-up".into())
                        .relative()
                        .flex()
                        .children(crate::fixture_control_targets::marker(
                            || "side-question-follow-up".into(),
                            || Some(self.session_id.clone()),
                        ))
                        .child(embedded_reply_frame(&card.editor, false, window, cx)),
                )
                .into_any_element(),
        )
    }

    fn side_icon(&self, action: SideAction, enabled: bool) -> gpui::Stateful<gpui::Div> {
        let (id, title, icon) = action.presentation();
        let theme = Theme::global();
        let button = div()
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .flex_none()
            .size(px(23.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .tooltip(move |_, cx| cx.new(|_| CardActionTooltip(title)).into())
            .children(crate::fixture_control_targets::marker(
                || id.into(),
                || Some(self.session_id.clone()),
            ))
            .child(gpui::svg().data(icon).size(px(14.)).text_color(if enabled {
                theme.TEXT_DIM
            } else {
                theme.TEXT_FAINT
            }));
        if enabled {
            crate::keyboard::button(button)
                .role(gpui::Role::Button)
                .aria_label(title)
                .cursor_pointer()
                .hover(|el| el.bg(theme.QUOTE_BG))
        } else {
            button
        }
    }

    fn render_side_question_history(
        &self,
        history: &HistoryView,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let content = match history {
            HistoryView::Loading(_) => div().child("Loading side questions…").into_any_element(),
            HistoryView::Opening(_) => div().child("Opening side question…").into_any_element(),
            HistoryView::Failed(error) => div()
                .text_color(theme.ERROR)
                .child(error.clone())
                .into_any_element(),
            HistoryView::Ready(topics) if topics.is_empty() => div()
                .child("No side questions yet. Type /btw followed by a question to start one.")
                .into_any_element(),
            HistoryView::Ready(topics) => crate::transcript_scroll::vertical(
                div()
                    .id("side-question-topics")
                    .max_h(window.viewport_size().height * 0.4)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(topics.iter().enumerate().map(|(index, topic)| {
                        let session_id = topic.session_id.clone();
                        crate::keyboard::button(
                            div()
                                .id(("side-question-topic", index))
                                .debug_selector(move || format!("side-question-topic-{index}"))
                                .relative()
                                .children(crate::fixture_control_targets::marker(
                                    || format!("side-question-topic-{index}"),
                                    || Some(self.session_id.clone()),
                                ))
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(theme.QUOTE_BG)
                                .hover(|el| el.bg(theme.ACCENT_DIM))
                                .role(gpui::Role::Button)
                                .aria_label(topic.question.clone())
                                .child(div().min_w_0().truncate().child(topic.question.clone())),
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_saved_side_question(&session_id, cx);
                                this.return_side_question_focus(window, cx);
                            },
                        ))
                    })),
            )
            .into_any_element(),
        };
        side_card("side-question-history")
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_color(theme.ACCENT)
                            .child("Side question history"),
                    )
                    .child(
                        self.side_icon(SideAction::CloseHistory, true)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.side_question_history = None;
                                this.return_side_question_focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(content)
            .into_any_element()
    }
}

fn side_card(id: &'static str) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .flex_none()
        .flex()
        .flex_col()
        .min_w_0()
        .gap_1()
        .mx_2()
        .px_2()
        .pb_2()
        .when(id == "side-question-history", |el| el.pt_2())
        .rounded_xl()
        .border_1()
        .border_color(theme.INPUT_BORDER)
        .bg(theme.TOOL_BG)
        .text_size(px(14.))
        .text_color(theme.TEXT)
}

fn side_turn(
    question: &str,
    answer: &str,
    index: usize,
    current: bool,
    card: &SideQuestionCard,
    window: &Window,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    let error = current.then_some(card.error.as_ref()).flatten();
    div()
        .id(("side-question-turn", index))
        .w_full()
        .min_w_0()
        .flex_none()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .when(current, |el| {
                    el.debug_selector(|| "side-question-current-prompt".into())
                })
                .min_w_0()
                .px_2()
                .py_1()
                .rounded_lg()
                .bg(theme.QUOTE_BG)
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.TEXT)
                .child(question.to_string()),
        )
        .when(!answer.trim().is_empty(), |el| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .font_family(theme.FONT_AI)
                    .child(markdown::render(answer, index, &card.selection, window, cx)),
            )
        })
        .when(answer.trim().is_empty() && error.is_none(), |el| {
            el.child(
                div()
                    .text_color(theme.TEXT_DIM)
                    .child(if current && card.is_working() {
                        "Waiting for an answer…"
                    } else {
                        "No answer saved."
                    }),
            )
        })
        .children(error.map(|message| div().text_color(theme.ERROR).child(message.clone())))
}

#[derive(Clone, Copy)]
enum SideAction {
    History,
    Copy,
    Promote,
    Dismiss,
    CloseHistory,
}

impl SideAction {
    fn presentation(self) -> (&'static str, &'static str, &'static [u8]) {
        match self {
            Self::History => (
                "side-question-history-open",
                "History",
                include_bytes!("../../../assets/icons/history.svg"),
            ),
            Self::Copy => (
                "side-question-copy",
                "Copy answer",
                include_bytes!("../../../assets/icons/copy.svg"),
            ),
            Self::Promote => (
                "side-question-promote",
                "Open in sidechat",
                include_bytes!("../../../assets/icons/arrow-right.svg"),
            ),
            Self::Dismiss => (
                "side-question-dismiss",
                "Dismiss",
                include_bytes!("../../../assets/icons/close.svg"),
            ),
            Self::CloseHistory => (
                "side-question-history-close",
                "Close history",
                include_bytes!("../../../assets/icons/close.svg"),
            ),
        }
    }
}
