//! Inline card for the `ask` tool: the agent blocks on the user's answers.
//!
// Each question owns its free-answer editor and each selected option its comment.
use super::chrome::{CardActionTooltip, embedded_reply_frame};
use super::*;
use crate::choice_keys::{
    self, ChoiceKey, Chord, MAX_NUMBERED, Scope, expandable, first_line, shown_label,
};
use gpui::{FontWeight, StyledText};
use jcode_sdk::{AskAnswer, AskQuestion, AskResponse};
use std::{cell::Cell, rc::Rc};

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
    #[serde(default)]
    other_inputs: Vec<PromptInputSnapshot>,
    #[serde(default)]
    comment_inputs: Vec<Vec<PromptInputSnapshot>>,
}

impl AskDraft {
    pub(super) fn matches_questions(&self, questions: &[AskQuestion]) -> bool {
        self.questions == questions
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AskCard {
    pub request_id: String,
    pub tool_call_id: String,
    pub questions: Vec<AskQuestion>,
    pub chosen: Vec<Vec<usize>>,
    pub other: Vec<Option<String>>,
    pub comments: Vec<Vec<String>>,
    pub other_inputs: Vec<Entity<PromptInput>>,
    pub comment_inputs: Vec<Vec<Entity<PromptInput>>>,
    pub cursor_question: usize,
    pub cursor_option: usize,
    comment_open: Vec<Vec<bool>>,
    pub(super) focus_scope: Option<gpui::FocusHandle>,
    tab_focus: Vec<gpui::FocusHandle>,
    option_focus: Vec<Vec<gpui::FocusHandle>>,
    confirming_decline: bool,
    pub voice_target: Option<(usize, Option<usize>)>,
    attempt_id: Option<u64>,
    pub submit_error: Option<String>,
    scroll: gpui::ScrollHandle,
    /// Options whose full description is open, as (question, option).
    expanded: HashSet<(usize, usize)>,
    /// Where the open question's options last painted, to keep the cursor row in view.
    option_bounds: std::rc::Rc<std::cell::RefCell<HashMap<usize, gpui::Bounds<gpui::Pixels>>>>,
    /// What each option label last painted, so tests can compare the wrapped
    /// lines with the box the layout gave them.
    #[cfg(test)]
    label_layouts: LabelLayouts,
}

/// Test access to what each option label painted.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct LabelLayouts(
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<(usize, usize), gpui::TextLayout>>>,
);

#[cfg(test)]
impl std::fmt::Debug for LabelLayouts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LabelLayouts")
    }
}

impl AskCard {
    /// The editor dictation should land in while this card is waiting.
    pub(super) fn voice_input(&self) -> Option<Entity<PromptInput>> {
        let (q, option) = self.voice_target?;
        match option {
            Some(o) => self.comment_inputs.get(q)?.get(o).cloned(),
            None => self.other_inputs.get(q).cloned(),
        }
    }

    pub fn new(request_id: String, questions: Vec<AskQuestion>) -> Self {
        let n = questions.len();
        let mut card = Self {
            request_id,
            tool_call_id: String::new(),
            comments: questions
                .iter()
                .map(|q| vec![String::new(); q.options.len()])
                .collect(),
            comment_open: questions
                .iter()
                .map(|q| vec![false; q.options.len()])
                .collect(),
            questions,
            chosen: vec![Vec::new(); n],
            other: vec![None; n],
            other_inputs: Vec::new(),
            comment_inputs: Vec::new(),
            focus_scope: None,
            tab_focus: Vec::new(),
            option_focus: Vec::new(),
            cursor_question: 0,
            cursor_option: 0,
            confirming_decline: false,
            voice_target: (n > 0).then_some((0, None)),
            attempt_id: None,
            submit_error: None,
            scroll: gpui::ScrollHandle::new(),
            expanded: HashSet::new(),
            option_bounds: Default::default(),
            #[cfg(test)]
            label_layouts: Default::default(),
        };
        card.cursor_option = card.recommended_cursor(0);
        card
    }

    /// Where the cursor starts on question `q`: the recommended option, else the first.
    fn recommended_cursor(&self, q: usize) -> usize {
        self.questions
            .get(q)
            .and_then(|question| {
                crate::choice_keys::recommended_option(
                    question.options.iter().map(|option| option.label.as_str()),
                )
            })
            .unwrap_or(0)
    }

    pub fn draft(&self, cx: &App) -> AskDraft {
        AskDraft {
            request_id: self.request_id.clone(),
            questions: self.questions.clone(),
            chosen: self.chosen.clone(),
            other: self.other.clone(),
            comments: self.comments.clone(),
            other_inputs: self
                .other_inputs
                .iter()
                .map(|input| input.read(cx).snapshot())
                .collect(),
            comment_inputs: self
                .comment_inputs
                .iter()
                .map(|inputs| {
                    inputs
                        .iter()
                        .map(|input| input.read(cx).snapshot())
                        .collect()
                })
                .collect(),
        }
    }

    pub(super) fn submitting(&self) -> bool {
        self.attempt_id.is_some()
    }

    pub fn restore_draft(&mut self, draft: AskDraft, cx: &mut App) {
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
        for (q, comments) in self.comments.iter().enumerate() {
            for (o, comment) in comments.iter().enumerate() {
                self.comment_open[q][o] = !comment.is_empty();
            }
        }
        for (q, input) in self.other_inputs.iter().enumerate() {
            let text = self.other[q].clone().unwrap_or_default();
            if let Some(snapshot) = draft
                .other_inputs
                .get(q)
                .filter(|snapshot| snapshot.content.trim() == text.trim())
            {
                input.update(cx, |input, cx| input.restore(snapshot.clone(), cx));
            } else {
                input.update(cx, |input, cx| input.set_content(text, cx));
            }
        }
        for (q, inputs) in self.comment_inputs.iter().enumerate() {
            for (o, input) in inputs.iter().enumerate() {
                let text = self.comments[q][o].clone();
                if let Some(snapshot) = draft
                    .comment_inputs
                    .get(q)
                    .and_then(|inputs| inputs.get(o))
                    .filter(|snapshot| snapshot.content == text)
                {
                    input.update(cx, |input, cx| input.restore(snapshot.clone(), cx));
                } else {
                    input.update(cx, |input, cx| input.set_content(text, cx));
                }
            }
        }
    }

    pub(super) fn open_comment(&mut self, question: usize, option: usize) {
        self.comment_open[question][option] = true;
    }

    fn focused_editor(&self, window: &Window, cx: &App) -> Option<(usize, Option<usize>)> {
        let focused = |input: &Entity<PromptInput>| input.read(cx).is_focused(window);
        if let Some(q) = self.other_inputs.iter().position(focused) {
            return Some((q, None));
        }
        self.comment_inputs
            .iter()
            .enumerate()
            .find_map(|(q, inputs)| inputs.iter().position(focused).map(|o| (q, Some(o))))
    }

    pub fn toggle(&mut self, q: usize, option: usize) {
        if let Some(pos) = self.chosen[q].iter().position(|&i| i == option) {
            self.chosen[q].remove(pos);
        } else {
            self.select(q, option);
        }
    }

    fn toggle_expanded(&mut self, q: usize, option: usize) {
        if !self.expanded.remove(&(q, option)) {
            self.expanded.insert((q, option));
        }
    }

    /// Whether `option`'s description has more to show than its collapsed line.
    fn can_expand(&self, q: usize, option: usize) -> bool {
        self.questions
            .get(q)
            .and_then(|question| question.options.get(option))
            .and_then(|option| option.description.as_deref())
            .is_some_and(crate::choice_keys::expandable)
    }

    /// Move the cursor row and scroll it into the card's view.
    fn move_cursor(&mut self, option: usize) {
        self.cursor_option = option;
        let (Some(row), viewport) = (
            self.option_bounds.borrow().get(&option).copied(),
            self.scroll.bounds(),
        ) else {
            return;
        };
        let offset = self.scroll.offset();
        let y = crate::choice_keys::reveal_offset(
            (viewport.top(), viewport.bottom()),
            (row.top(), row.bottom()),
            offset.y,
            self.scroll.max_offset().y,
        );
        self.scroll.set_offset(gpui::point(offset.x, y));
    }

    fn select(&mut self, q: usize, option: usize) {
        let chosen = &mut self.chosen[q];
        if self.questions[q].multi_select {
            if chosen.contains(&option) {
                return;
            }
            chosen.push(option);
        } else {
            *chosen = vec![option];
            self.other[q] = None;
        }
    }

    fn set_comment(&mut self, q: usize, option: usize, text: &str) {
        self.comments[q][option] = text.to_owned();
        if !text.trim().is_empty() {
            self.select(q, option);
        }
    }

    /// Question tabs, plus a review tab once there is more than one answer
    /// to look over. A single question is answered and sent from its own tab.
    pub(super) fn tab_count(&self) -> usize {
        self.questions.len() + usize::from(self.questions.len() > 1)
    }

    pub(super) fn switch_tab(&mut self, q: usize) {
        let q = q.min(self.tab_count().saturating_sub(1));
        if self.cursor_question != q {
            self.cursor_question = q;
            self.cursor_option = self.recommended_cursor(q);
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.confirming_decline = false;
    }

    pub fn set_other(&mut self, q: usize, text: &str) {
        let text = text.trim();
        self.other[q] = (!text.is_empty()).then(|| text.to_string());
        if self.other[q].is_some() && !self.questions[q].multi_select {
            self.chosen[q].clear();
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

/// A question that arrived while another card was on screen.
#[derive(Clone, Debug)]
pub(super) struct QueuedAsk {
    request_id: String,
    tool_call_id: String,
    questions: Vec<AskQuestion>,
}

#[derive(Deserialize)]
struct AskToolInput {
    questions: Vec<AskQuestion>,
}

const ASK_CONNECTION_CHANGED: &str =
    "The connection changed. Your answer is retained here and was not resent.";

impl Panel {
    pub(super) fn show_live_ask(
        &mut self,
        request_id: &str,
        tool_call_id: &str,
        questions: &[AskQuestion],
        cx: &mut Context<Self>,
    ) {
        if self
            .unavailable_asks
            .get(request_id)
            .is_some_and(|id| *id != self.ask_attachment_id)
        {
            self.unavailable_asks.remove(request_id);
        }
        self.show_ask(request_id, tool_call_id, questions, cx);
    }

    pub(super) fn dismiss_stopped_asks(&mut self, cx: &mut Context<Self>) {
        self.ask_recovery = None;
        self.pending_ask_draft = None;
        self.answered_asks
            .extend(self.queued_asks.drain(..).map(|ask| ask.request_id));
        if self.ask_card.is_some() {
            self.cancel_ask_voice(cx);
            if let Some(card) = self.ask_card.take() {
                self.ask_focus_return = card.focus_scope;
                self.answered_asks.insert(card.request_id);
            }
        }
    }

    pub(super) fn show_ask(
        &mut self,
        request_id: &str,
        tool_call_id: &str,
        questions: &[AskQuestion],
        cx: &mut Context<Self>,
    ) {
        if self.answered_asks.contains(request_id) || self.unavailable_asks.contains_key(request_id)
        {
            return;
        }
        self.ask_recovery = None;
        if let Some(card) = self.ask_card.as_mut()
            && card.request_id == request_id
            && card.questions == questions
        {
            if !tool_call_id.is_empty() {
                tool_call_id.clone_into(&mut card.tool_call_id);
            }
            return;
        }
        // Several questions can wait at once, such as an `ask` and a
        // computer-use consent prompt, and a reconnect replays all of them.
        // Queue the others behind the card on screen instead of replacing it,
        // or the replaced question can never be answered.
        if let Some(card) = self.ask_card.as_ref()
            && card.request_id != request_id
            && (tool_call_id.is_empty() || card.tool_call_id != tool_call_id)
        {
            let queued = QueuedAsk {
                request_id: request_id.to_owned(),
                tool_call_id: tool_call_id.to_owned(),
                questions: questions.to_vec(),
            };
            match self
                .queued_asks
                .iter_mut()
                .find(|ask| ask.request_id == request_id)
            {
                Some(existing) => *existing = queued,
                None => self.queued_asks.push_back(queued),
            }
            cx.notify();
            return;
        }
        let mut card = AskCard::new(request_id.to_owned(), questions.to_vec());
        tool_call_id.clone_into(&mut card.tool_call_id);
        self.ask_card = Some(card);
        self.prepare_ask_inputs(cx);
        if self
            .pending_ask_draft
            .as_ref()
            .is_some_and(|draft| draft.matches_questions(questions))
            && let (Some(draft), Some(card)) =
                (self.pending_ask_draft.take(), self.ask_card.as_mut())
        {
            card.restore_draft(draft, cx);
        }
        self.stick_to_bottom = true;
        cx.notify();
    }

    fn recoverable_ask(&self) -> Option<(&str, Vec<AskQuestion>)> {
        let mut seen = HashSet::new();
        self.items.iter().rev().find_map(|item| {
            let Item::Tool {
                call_id,
                name,
                input,
                done,
                ..
            } = item
            else {
                return None;
            };
            // History and the event stream can both supply the same tool row.
            // Only the most recent state of that call can reopen a card.
            if name != "ask" || call_id.is_empty() || !seen.insert(call_id.as_str()) || *done {
                return None;
            }
            let request_id = format!("ask-{call_id}");
            if self.answered_asks.contains(&request_id) || self.unavailable_asks.contains_key(&request_id) {
                return None;
            }
            let AskToolInput { questions } = serde_json::from_str(input).ok()?;
            (!questions.is_empty()).then_some((call_id.as_str(), questions))
        })
    }

    pub(super) fn schedule_ask_recovery(&mut self, cx: &mut Context<Self>) {
        if self.ask_card.is_some() || self.ask_recovery.is_some() {
            return;
        }
        let Some((call_id, _)) = self.recoverable_ask() else {
            return;
        };
        let call_id = call_id.to_owned();
        let grace = cx.background_executor().timer(Duration::from_secs(1));
        self.ask_recovery = Some(cx.spawn(async move |panel, cx| {
            grace.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.ask_recovery = None;
                if panel.ask_card.is_some() {
                    return;
                }
                match panel.recoverable_ask() {
                    Some((current, questions)) if current == call_id => {
                        panel.show_ask(&format!("ask-{call_id}"), &call_id, &questions, cx);
                    }
                    Some(_) => panel.schedule_ask_recovery(cx),
                    None => {}
                }
            });
        }));
    }

    pub(super) fn ask_result_images(&self, call_id: &str) -> Vec<(usize, &TranscriptImage)> {
        self.items.iter().enumerate().filter_map(|(index, item)| match item {
            Item::Image(image)
                if matches!(&image.source, jcode_sdk::RenderedImageSource::ToolResult { tool_name } if tool_name == "ask")
                    && matches!(&image.anchor, Some(jcode_sdk::RenderedImageAnchor::ToolCall { id }) if id == call_id) => Some((index, image)),
            _ => None,
        }).collect()
    }

    pub(super) fn ask_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.ask_card
            .as_ref()
            .is_some_and(|card| card.focused_editor(window, cx).is_some())
    }

    pub(crate) fn capture_focused_ask_voice_target(&mut self, window: &Window, cx: &App) -> bool {
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        let Some(editor) = card.focused_editor(window, cx) else {
            return false;
        };
        card.voice_target = Some(editor);
        true
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
        crate::keyboard::button(div().id(("ask-voice", q * 16 + option.map_or(0, |o| o + 1))))
            .debug_selector(move || format!("ask-voice-{q}-{}", option.map_or(0, |o| o + 1)))
            .relative()
            .size(px(28.))
            .rounded_full()
            .cursor_pointer()
            .bg(if active { theme.ERROR } else { theme.TOOL_BG })
            .child({
                let icon = div()
                    .absolute()
                    .inset_0()
                    .when(active, |el| {
                        el.debug_selector(move || {
                            format!(
                                "ask-voice-recording-indicator-{q}-{}",
                                option.map_or(0, |o| o + 1)
                            )
                        })
                    })
                    .child(jcode_desktop_voice::chrome::voice_microphone_face(
                        if active {
                            crate::theme::to_hsla(theme.BG)
                        } else {
                            crate::theme::to_hsla(theme.TEXT_DIM)
                        },
                    ));
                if active {
                    gpui::AnimationExt::with_animation(
                        icon,
                        ("ask-voice-pulse", q * 16 + option.map_or(0, |o| o + 1)),
                        gpui::Animation::new(std::time::Duration::from_millis(1200))
                            .repeat()
                            .with_easing(gpui::pulsating_between(0.5, 1.0)),
                        |el, opacity| el.opacity(opacity),
                    )
                    .into_any_element()
                } else {
                    icon.into_any_element()
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if !this.voice_active()
                    && let Some(card) = this.ask_card.as_mut()
                {
                    card.voice_target = Some((q, option));
                }
                if let Some(input) = this.ask_voice_input() {
                    window.focus(&input.read(cx).editor_focus(), cx);
                }
                window.dispatch_action(Box::new(voice::ToggleVoice), cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn handle_ask_editor_submit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(card) = self.ask_card.as_ref() else {
            return false;
        };
        let Some((q, option)) = card.focused_editor(window, cx) else {
            return false;
        };
        // Enter in a comment must not advance or submit the Ask.
        if option.is_none() && q == card.cursor_question && card.other[q].is_some() {
            self.advance_ask(window, cx);
        }
        true
    }

    fn advance_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.ask_card.as_mut().filter(|card| !card.submitting()) else {
            return;
        };
        if card.cursor_question + 1 < card.tab_count() {
            card.switch_tab(card.cursor_question + 1);
            window.focus(&self.focus_handle, cx);
            cx.notify();
        } else {
            let response = card.response_with_images(cx);
            self.answer_ask(response, cx);
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
            let id = request_id.clone();
            other_inputs.push(cx.new(|cx| {
                PromptInput::embedded_editor(cx, "Your own answer…").with_on_change(
                    move |text, app| {
                        let _ = panel.update(app, |panel, cx| {
                            if let Some(card) =
                                panel.ask_card.as_mut().filter(|card| card.request_id == id)
                            {
                                card.set_other(q, text);
                                cx.notify();
                            }
                        });
                    },
                )
            }));
            let mut inputs = Vec::new();
            for o in 0..count {
                let panel = cx.weak_entity();
                let id = request_id.clone();
                inputs.push(cx.new(|cx| {
                    PromptInput::embedded_editor(cx, "Add a comment…").with_on_change(
                        move |text, app| {
                            let _ = panel.update(app, |panel, cx| {
                                if let Some(card) =
                                    panel.ask_card.as_mut().filter(|card| card.request_id == id)
                                {
                                    card.set_comment(q, o, text);
                                    cx.notify();
                                }
                            });
                        },
                    )
                }));
            }
            comment_inputs.push(inputs);
        }
        if let Some(card) = self.ask_card.as_mut() {
            card.other_inputs = other_inputs;
            card.comment_inputs = comment_inputs;
            card.focus_scope = Some(cx.focus_handle());
            card.tab_focus = (0..card.tab_count()).map(|_| cx.focus_handle()).collect();
            card.option_focus = card
                .questions
                .iter()
                .map(|q| (0..q.options.len()).map(|_| cx.focus_handle()).collect())
                .collect();
        }
    }

    pub(crate) fn ask_response_settled(
        &mut self,
        request_id: &str,
        attempt_id: u64,
        result: Result<(), crate::harness::AskResponseError>,
        cx: &mut Context<Self>,
    ) {
        use crate::harness::AskResponseError;
        let Some(card) = self
            .ask_card
            .as_mut()
            .filter(|card| card.request_id == request_id && card.attempt_id == Some(attempt_id))
        else {
            return;
        };
        match result {
            Ok(()) => {
                self.answered_asks.insert(request_id.to_owned());
                self.close_ask_card(cx);
            }
            Err(AskResponseError::Stale) => {
                self.unavailable_asks
                    .insert(request_id.to_owned(), self.ask_attachment_id);
                self.items.push(super::Item::Error(
                    "This question is no longer pending. Your answer was not accepted.".into(),
                ));
                self.close_ask_card(cx);
            }
            Err(AskResponseError::ConnectionChanged) => {
                card.attempt_id = None;
                card.submit_error = Some(ASK_CONNECTION_CHANGED.into());
            }
            Err(AskResponseError::Failed(error)) => {
                card.attempt_id = None;
                card.submit_error = Some(error);
            }
        }
        cx.notify();
    }

    fn close_ask_card(&mut self, cx: &mut Context<Self>) {
        self.cancel_ask_voice(cx);
        self.ask_focus_return = self.ask_card.take().and_then(|card| card.focus_scope);
        self.pending_ask_draft = None;
        self.show_next_queued_ask(cx);
        self.schedule_ask_recovery(cx);
    }

    pub(crate) fn reconcile_asks(
        &mut self,
        attachment_id: u64,
        pending: Option<&[String]>,
        cx: &mut Context<Self>,
    ) {
        if attachment_id <= self.ask_attachment_id {
            return;
        }
        let previous_attachment = std::mem::replace(&mut self.ask_attachment_id, attachment_id);
        if let Some(card) = self.ask_card.as_mut()
            && card.attempt_id.take().is_some()
        {
            card.submit_error = Some(ASK_CONNECTION_CHANGED.into());
        }
        if let Some(pending) = pending {
            self.ask_recovery = None;
            self.unavailable_asks.retain(|id, _| !pending.contains(id));
            for item in &self.items {
                if let super::Item::Tool {
                    name, call_id, done: false, ..
                } = item
                    && name == "ask"
                    && !call_id.is_empty()
                {
                    let request_id = format!("ask-{call_id}");
                    if !pending.contains(&request_id) {
                        self.unavailable_asks
                            .entry(request_id)
                            .or_insert(previous_attachment);
                    }
                }
            }
            self.queued_asks.retain(|ask| {
                let still_pending = pending.contains(&ask.request_id);
                if !still_pending {
                    self.unavailable_asks
                        .insert(ask.request_id.clone(), previous_attachment);
                }
                still_pending
            });
            if let Some(card) = self
                .ask_card
                .as_ref()
                .filter(|card| !pending.contains(&card.request_id))
            {
                self.unavailable_asks
                    .insert(card.request_id.clone(), previous_attachment);
                self.items.push(super::Item::Error(
                    "This question is no longer pending. No answer was sent on reconnect.".into(),
                ));
                self.close_ask_card(cx);
            }
            self.schedule_ask_recovery(cx);
        }
        cx.notify();
    }

    /// Retire a queued question whose tool finished without it being shown.
    pub(super) fn forget_queued_ask(&mut self, call_id: &str) {
        self.queued_asks.retain(|ask| {
            ask.tool_call_id != call_id && ask.request_id.strip_prefix("ask-") != Some(call_id)
        });
    }

    /// Show the oldest question still waiting once the card is free.
    pub(super) fn show_next_queued_ask(&mut self, cx: &mut Context<Self>) {
        while self.ask_card.is_none() {
            let Some(next) = self.queued_asks.pop_front() else {
                return;
            };
            self.show_ask(&next.request_id, &next.tool_call_id, &next.questions, cx);
        }
    }
}

// Keys: one map for every list answered from the keyboard (`choice_keys`).

impl Panel {
    pub(super) fn handle_ask_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(card) = self.ask_card.as_ref().filter(|card| !card.submitting()) else {
            return false;
        };
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let chord = Chord {
            key,
            shift: modifiers.shift,
            other_modifier: modifiers.control
                || modifiers.alt
                || modifiers.platform
                || modifiers.function,
        };
        let editing = card.focused_editor(window, cx).is_some();
        let q = card.cursor_question;
        let count = card
            .questions
            .get(q)
            .map_or(0, |question| question.options.len());
        if editing {
            return match choice_keys::classify(chord, Scope::Text, count) {
                Some(ChoiceKey::Dismiss | ChoiceKey::SwitchField) => {
                    window.focus(&self.focus_handle, cx);
                    cx.notify();
                    true
                }
                Some(ChoiceKey::Confirm) => self.handle_ask_editor_submit(window, cx),
                _ => false,
            };
        }
        if chord.other_modifier
            || (!self.focus_handle.is_focused(window)
                && !self
                    .transcript_selection
                    .read(cx)
                    .focus_handle()
                    .is_focused(window))
        {
            return false;
        }
        let action = choice_keys::classify(chord, Scope::List, count);
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        if card.confirming_decline
            && key != "tab"
            && !matches!(action, Some(ChoiceKey::Dismiss | ChoiceKey::Confirm))
        {
            return true;
        }
        let at = card.cursor_option.min(count.saturating_sub(1));
        match action {
            Some(ChoiceKey::Dismiss) => card.confirming_decline = !card.confirming_decline,
            Some(ChoiceKey::Confirm) if card.confirming_decline => {
                self.answer_ask(AskResponse::default(), cx);
            }
            Some(ChoiceKey::Confirm) => {
                self.advance_ask(window, cx);
                return true;
            }
            Some(ChoiceKey::PrevGroup) => {
                card.switch_tab((q + card.tab_count() - 1) % card.tab_count());
            }
            Some(ChoiceKey::NextGroup) => card.switch_tab((q + 1) % card.tab_count()),
            Some(ChoiceKey::Prev) if count > 0 => {
                card.move_cursor(choice_keys::step(Scope::List, at, -1, count))
            }
            Some(ChoiceKey::Next) if count > 0 => {
                card.move_cursor(choice_keys::step(Scope::List, at, 1, count))
            }
            Some(ChoiceKey::First) if count > 0 => card.move_cursor(0),
            Some(ChoiceKey::Last) if count > 0 => card.move_cursor(count - 1),
            Some(ChoiceKey::Toggle) if count > 0 => card.toggle(q, at),
            Some(ChoiceKey::Pick(option)) => {
                card.move_cursor(option);
                if card.questions[q].multi_select {
                    card.toggle(q, option);
                } else {
                    card.select(q, option);
                }
            }
            Some(ChoiceKey::SwitchField) => {
                let Some(input) = card.other_inputs.get(q) else {
                    return false;
                };
                window.focus(&input.read(cx).editor_focus(), cx);
            }
            None if key == "n" && count > 0 => {
                card.open_comment(q, at);
                if let Some(input) = card.comment_inputs.get(q).and_then(|inputs| inputs.get(at)) {
                    let focus = input.read(cx).editor_focus();
                    window.focus(&focus, cx);
                }
            }
            None if key == "e" && !chord.shift && card.can_expand(q, at) => {
                card.toggle_expanded(q, at)
            }
            _ => return false,
        }
        cx.notify();
        true
    }
}

// Submission: the only code that builds and sends the wire response, so a
// change to the answer wire touches this section only.
impl AskCard {
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
                    other_images: Vec::new(),
                    comment_images: Default::default(),
                }
            })
            .collect();
        AskResponse {
            answers: Some(answers),
        }
    }

    /// Snapshot only the active answers, in marker order, immediately before
    /// dispatch. Images belonging to deselected choices are never sent.
    pub(super) fn response_with_images(&self, cx: &App) -> AskResponse {
        fn images(input: &Entity<PromptInput>, cx: &App) -> Vec<(String, String)> {
            let snapshot = input.read(cx).snapshot();
            let mut attachments = snapshot.attachments;
            attachments
                .sort_by_key(|image| snapshot.content.find(&image.label).unwrap_or(usize::MAX));
            attachments
                .into_iter()
                .map(|image| (image.media_type, image.encoded))
                .collect()
        }
        let mut response = self.response();
        if let Some(answers) = response.answers.as_mut() {
            for (q, answer) in answers.iter_mut().enumerate() {
                if answer.other.is_some()
                    && let Some(input) = self.other_inputs.get(q)
                {
                    answer.other_images = images(input, cx);
                }
                for &o in &self.chosen[q] {
                    if let (Some(option), Some(input)) = (
                        self.questions[q].options.get(o),
                        self.comment_inputs.get(q).and_then(|inputs| inputs.get(o)),
                    ) {
                        let attachments = images(input, cx);
                        if !attachments.is_empty() {
                            answer
                                .comment_images
                                .insert(option.label.clone(), attachments);
                        }
                    }
                }
            }
        }
        response
    }
}

impl Panel {
    pub(super) fn answer_ask(&mut self, response: AskResponse, cx: &mut Context<Self>) {
        self.cancel_ask_voice(cx);
        let Some(card) = self.ask_card.as_mut() else {
            return;
        };
        if card.submitting() {
            return;
        }
        static NEXT_ATTEMPT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let attempt_id = NEXT_ATTEMPT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        card.attempt_id = Some(attempt_id);
        card.submit_error = None;
        self.bridge.send(Command::AskResponse {
            session_id: self.session_id.clone(),
            request_id: card.request_id.clone(),
            attachment_id: self.ask_attachment_id,
            attempt_id,
            response,
        });
        cx.notify();
    }
}

// The card's view: a compact, resizable card with rich questions. Questions and
// option descriptions go through the chat's markdown renderer and an applet
// mounted on the ask call renders with the applet surface; long content scrolls
// inside the card, and the tab strip, key hint and answer row never wrap.
const COMMENT_ICON: &[u8] = include_bytes!("../../../../../assets/icons/comment.svg");
const EXPAND_ICON: &[u8] = include_bytes!("../../../../../assets/icons/arrow-down.svg");
const COLLAPSE_ICON: &[u8] = include_bytes!("../../../../../assets/icons/arrow-up.svg");

/// A card taller than this share of the window would cover the conversation it
/// is asking about, so it scrolls instead. Dragging the handle can take it up to
/// `MAX_WINDOW_SHARE`.
const DEFAULT_WINDOW_SHARE: f32 = 0.45;
const MAX_WINDOW_SHARE: f32 = 0.8;
/// The tab strip, one option, the key hint and the answer row.
const MIN_HEIGHT: f32 = 150.;
/// Markdown row numbers for card text, clear of every transcript row.
const MARKDOWN_ROW: usize = 1 << 40;

/// Test-only: where a collapsed description's layout is recorded among the labels'.
#[cfg(test)]
pub(super) const DESCRIPTION_LAYOUT: usize = 1000;

fn ask_action(
    id: &'static str,
    label: &'static str,
    icon: &'static [u8],
    primary: bool,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .when(enabled, crate::keyboard::button)
        .flex_none()
        .size(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(if primary {
            theme.ACCENT_DIM
        } else {
            theme.TOOL_BG
        })
        .when(enabled, |el| {
            el.cursor_pointer().hover(|el| el.bg(theme.ACCENT_DIM))
        })
        .tooltip(move |_, cx| cx.new(|_| CardActionTooltip(label)).into())
        .child(gpui::svg().data(icon).size(px(16.)).text_color(if enabled {
            theme.TEXT
        } else {
            theme.TEXT_FAINT
        }))
}

/// A small round icon button pinned in an option row's right margin.
fn row_button(
    id: &'static str,
    index: usize,
    tip: &'static str,
    icon: &'static [u8],
    lit: bool,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::global();
    crate::keyboard::button(div().id((id, index)))
        .flex_none()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(theme.TOOL_BG)
        .border_1()
        .border_color(if lit {
            theme.ACCENT
        } else {
            theme.INPUT_BORDER
        })
        .hover(|el| el.bg(theme.ACCENT_DIM))
        .tooltip(move |_, cx| cx.new(|_| CardActionTooltip(tip)).into())
        .child(gpui::svg().data(icon).size(px(12.)).text_color(if lit {
            theme.ACCENT
        } else {
            theme.TEXT
        }))
}

impl AskCard {
    /// What a hidden-window run can read of the card beyond its answers: where
    /// the keyboard is, which descriptions are open, a pending decline.
    pub(in crate::panel) fn ux_state(
        &self,
        panel_focused: bool,
        window: &Window,
        cx: &App,
    ) -> serde_json::Value {
        let mut expanded: Vec<_> = self.expanded.iter().copied().collect();
        expanded.sort_unstable();
        let focus = match self.focused_editor(window, cx) {
            Some((_, None)) => "free-text",
            Some((_, Some(_))) => "comment",
            None if panel_focused => "options",
            None => "elsewhere",
        };
        serde_json::json!({"focus": focus, "expanded": expanded, "confirming_decline": self.confirming_decline})
    }
}

impl Panel {
    /// The bounds a drag may give the card, and the height it has by default.
    /// Measured against the window: the room above the composer shrinks as the
    /// card grows, so it cannot size the card it is measured around.
    fn ask_height(
        &self,
        window: &Window,
    ) -> (std::ops::RangeInclusive<gpui::Pixels>, gpui::Pixels) {
        let window_height = window.viewport_size().height;
        (
            px(MIN_HEIGHT)..=(window_height * MAX_WINDOW_SHARE).max(px(MIN_HEIGHT)),
            (window_height * DEFAULT_WINDOW_SHARE).max(px(MIN_HEIGHT)),
        )
    }

    fn render_ask_nav(card: &AskCard, cx: &mut Context<Self>) -> gpui::Div {
        let last_tab = card.cursor_question + 1 >= card.tab_count();
        let enabled = !card.submitting();
        let (id, label, icon): (_, _, &[u8]) = if last_tab {
            (
                "ask-submit",
                if enabled { "Submit" } else { "Sending…" },
                include_bytes!("../../../../../assets/icons/arrow-right-to-line.svg"),
            )
        } else {
            (
                "ask-next",
                "Next",
                include_bytes!("../../../../../assets/icons/arrow-right.svg"),
            )
        };
        div()
            .flex_none()
            .flex()
            .gap_1()
            .when(card.cursor_question > 0, |el| {
                el.child(
                    ask_action(
                        "ask-back",
                        "Back",
                        include_bytes!("../../../../../assets/icons/arrow-left.svg"),
                        false,
                        enabled,
                    )
                    .when(enabled, |el| {
                        el.on_click(cx.listener(|this, _, window, cx| {
                            if let Some(card) = this.ask_card.as_mut() {
                                card.switch_tab(card.cursor_question.saturating_sub(1));
                                window.focus(&this.focus_handle, cx);
                                cx.notify();
                            }
                        }))
                    }),
                )
            })
            .child(
                ask_action(id, label, icon, true, enabled).when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| this.advance_ask(window, cx)))
                }),
            )
    }

    /// Markdown through the chat's renderer, in the card's own selection scope.
    fn ask_markdown(
        &self,
        card: &AskCard,
        source: &str,
        key: String,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        crate::markdown::render_inline_answer(
            source,
            MARKDOWN_ROW,
            &format!("ask-{}-{key}", card.request_id),
            &self.applet_selection,
            window,
            cx,
            self.media_preview_handler(cx),
        )
        .into_any_element()
    }

    fn render_ask_tabs(&self, card: &AskCard, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::global();
        let mut tabs = div()
            .id("ask-tabs")
            .debug_selector(|| "ask-tabs".into())
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden();
        for q in 0..card.tab_count() {
            let label = match card.questions.get(q) {
                Some(question) => match question.header.as_deref().filter(|s| !s.trim().is_empty())
                {
                    Some(header) => format!("{} {}", q + 1, header.trim()),
                    None => format!("Question {}", q + 1),
                },
                None => "Review".to_owned(),
            };
            let label_selector = format!("ask-tab-label-{label}");
            let button = div().id(("ask-tab", q));
            let button = if let Some(focus) = card.tab_focus.get(q) {
                crate::keyboard::tracked_button(button, focus)
            } else {
                crate::keyboard::button(button)
            };
            tabs = tabs.child(
                button
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            if event.keystroke.modifiers != gpui::Modifiers::default() {
                                return;
                            }
                            let Some(card) = this.ask_card.as_mut() else {
                                return;
                            };
                            let next = match event.keystroke.key.as_str() {
                                "left" => (q + card.tab_count() - 1) % card.tab_count(),
                                "right" => (q + 1) % card.tab_count(),
                                _ => return,
                            };
                            card.switch_tab(next);
                            if let Some(handle) = card.tab_focus.get(next) {
                                window.focus(handle, cx);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        },
                    ))
                    .debug_selector(move || format!("ask-tab-{q}"))
                    .min_w_0()
                    .max_w_full()
                    .px_2p5()
                    .py(px(2.))
                    .rounded_full()
                    .cursor_pointer()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(card.cursor_question == q, |el| {
                        el.bg(theme.ACCENT_DIM).text_color(theme.TEXT)
                    })
                    .when(card.cursor_question != q, |el| {
                        el.text_color(theme.TEXT_DIM)
                    })
                    .child(
                        div()
                            .debug_selector(move || label_selector)
                            .truncate()
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, event, window, cx| {
                        cx.stop_propagation();
                        if let Some(card) = this.ask_card.as_mut() {
                            card.switch_tab(q);
                            if !matches!(event, gpui::ClickEvent::Keyboard(_)) {
                                window.focus(&this.focus_handle, cx);
                            }
                            cx.notify();
                        }
                    })),
            );
        }
        tabs
    }

    fn render_ask_mark(card: &AskCard, q: usize, o: usize) -> gpui::Div {
        let theme = Theme::global();
        let multi = card.questions[q].multi_select;
        let selected = card.chosen[q].contains(&o);
        div()
            .debug_selector(move || format!("ask-mark-{q}-{o}"))
            .flex_none()
            .size(px(16.))
            .border_1()
            .border_color(if selected {
                theme.ACCENT
            } else {
                theme.TEXT_DIM
            })
            .when(multi, |el| el.rounded_sm())
            .when(!multi, |el| el.rounded_full())
            .flex()
            .items_center()
            .justify_center()
            .when(!multi, |el| {
                el.child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .when(selected, |el| el.bg(theme.ACCENT)),
                )
            })
            .when(multi, |el| {
                let color = theme.ACCENT;
                el.child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            if !selected {
                                return;
                            }
                            let mut path = gpui::PathBuilder::stroke(px(2.));
                            path.move_to(bounds.origin + gpui::point(px(2.), px(6.)));
                            path.line_to(bounds.origin + gpui::point(px(4.5), px(9.)));
                            path.line_to(bounds.origin + gpui::point(px(10.), px(2.)));
                            if let Ok(path) = path.build() {
                                window.paint_path(path, color);
                            }
                        },
                    )
                    .size(px(12.)),
                )
            })
    }

    fn render_ask_option(
        &self,
        card: &AskCard,
        q: usize,
        o: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = Theme::global();
        let option = &card.questions[q].options[o];
        let selected = card.chosen[q].contains(&o);
        let has_comment = !card.comments[q][o].trim().is_empty();
        let description = option
            .description
            .as_deref()
            .filter(|d| !d.trim().is_empty());
        let can_expand = description.is_some_and(expandable);
        let expanded = can_expand && card.expanded.contains(&(q, o));
        let (label, recommended) = shown_label(&option.label);
        let row = div().id(("ask-option-row", q * 16 + o));
        let row = if let Some(focus) = card.option_focus.get(q).and_then(|rows| rows.get(o)) {
            crate::keyboard::tracked_button(row, focus)
        } else {
            crate::keyboard::button(row)
        };
        let row = row
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    let Some(card) = this.ask_card.as_mut() else {
                        return;
                    };
                    let Some(rows) = card.option_focus.get(q) else {
                        return;
                    };
                    if let Some(next) = crate::keyboard::arrow_peer(event, rows, window, cx) {
                        window.focus(&next, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .flex()
            .flex_col()
            .min_w_0()
            .px_2()
            .py(px(3.))
            .rounded_lg()
            .cursor_pointer()
            .when(!selected, |el| el.hover(|el| el.bg(theme.INPUT_BG)))
            .child(
                // The label is a stretched block with the indicator and the
                // buttons pinned in its margins. As a shrinking flex item the
                // installed app laid it out as one line yet painted it one
                // letter per line.
                div()
                    .relative()
                    .min_w_0()
                    .min_h(px(22.))
                    .pl(px(24.))
                    .pr(px(if can_expand { 54. } else { 28. }))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Self::render_ask_mark(card, q, o)
                            .absolute()
                            .left_0()
                            .top(px(3.)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(gpui::relative(0.6))
                            .debug_selector(move || format!("ask-label-{q}-{o}"))
                            .child({
                                let text = StyledText::new(if o < MAX_NUMBERED {
                                    format!("{}. {label}", o + 1)
                                } else {
                                    label.to_owned()
                                });
                                #[cfg(test)]
                                card.label_layouts
                                    .0
                                    .borrow_mut()
                                    .insert((q, o), text.layout().clone());
                                text
                            }),
                    )
                    .when(recommended, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .debug_selector(move || format!("ask-recommended-{q}-{o}"))
                                .px_1p5()
                                .rounded_full()
                                .bg(theme.ACCENT_DIM)
                                .text_size(px(11.))
                                .text_color(theme.ACCENT)
                                .child("Recommended"),
                        )
                    })
                    .when_some(description.filter(|_| !expanded), |el, desc| {
                        let line = if can_expand { first_line(desc) } else { desc };
                        // The text sits in an absolutely placed box, which always
                        // gets a definite width to cut against; as a flex item's
                        // own text the installed app clipped it without a mark.
                        el.child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h(px(18.))
                                .debug_selector(move || format!("ask-description-{q}-{o}"))
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(theme.TEXT_DIM)
                                        .child({
                                            let text = StyledText::new(line.to_owned());
                                            #[cfg(test)]
                                            card.label_layouts.0.borrow_mut().insert(
                                                (q, DESCRIPTION_LAYOUT + o),
                                                text.layout().clone(),
                                            );
                                            text
                                        }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(can_expand, |el| {
                                el.child(
                                    row_button(
                                        "ask-expand-button",
                                        q * 16 + o,
                                        if expanded { "Collapse" } else { "Expand" },
                                        if expanded { COLLAPSE_ICON } else { EXPAND_ICON },
                                        expanded,
                                    )
                                    .debug_selector(move || format!("ask-expand-button-{q}-{o}"))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if let Some(card) = this.ask_card.as_mut() {
                                                card.cursor_option = o;
                                                card.toggle_expanded(q, o);
                                                cx.stop_propagation();
                                                cx.notify();
                                            }
                                        },
                                    )),
                                )
                            })
                            .child(
                                row_button(
                                    "ask-comment-button",
                                    q * 16 + o,
                                    "Comment",
                                    COMMENT_ICON,
                                    has_comment,
                                )
                                .debug_selector(move || format!("ask-comment-button-{q}-{o}"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if let Some(card) = this.ask_card.as_mut() {
                                            card.cursor_option = o;
                                            card.open_comment(q, o);
                                            if let Some(input) = card
                                                .comment_inputs
                                                .get(q)
                                                .and_then(|inputs| inputs.get(o))
                                            {
                                                let focus = input.read(cx).editor_focus();
                                                window.focus(&focus, cx);
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        }
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(description.filter(|_| expanded), |el, desc| {
                el.child(
                    div()
                        .min_w_0()
                        .ml(px(24.))
                        .mr(px(54.))
                        .pb_1()
                        .text_size(px(13.))
                        .debug_selector(move || format!("ask-description-{q}-{o}"))
                        .text_color(theme.TEXT_DIM)
                        .child(self.ask_markdown(card, desc, format!("q{q}-o{o}"), window, cx)),
                )
            })
            .on_click(cx.listener(move |this, event, window, cx| {
                cx.stop_propagation();
                if let Some(card) = this.ask_card.as_mut() {
                    card.cursor_question = q;
                    card.cursor_option = o;
                    card.toggle(q, o);
                    if !matches!(event, gpui::ClickEvent::Keyboard(_)) {
                        window.focus(&this.focus_handle, cx);
                    }
                    cx.notify();
                }
            }));
        let editor = card
            .comment_inputs
            .get(q)
            .and_then(|inputs| inputs.get(o))
            .filter(|_| card.comment_open[q][o]);
        let bounds = card.option_bounds.clone();
        div()
            .debug_selector(move || format!("ask-option-{q}-{o}"))
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .child(
                gpui::canvas(
                    move |area, _, _| {
                        bounds.borrow_mut().insert(o, area);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .rounded(if editor.is_some() { px(12.) } else { px(16.) })
            .when(selected, |el| el.bg(theme.ACCENT_DIM))
            // Focus changes the outline, not the width used to wrap the options.
            .border_1()
            .when(
                self.focus_handle.is_focused(window) && card.cursor_option == o,
                |el| el.border_color(theme.ACCENT),
            )
            .child(row)
            .when_some(editor, |el, input| {
                el.child(
                    div()
                        .ml(px(30.))
                        .pr_2()
                        .pb_1()
                        .debug_selector(move || format!("ask-comment-{q}-{o}"))
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(embedded_reply_frame(input, selected, window, cx))
                        .child(self.render_ask_microphone(q, Some(o), cx)),
                )
            })
    }

    fn render_ask_review(&self, card: &AskCard) -> (gpui::Div, Option<gpui::AnyElement>) {
        let theme = Theme::global();
        let mut reply = None;
        let answers = card.response().answers.unwrap_or_default();
        let mut review = div()
            .debug_selector(|| "ask-review".into())
            .flex()
            .flex_col()
            .gap_2();
        for (q, (question, answer)) in card.questions.iter().zip(answers).enumerate() {
            let mut values = answer.selected;
            if let Some(other) = answer.other {
                values.push(other);
            }
            let unanswered = values.is_empty();
            let text = if unanswered {
                "Unanswered".to_string()
            } else {
                values.join(", ")
            };
            let mut details = vec![
                div()
                    .when(unanswered, |el| {
                        el.debug_selector(move || format!("ask-review-unanswered-{q}"))
                    })
                    .text_color(theme.TEXT_DIM)
                    .child(text),
            ];
            for &o in &card.chosen[q] {
                let comment = card.comments[q][o].trim();
                if !comment.is_empty() {
                    details.push(
                        div()
                            .debug_selector(move || format!("ask-review-comment-{q}-{o}"))
                            .text_color(theme.TEXT_DIM)
                            .child(format!("{}: {}", question.options[o].label, comment)),
                    );
                }
            }
            if q + 1 == card.questions.len() {
                reply = details.pop().map(|detail| {
                    div()
                        .id("ask-review-tail")
                        .flex_1()
                        .min_w_0()
                        .max_h(px(72.))
                        .overflow_y_scroll()
                        .child(detail)
                        .into_any_element()
                });
            }
            review = review.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!("{}. {}", q + 1, question.question))
                    .children(details),
            );
        }
        (review, reply)
    }

    pub(in crate::panel) fn render_ask_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let card = self.ask_card.as_ref()?;
        let theme = Theme::global();
        let (bounds, automatic) = self.ask_height(window);
        let resized = self.ask_resize.requested_height().is_some();
        let height = if resized {
            self.ask_resize.height(bounds.clone())
        } else {
            automatic
        };
        let measured = Rc::new(Cell::new(height));
        let drag_height = measured.clone();
        let editing = card.focused_editor(window, cx).is_some();
        let on_question = card.questions.get(card.cursor_question);
        let mut body = div()
            .id("ask-card")
            .when_some(card.focus_scope.as_ref(), |el, scope| el.track_focus(scope))
            .debug_selector(|| "ask-card".into())
            .occlude()
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                    if let Some(card) = &this.ask_card {
                        let scroll = &card.scroll;
                        if !scroll.bounds().contains(&event.position) {
                            let dy = event.delta.pixel_delta(window.line_height()).y;
                            let y = (scroll.offset().y + dy).clamp(-scroll.max_offset().y, px(0.));
                            scroll.set_offset(gpui::point(scroll.offset().x, y));
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                }),
            )
            .flex_none()
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .min_w_0()
            .max_h(height)
            .when(resized, |el| el.h(height))
            .overflow_hidden()
            .px_2()
            .pt(px(10.))
            .pb_1p5()
            .rounded_lg()
            .border_1()
            .border_color(theme.INPUT_BORDER)
            .bg(theme.INPUT_BG)
            .text_size(px(13.))
            .text_color(theme.TEXT)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(self.ask_resize.handle(
                        "ask-resize",
                        "ask-resize",
                        move || drag_height.get(),
                        bounds,
                        cx,
                    )),
            )
            .child(
                gpui::canvas(
                    move |bounds, _, _| measured.set(bounds.size.height + px(2.)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .children(
                card.submit_error
                    .as_ref()
                    .map(|error| div().text_color(theme.TEXT_DIM).child(error.clone())),
            );
        let enabled = !card.submitting();
        body = body.child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(self.render_ask_tabs(card, cx))
                .child(
                    ask_action(
                        "ask-decline",
                        "Discard",
                        include_bytes!("../../../../../assets/icons/close.svg"),
                        false,
                        enabled,
                    )
                    .when(enabled, |el| {
                        el.on_click(cx.listener(|this, _, _, cx| {
                            this.answer_ask(AskResponse::declined(), cx);
                        }))
                    }),
                ),
        );
        let mut questions = div()
            .id("ask-scroll")
            .debug_selector(|| "ask-scroll".into())
            .flex()
            .flex_col()
            .gap_1()
            .min_w_0()
            .min_h_0();
        let mut reply = None;
        for (q, question) in card
            .questions
            .iter()
            .enumerate()
            .filter(|(q, _)| *q == card.cursor_question)
        {
            let mut block = div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .p_1p5()
                .rounded_md()
                .bg(theme.QUOTE_BG)
                .child(
                    div()
                        .min_w_0()
                        .pb_1()
                        .font_family(theme.FONT_AI)
                        .debug_selector(move || format!("ask-question-{q}"))
                        .child(self.ask_markdown(
                            card,
                            question_without_header(&question.question, question.header.as_deref()),
                            format!("q{q}"),
                            window,
                            cx,
                        )),
                );
            if let Some(applet) = self.ask_applet(card, window, cx) {
                block = block.child(
                    div()
                        .min_w_0()
                        .pb_1()
                        .debug_selector(|| "ask-applet".into())
                        .child(applet),
                );
            }
            for o in 0..question.options.len() {
                block = block.child(self.render_ask_option(card, q, o, window, cx));
            }
            if let Some(input) = card.other_inputs.get(q) {
                let own_selected = card.other[q].is_some();
                reply = Some(
                    div()
                        .debug_selector(move || format!("ask-other-editor-{q}"))
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(embedded_reply_frame(input, own_selected, window, cx))
                        .child(self.render_ask_microphone(q, None, cx))
                        .into_any_element(),
                );
            }
            questions = questions.child(block);
        }
        if card.cursor_question == card.questions.len() {
            let (review, tail) = self.render_ask_review(card);
            reply = tail.or(reply);
            questions = questions.child(review);
        }
        if card.confirming_decline {
            body = body.child(
                div()
                    .debug_selector(|| "ask-confirm-decline".into())
                    .text_color(theme.TEXT_DIM)
                    .child(
                        "Decline this request? Press Enter to confirm or Esc to keep answering.",
                    ),
            );
        }
        body = body.child(
            div()
                .relative()
                .flex()
                .flex_col()
                .min_h_0()
                .when(resized, |el| el.flex_1())
                .child(crate::transcript_scroll::tracked_vertical(
                    questions,
                    &card.scroll,
                ))
                .child(crate::scrollbar::vertical(&card.scroll, "ask-scrollbar"))
                .when(card.submitting(), |el| {
                    el.child(div().absolute().inset_0().occlude())
                }),
        );
        let scope = if editing { Scope::Text } else { Scope::List };
        Some(
            body.child(
                div()
                    .debug_selector(|| "ask-hint".into())
                    .flex_none()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_FAINT)
                    .child(choice_keys::hint(
                        scope,
                        on_question.is_some_and(|question| question.multi_select),
                    )),
            )
            .child(
                div()
                    .id("ask-reply-row")
                    .debug_selector(|| "ask-reply-row".into())
                    .flex_none()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .children(reply)
                    .child(Self::render_ask_nav(card, cx))
                    .when(card.submitting(), |el| {
                        el.child(div().absolute().inset_0().occlude())
                    }),
            )
            .into_any_element(),
        )
    }

    /// The applet mounted on the ask call, drawn by the applet surface.
    fn ask_applet(
        &self,
        card: &AskCard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let id = crate::applet_runtime::get(cx)
            .tool_card_instance_id(&self.session_id, &card.tool_call_id)?;
        crate::applet_surface::card(&id, false, &self.applet_selection, window, cx)
    }
}

#[cfg(test)]
#[path = "panel_ask_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "panel_ask_ux_tests.rs"]
mod ux_tests;
