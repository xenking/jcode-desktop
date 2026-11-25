//! Inline card for the `ask` tool: the agent blocks on the user's answers.
//!
// Each question owns its free-answer editor and each selected option its comment.
use super::*;
use super::chrome::{CardActionTooltip, embedded_reply_frame};
use jcode_sdk::{AskAnswer, AskQuestion, AskResponse};

const COMMENT_ICON: &[u8] = include_bytes!("../../../assets/icons/comment.svg");

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
        Self {
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
            #[cfg(test)]
            label_layouts: Default::default(),
        }
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
        let focused = |input: &Entity<PromptInput>| input.read(cx).focus_handle.is_focused(window);
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
            self.cursor_option = 0;
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
    fn response_with_images(&self, cx: &App) -> AskResponse {
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
                    window.focus(&input.read(cx).focus_handle.clone(), cx);
                }
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
        if event.keystroke.key == "tab" || card.submitting() {
            return false;
        }
        let q = card.cursor_question;
        let editing = card.focused_editor(window, cx).is_some();
        let key = event.keystroke.key.as_str();
        if editing && key == "escape" {
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return true;
        }
        if editing && !(key == "enter" && !event.keystroke.modifiers.shift) {
            return false;
        }
        if !editing
            && !self.focus_handle.is_focused(window)
            && !self
                .transcript_selection
                .read(cx)
                .focus_handle()
                .is_focused(window)
        {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return false;
        }
        if editing {
            return self.handle_ask_editor_submit(window, cx);
        }
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        if card.confirming_decline && key != "escape" && key != "enter" {
            return true;
        }
        let count = card
            .questions
            .get(q)
            .map_or(0, |question| question.options.len());
        match key {
            "escape" => {
                card.confirming_decline = !card.confirming_decline;
            }
            "enter" if card.confirming_decline => {
                self.answer_ask(AskResponse::default(), cx);
            }
            "left" => {
                card.switch_tab((q + card.tab_count() - 1) % card.tab_count());
            }
            "right" => {
                card.switch_tab((q + 1) % card.tab_count());
            }
            "up" | "k" if count > 0 => {
                card.cursor_option = (card.cursor_option + count - 1) % count
            }
            "down" | "j" if count > 0 => card.cursor_option = (card.cursor_option + 1) % count,
            "space" if count > 0 => card.toggle(q, card.cursor_option.min(count - 1)),
            "n" if count > 0 => {
                let o = card.cursor_option.min(count - 1);
                card.open_comment(q, o);
                if let Some(input) = card.comment_inputs.get(q).and_then(|inputs| inputs.get(o)) {
                    let focus = input.read(cx).focus_handle.clone();
                    window.focus(&focus, cx);
                }
            }
            "enter" => {
                self.advance_ask(window, cx);
                return true;
            }
            digit if digit.len() == 1 && !event.keystroke.modifiers.shift => {
                let Some(o) = digit.as_bytes()[0]
                    .checked_sub(b'1')
                    .map(usize::from)
                    .filter(|&o| o < count && o < 9)
                else {
                    return false;
                };
                card.cursor_option = o;
                card.toggle(q, o);
            }
            _ => return false,
        }
        cx.notify();
        true
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

    fn render_ask_nav(card: &AskCard, cx: &mut Context<Self>) -> gpui::Div {
        let last_tab = card.cursor_question + 1 >= card.tab_count();
        let enabled = !card.submitting();
        let (id, label, icon): (_, _, &[u8]) = if last_tab {
            (
                "ask-submit",
                if enabled { "Submit" } else { "Sending…" },
                include_bytes!("../../../assets/icons/arrow-right-to-line.svg"),
            )
        } else {
            (
                "ask-next",
                "Next",
                include_bytes!("../../../assets/icons/arrow-right.svg"),
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
                        include_bytes!("../../../assets/icons/arrow-left.svg"),
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

    pub(super) fn render_ask_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let card = self.ask_card.as_ref()?;
        let theme = Theme::global();
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
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(theme.INPUT_BORDER)
            .bg(theme.INPUT_BG)
            .text_size(px(13.))
            .text_color(theme.TEXT)
            .children(
                card.submit_error
                    .as_ref()
                    .map(|error| div().text_color(theme.TEXT_DIM).child(error.clone())),
            );
        let mut tabs = div()
            .id("ask-tabs")
            .debug_selector(|| "ask-tabs".into())
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .gap_1();
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
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
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
        let enabled = !card.submitting();
        body = body.child(
            div().flex().items_start().gap_1().child(tabs).child(
                ask_action(
                    "ask-decline",
                    "Discard",
                    include_bytes!("../../../assets/icons/close.svg"),
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
            .gap_2()
            .min_w_0()
            .max_h(px(
                (f32::from(window.viewport_size().height) * 0.4 - 32.).max(60.)
            ));
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
                .gap_1()
                .min_w_0()
                .p_2()
                .rounded_md()
                .bg(theme.QUOTE_BG)
                .child(
                    div()
                        .min_w_0()
                        .pb_1()
                        .debug_selector(move || format!("ask-question-{q}"))
                        .child(
                            question_without_header(&question.question, question.header.as_deref())
                                .to_owned(),
                        ),
                );
            for (o, option) in question.options.iter().enumerate() {
                let selected = card.chosen[q].contains(&o);
                let mark = div()
                    .debug_selector(move || format!("ask-mark-{q}-{o}"))
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
                    .when(!question.multi_select, |el| {
                        el.child(
                            div()
                                .size(px(10.))
                                .rounded_full()
                                .when(selected, |el| el.bg(theme.ACCENT)),
                        )
                    })
                    .when(question.multi_select, |el| {
                        let color = theme.ACCENT;
                        el.child(
                            gpui::canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    if !selected {
                                        return;
                                    }
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
                let has_comment = !card.comments[q][o].trim().is_empty();
                let row = div().id(("ask-option-row", q * 16 + o));
                let row = if let Some(focus) = card.option_focus.get(q).and_then(|rows| rows.get(o))
                {
                    crate::keyboard::tracked_button(row, focus)
                } else {
                    crate::keyboard::button(row)
                };
                let row = row
                    .on_key_down(cx.listener(
                        move |this, event: &gpui::KeyDownEvent, window, cx| {
                            let Some(card) = this.ask_card.as_mut() else {
                                return;
                            };
                            let Some(rows) = card.option_focus.get(q) else {
                                return;
                            };
                            if let Some(next) = crate::keyboard::arrow_peer(event, rows, window, cx)
                            {
                                window.focus(&next, cx);
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .cursor_pointer()
                    .when(!selected, |el| el.hover(|el| el.bg(theme.INPUT_BG)))
                    .child(
                        // The label is a stretched block, like the description
                        // under it, with the indicator and the comment button
                        // pinned in its margins. As a shrinking flex item the
                        // installed app laid it out as one line yet painted it
                        // one letter per line; the description, a stretched
                        // block in the same row, never broke that way.
                        div()
                            .relative()
                            .min_w_0()
                            .min_h(px(26.))
                            .pl(px(26.))
                            .pr(px(34.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .child(mark.absolute().left_0().top(px(4.)))
                            .child(
                                div()
                                    .min_w_0()
                                    .debug_selector(move || format!("ask-label-{q}-{o}"))
                                    .child({
                                        let text = gpui::StyledText::new(if o < 9 {
                                            format!("{}. {}", o + 1, option.label)
                                        } else {
                                            option.label.clone()
                                        });
                                        #[cfg(test)]
                                        card.label_layouts
                                            .0
                                            .borrow_mut()
                                            .insert((q, o), text.layout().clone());
                                        text
                                    }),
                            )
                            .child(
                                crate::keyboard::button(
                                    div().id(("ask-comment-button", q * 16 + o)),
                                )
                                .debug_selector(move || format!("ask-comment-button-{q}-{o}"))
                                .absolute()
                                .top_0()
                                .right_0()
                                .size(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(theme.TOOL_BG)
                                .border_1()
                                .border_color(if has_comment {
                                    theme.ACCENT
                                } else {
                                    theme.INPUT_BORDER
                                })
                                .hover(|el| el.bg(theme.ACCENT_DIM))
                                .tooltip(|_, cx| cx.new(|_| CardActionTooltip("Comment")).into())
                                .child(gpui::svg().data(COMMENT_ICON).size(px(14.)).text_color(
                                    if has_comment {
                                        theme.ACCENT
                                    } else {
                                        theme.TEXT
                                    },
                                ))
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
                                                let focus = input.read(cx).focus_handle.clone();
                                                window.focus(&focus, cx);
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        }
                                    },
                                )),
                            ),
                    )
                    .when_some(option.description.clone(), |el, desc| {
                        el.child(
                            div()
                                .min_w_0()
                                .ml(px(26.))
                                .mr(px(34.))
                                .debug_selector(move || format!("ask-description-{q}-{o}"))
                                .text_color(theme.TEXT_DIM)
                                .child(desc),
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
                block = block.child(
                    div()
                        .debug_selector(move || format!("ask-option-{q}-{o}"))
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .rounded(if editor.is_some() { px(12.) } else { px(20.) })
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
                                    .ml(px(34.))
                                    .pr_2()
                                    .pb_1()
                                    .debug_selector(move || format!("ask-comment-{q}-{o}"))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(embedded_reply_frame(input, selected, window, cx))
                                    .child(self.render_ask_microphone(q, Some(o), cx)),
                            )
                        }),
                );
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
                .child(crate::transcript_scroll::tracked_vertical(
                    questions,
                    &card.scroll,
                ))
                .when(card.submitting(), |el| {
                    el.child(div().absolute().inset_0().occlude())
                }),
        );
        Some(
            body.child(
                div()
                    .id("ask-reply-row")
                    .debug_selector(|| "ask-reply-row".into())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::AskResponseError;
    use jcode_sdk::AskOption;

    fn settle_test_ask(
        panel: &mut Panel,
        request_id: &str,
        result: Result<(), AskResponseError>,
        cx: &mut Context<Panel>,
    ) {
        let attempt_id = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
        panel.ask_response_settled(request_id, attempt_id, result, cx);
    }

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
        assert_eq!(card.response().answers.unwrap()[1], AskAnswer::default());
        card.toggle(1, 1);
        card.toggle(1, 0);
        card.set_other(1, " extra ");
        let answers = card.response().answers.unwrap();
        assert_eq!(answers[0].selected, vec!["B"]);
        assert_eq!(answers[1].selected, vec!["A", "B"]);
        assert_eq!(answers[1].other.as_deref(), Some("extra"));
        card.set_other(0, "mine");
        assert!(card.chosen[0].is_empty());
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

    /// A long option description (the real ask that collapsed: each label was
    /// painted one character per line beside an empty spacer).
    #[gpui::test]
    fn long_option_text_keeps_the_row_width(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        // Verbatim from the session journal of the ask that collapsed.
        let value = serde_json::json!({
            "ev": "ask_request", "session_id": "session-a", "request_id": "ask-1",
            "tool_call_id": "toolu_1",
            "questions": [{"header": "Старый кеш", "options": [
                {"description": "Новая версия этот файл не использует, данные в общей базе.", "label": "Удалить (Recommended)"},
                {"description": "Файл останется на диске, приложение его не трогает.", "label": "Оставить"}],
              "question": "Удалить старый кеш ~/Library/Application Support/ReddyMac/90813.sqlite (около 2 МБ)? Новая версия его не читает: все данные теперь в ~/.local/share/reddy/history.sqlite3. Восстановить после удаления нельзя, но это только копия того, что есть на сервере."}]
        });
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_value(value).unwrap(), cx);
        });
        vcx.run_until_parked();
        let row = vcx.debug_bounds("ask-option-0-0").unwrap();
        let label = vcx.debug_bounds("ask-label-0-0").unwrap();
        let description = vcx.debug_bounds("ask-description-0-0").unwrap();
        let comment = vcx.debug_bounds("ask-comment-button-0-0").unwrap();
        let radio = vcx.debug_bounds("ask-mark-0-0").unwrap();
        assert!(
            label.size.width > px(150.),
            "the label keeps its width: {:?} of {:?}",
            label.size.width,
            row.size.width
        );
        assert!(description.size.width > px(150.));
        assert!(label.size.height < px(40.), "the label stays on one line");
        assert_eq!(label.center().y, radio.center().y);
        assert_eq!(label.center().y, comment.center().y);
        assert!(comment.right() <= row.right() && comment.left() >= label.right() - px(1.));
        assert!(
            comment.left() - label.right() < px(16.),
            "the text fills the row up to the comment button"
        );
        assert!(
            comment.size.width <= px(28.),
            "the comment button is an icon, not a word"
        );
        assert!(
            vcx.debug_bounds("ask-tab-1").is_none(),
            "one question needs no review tab"
        );
        // One question: pick, then Enter sends. No review step.
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("1 enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(
                panel.ask_card.as_ref().is_none_or(|card| card.submitting()),
                "Enter on the only question submits it"
            );
        });
    }

    #[gpui::test]
    fn selecting_an_option_preserves_option_text_geometry(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.simulate_resize(gpui::size(px(900.), px(700.)));
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        let second = AskQuestion {
            question: "Какие идеи Hax стоит добавить в бэклог?".into(),
            header: Some("Из Hax".into()),
            ..question(true)
        };
        let event = serde_json::from_value(serde_json::json!({
            "ev": "ask_request", "session_id": "session-a", "request_id": "ask-1",
            "tool_call_id": "toolu_1", "questions": [{
                "allow_other": true, "header": "Из Codex", "multi_select": true,
                "question": "Что забираем из Codex в первую очередь?",
                "options": [
                    {"label": "CLI для CI (Recommended)", "description": "stdin, отдельный файл ответа, запуск без сохранения сессии. JSON/NDJSON не ломаем."},
                    {"label": "Ревью по Git", "description": "Ревью по base, commit или uncommitted с файлами, строками и серьёзностью. Нынешний /review остаётся."},
                    {"label": "Worktree и Git diff", "description": "Новая сессия в отдельном worktree одной командой; из TUI видно staged, unstaged и untracked diff."},
                    {"label": "AGENTS.md и хуки", "description": "Инструкции от корня до cwd с override и показом источников; хуки до отправки и после ответа."},
                    {"label": "Пока ничего", "description": "Оставить Codex-аудит как бэклог без выбора сейчас."}
                ]
            }, second]
        }))
        .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&event, cx);
        });
        vcx.run_until_parked();

        let rows = [
            ("ask-label-0-0", "ask-description-0-0"),
            ("ask-label-0-1", "ask-description-0-1"),
            ("ask-label-0-2", "ask-description-0-2"),
            ("ask-label-0-3", "ask-description-0-3"),
            ("ask-label-0-4", "ask-description-0-4"),
        ];
        let before = rows.map(|(label, _)| vcx.debug_bounds(label).unwrap().size);
        let first = vcx.debug_bounds("ask-label-0-0").unwrap().center();
        vcx.simulate_click(first, gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], [0]);
        });

        for ((selector, description), before) in rows.into_iter().zip(before) {
            let label = vcx.debug_bounds(selector).unwrap();
            let description = vcx.debug_bounds(description).unwrap();
            assert!(
                label.size == before && label.bottom() <= description.top(),
                "{selector}: label was {before:?}, now {label:?}; description {description:?}"
            );
        }
    }

    /// Every option label paints the lines the layout made room for. The
    /// installed app laid a label out as one line and then painted it one
    /// letter per line over the descriptions below.
    fn assert_labels_paint_inside_their_rows(
        panel: &Entity<Panel>,
        vcx: &mut gpui::VisualTestContext,
        stage: &str,
    ) {
        let options = panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            card.questions[card.cursor_question].options.len()
        });
        let q = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().cursor_question
        });
        for o in 0..options {
            let selector: &'static str = format!("ask-label-{q}-{o}").leak();
            let bounds = vcx.debug_bounds(selector).unwrap();
            let (lines, line_height, text) = panel.read_with(vcx, |panel, _| {
                let card = panel.ask_card.as_ref().unwrap();
                let layouts = card.label_layouts.0.borrow();
                let layout = &layouts[&(q, o)];
                let wrapped = layout.wrapped_text();
                (wrapped.lines().count(), layout.line_height(), wrapped)
            });
            assert!(
                line_height * lines as f32 <= bounds.size.height + px(1.),
                "{stage}: label {o} paints {lines} lines into {bounds:?}: {text:?}"
            );
        }
    }

    #[gpui::test]
    fn option_labels_paint_the_lines_they_were_laid_out_for(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        vcx.simulate_resize(gpui::size(px(120.), px(800.)));
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        // The ask from the report: one question, three long descriptions.
        let event = serde_json::from_value(serde_json::json!({
            "ev": "ask_request", "session_id": "session-a", "request_id": "ask-htpc",
            "tool_call_id": "toolu_htpc", "questions": [{
                "header": "Очистка", "multi_select": false, "allow_other": true,
                "question": "Какой из проверенных наборов удалить с root HTPC?",
                "options": [
                    {"label": "Очистить root (Recommended)", "description": "Освободить около 2,2 ГиБ на root: неиспользуемый Docker-образ node:22, кэш пакетов APT и два неполных rollout-снимка SQLite без манифеста. Сохранить три полных отката, действующие сервисы, видеозаписи и все данные SSD."},
                    {"label": "Только Docker и APT", "description": "Удалить только неиспользуемый Docker-образ node:22 и кэш загрузок APT. Около 1 ГиБ, все бэкапы остаются."},
                    {"label": "Ничего не удалять", "description": "Ничего не удалять. На SSD свободно около 388 ГиБ, пригодных для безопасного удаления копий не нашёл."}
                ]
            }]
        }))
        .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&event, cx);
        });
        vcx.run_until_parked();
        assert_labels_paint_inside_their_rows(&panel, vcx, "shown");

        for width in [260., 520., 900., 640.] {
            vcx.simulate_resize(gpui::size(px(width), px(800.)));
            vcx.run_until_parked();
            assert_labels_paint_inside_their_rows(&panel, vcx, &format!("width {width}"));
        }
        let first = vcx.debug_bounds("ask-label-0-0").unwrap().center();
        vcx.simulate_click(first, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_labels_paint_inside_their_rows(&panel, vcx, "selected");
        let second = vcx.debug_bounds("ask-label-0-1").unwrap().center();
        vcx.simulate_mouse_move(second, None, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_labels_paint_inside_their_rows(&panel, vcx, "hovered");
    }

    #[gpui::test]
    fn question_name_is_in_the_tab_strip(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
        });
        vcx.run_until_parked();
        let tabs = vcx.debug_bounds("ask-tabs").unwrap();
        let label = vcx
            .debug_bounds("ask-tab-label-1 Database")
            .expect("question name is rendered in the tab strip");
        assert!(tabs.contains(&label.center()));
        let question = vcx.debug_bounds("ask-question-0").unwrap();
        assert!(
            question.top() - tabs.bottom() < px(20.),
            "no duplicate heading between tabs and question"
        );
    }

    #[gpui::test]
    fn tabs_digits_and_comment_keep_answers(cx: &mut gpui::TestAppContext) {
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
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        vcx.run_until_parked();
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let mut second = value["questions"][0].clone();
        second["multi_select"] = true.into();
        value["questions"].as_array_mut().unwrap().push(second);
        panel.update(vcx, |panel, cx| {
            panel.apply(&serde_json::from_value(value).unwrap(), cx)
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        assert!(vcx.debug_bounds("ask-question-0").is_some());
        assert!(vcx.debug_bounds("ask-question-1").is_none());
        assert!(vcx.debug_bounds("ask-comment-0-0").is_none());
        let tabs = vcx.debug_bounds("ask-tabs").unwrap();
        let card_bounds = vcx.debug_bounds("ask-card").unwrap();
        assert!(
            tabs.top() - card_bounds.top() <= px(16.),
            "tabs must be the card header"
        );
        vcx.simulate_keystrokes("2 n");
        vcx.run_until_parked();
        let editor = vcx.debug_bounds("ask-comment-0-1").unwrap();
        let option = vcx.debug_bounds("ask-option-0-1").unwrap();
        assert!(
            option.contains(&editor.center()),
            "selected option must contain its own comment editor"
        );
        vcx.simulate_click(editor.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let panel = panel.read(cx);
            assert!(
                panel.ask_card.as_ref().unwrap().comment_inputs[0][1]
                    .read(cx)
                    .focus_handle
                    .is_focused(window),
                "N should focus comment"
            );
        });
        vcx.simulate_input("comment");
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let panel = panel.read(cx);
            assert!(
                panel.focus_handle.is_focused(window),
                "escape should focus options"
            );
        });
        vcx.simulate_keystrokes("right");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-question-1").is_some());
        vcx.simulate_keystrokes("left right 1 down n");
        vcx.run_until_parked();
        vcx.simulate_input("second comment");
        vcx.simulate_keystrokes("shift-tab");
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-question-1").is_some(),
            "Tab navigates controls without switching questions"
        );
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("left");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(card.chosen[1], vec![0, 1]);
            assert_eq!(card.comments[0][1], "comment");
            assert_eq!(card.comments[1][1], "second comment");
            assert_eq!(card.cursor_question, 0);
        });
        vcx.simulate_keystrokes("right right");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-review").is_some());
        vcx.simulate_keystrokes("right");
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-question-0").is_some(),
            "right wraps review to first question"
        );
        vcx.simulate_keystrokes("left");
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-review").is_some(),
            "left wraps first question to review"
        );
        assert!(
            vcx.debug_bounds("ask-review-comment-0-1").is_some(),
            "review should show the selected option's comment"
        );
    }

    #[gpui::test]
    fn escape_requires_confirmation_to_decline(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-confirm-decline").is_some());
        assert!(
            !commands
                .try_iter()
                .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
        );
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-confirm-decline").is_none());
        vcx.simulate_keystrokes("escape enter");
        vcx.run_until_parked();
        assert!(commands.try_iter().any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { response, .. } if response.answers.is_none())));
    }

    #[gpui::test]
    fn wheel_over_option_scrolls_ask_body(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(40, cx));
        vcx.run_until_parked();
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let options = value["questions"][0]["options"].as_array().unwrap().clone();
        value["questions"][0]["options"] =
            serde_json::Value::Array((0..8).flat_map(|_| options.clone()).collect());
        panel.update(vcx, |panel, cx| {
            panel.apply(&serde_json::from_value(value).unwrap(), cx)
        });
        vcx.run_until_parked();
        let option = vcx.debug_bounds("ask-option-0-0").unwrap();
        let before = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        let transcript = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
        vcx.simulate_mouse_move(option.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: option.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-80.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        let after = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        assert!(
            after < before,
            "wheel over option should scroll ask: {before:?} -> {after:?}"
        );
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
            transcript
        );
        panel.update(vcx, |panel, cx| {
            panel
                .ask_card
                .as_ref()
                .unwrap()
                .scroll
                .set_offset(gpui::point(px(0.), px(0.)));
            cx.notify();
        });
        vcx.run_until_parked();
        let option = vcx.debug_bounds("ask-option-0-0").unwrap();
        vcx.simulate_mouse_move(option.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: option.center(),
            delta: gpui::ScrollDelta::Lines(gpui::point(0., -2.)),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        assert!(
            panel.read_with(vcx, |panel, _| panel
                .ask_card
                .as_ref()
                .unwrap()
                .scroll
                .offset()
                .y)
                < px(0.),
            "line wheel over option should scroll the card"
        );
        panel.update(vcx, |panel, cx| {
            let scroll = &panel.ask_card.as_ref().unwrap().scroll;
            scroll.set_offset(gpui::point(px(0.), -scroll.max_offset().y));
            cx.notify();
        });
        vcx.run_until_parked();
        let own = panel.read_with(vcx, |panel, cx| {
            panel.ask_card.as_ref().unwrap().other_inputs[0]
                .read(cx)
                .test_editor_scroll()
                .bounds()
        });
        let before = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        vcx.simulate_mouse_move(own.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: own.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(80.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        let after = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        assert!(
            after > before,
            "wheel over short editor should scroll ask: {before:?} -> {after:?}"
        );
        let input = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().other_inputs[0].clone()
        });
        input.update(vcx, |input, cx| {
            input.set_content("long answer\n".repeat(100), cx)
        });
        vcx.run_until_parked();
        panel.update(vcx, |panel, cx| {
            let scroll = &panel.ask_card.as_ref().unwrap().scroll;
            scroll.set_offset(gpui::point(px(0.), -scroll.max_offset().y));
            cx.notify();
        });
        vcx.run_until_parked();
        let editor = input.read_with(vcx, |input, _| input.test_editor_scroll());
        assert!(
            editor.max_offset().y > px(100.),
            "long editor must overflow"
        );
        editor.set_offset(gpui::point(px(0.), px(-100.)));
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
        let own = editor.bounds();
        let before_card = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        let before_editor = editor.offset().y;
        vcx.simulate_mouse_move(own.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: own.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(40.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        assert!(
            editor.offset().y > before_editor,
            "overflowing editor should scroll first"
        );
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel
                .ask_card
                .as_ref()
                .unwrap()
                .scroll
                .offset()
                .y),
            before_card,
            "editor wheel must not also scroll the card"
        );
        editor.set_offset(gpui::point(px(0.), px(0.)));
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: own.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(40.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        let after_card = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().scroll.offset().y
        });
        assert!(
            after_card > before_card,
            "wheel at editor edge should scroll the card: {before_card:?} -> {after_card:?}"
        );
    }

    #[gpui::test]
    fn space_selects_ask_when_transcript_has_focus(cx: &mut gpui::TestAppContext) {
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
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
            panel.apply(&event, cx);
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let focus = panel
                .read(cx)
                .transcript_selection
                .read(cx)
                .focus_handle()
                .clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("space");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], vec![0]);
            assert!(panel.input.read(cx).snapshot().content.is_empty());
        });
    }

    #[gpui::test]
    fn keyboard_navigates_multiple_groups_and_submits(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
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
        vcx.update(|window, cx| window.focus(&panel.read(cx).input_focus_handle(cx), cx));
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let second = value["questions"][0].clone();
        value["questions"].as_array_mut().unwrap().push(second);
        let event: jcode_sdk::ApiEvent = serde_json::from_value(value).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("j k up space right j space enter");
        vcx.run_until_parked();
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(commands.try_iter().any(|command| matches!(command,
            crate::harness::Command::AskResponse { response, .. }
            if response.answers.as_ref().is_some_and(|answers|
                answers.len() == 2 && answers[0].selected == ["DynamoDB"] && answers[1].selected == ["SQLite"])
        )), "keyboard selection must submit both groups");
    }

    #[gpui::test]
    fn ask_can_submit_after_skipping_a_question(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        vcx.run_until_parked();
        vcx.update(|window, cx| window.focus(&panel.read(cx).input_focus_handle(cx), cx));
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let second = value["questions"][0].clone();
        value["questions"].as_array_mut().unwrap().push(second);
        let event = serde_json::from_value(value).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("1 enter enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-review").is_some());
        let submit = vcx.debug_bounds("ask-submit").unwrap();
        let back = vcx.debug_bounds("ask-back").unwrap();
        assert!((back.center().y - submit.center().y).abs() <= px(1.));
        assert!(back.right() <= submit.left());
        vcx.simulate_click(back.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-question-1").is_some());
        let next = vcx.debug_bounds("ask-next").unwrap();
        let input = vcx.debug_bounds("ask-other-editor-1").unwrap();
        let back = vcx.debug_bounds("ask-back").unwrap();
        assert!((back.center().y - input.center().y).abs() <= px(1.));
        assert!(input.right() <= back.left() && back.right() <= next.left());
        vcx.simulate_click(next.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let submit = vcx.debug_bounds("ask-submit").unwrap();
        let unanswered = vcx.debug_bounds("ask-review-unanswered-1");
        vcx.simulate_click(submit.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            commands.try_iter().any(|command| matches!(command,
                Command::AskResponse { response, .. }
                if response.answers == Some(vec![AskAnswer {
                    selected: vec!["Postgres".into()],
                    ..Default::default()
                }, AskAnswer::default()])
            )),
            "submit must preserve the skipped question without selecting a default"
        );
        let unanswered = unanswered.expect("review labels the skipped question");
        assert!((unanswered.center().y - submit.center().y).abs() <= px(1.));
    }

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
        q["question"] = serde_json::Value::String("This question must wrap cleanly. ".repeat(12));
        q["options"][0]["label"] =
            serde_json::Value::String("This option label must wrap. ".repeat(8));
        q["options"][0]["description"] = serde_json::Value::String(
            "This description must wrap cleanly without truncating. ".repeat(16),
        );
        value["questions"] = serde_json::Value::Array(vec![q; 4]);
        let event: jcode_sdk::ApiEvent = serde_json::from_value(value).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        let viewport = vcx.debug_bounds("ask-scroll").unwrap();
        let footer = vcx.debug_bounds("ask-next").unwrap();
        let input = vcx.debug_bounds("ask-other-editor-0").unwrap();
        assert!((input.center().y - footer.center().y).abs() <= px(1.));
        assert!(input.right() <= footer.left());
        let description = vcx.debug_bounds("ask-description-0-0").unwrap();
        let question = vcx.debug_bounds("ask-question-0").unwrap();
        let label = vcx.debug_bounds("ask-label-0-0").unwrap();
        assert!(
            question.size.height > px(24.),
            "long question must wrap: {question:?}"
        );
        assert!(
            label.size.height > px(24.),
            "long option label must wrap: {label:?}"
        );
        assert!(
            description.size.height > px(24.),
            "long description must wrap: {description:?}"
        );
        let transcript_before = panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y());
        let first_before = vcx.debug_bounds("ask-option-0-0").unwrap();
        vcx.simulate_mouse_move(footer.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: footer.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-option-0-0").unwrap().top() < first_before.top(),
            "wheel over sticky footer must scroll the card"
        );
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.test_scroll_offset_y()),
            transcript_before
        );
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
        assert_eq!(vcx.debug_bounds("ask-other-editor-0"), Some(input));
        assert!(input.top() >= viewport.bottom());
        assert_eq!(
            vcx.debug_bounds("ask-next"),
            Some(footer),
            "footer must remain fixed"
        );
        vcx.simulate_mouse_move(viewport.center(), None, gpui::Modifiers::default());
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-6000.))),
            modifiers: gpui::Modifiers::default(),
            touch_phase: gpui::TouchPhase::Started,
        });
        vcx.run_until_parked();
        let own = vcx.debug_bounds("ask-other-editor-0").unwrap();
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
                card.other_inputs[0].read(cx).snapshot().content,
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
        let input = serde_json::json!({"questions":[
            {"question":"Which?","options":[{"label":"A"},{"label":"B"}]},
            {"question":"Where?","options":[{"label":"Here (Recommended)"},{"label":"There"}]}
        ]});
        let history: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
            {"role":"user","content":"Choose"},
            {"role":"tool","content":"User answers:\n1. Which? -> B (comment: reason)\n2. Where? -> unanswered (user skipped)\n","tool":{"id":"ask-1","name":"ask","input":input}}
        ])).unwrap();
        let image = jcode_sdk::RenderedImage {
            session_image: None,
            history_message_index: Some(2),
            media_type: "image/png".into(),
            data: base64::engine::general_purpose::STANDARD
                .encode(include_bytes!("../../../assets/previews/image-preview.png")),
            label: Some("Answer image".into()),
            source: jcode_sdk::RenderedImageSource::ToolResult {
                tool_name: "ask".into(),
            },
            anchor: Some(jcode_sdk::RenderedImageAnchor::ToolCall { id: "ask-1".into() }),
        };
        let mut second = image.clone();
        second.history_message_index = None;
        second.label = Some("Comment image".into());
        panel.update(vcx, |panel, cx| {
            panel.load_history(history, vec![image], cx);
            let event = jcode_sdk::ApiEvent::SidePaneImages {
                session_id: "session-a".into(),
                images: vec![second],
            };
            panel.apply(&event, cx);
            panel.apply(&event, cx);
        });
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| assert!(panel.items.iter().any(|item| matches!(item, Item::Tool { name, output, .. } if name == "ask" && output.contains("reason")))));
        let row = vcx
            .debug_bounds("decision-header")
            .expect("compact decision row");
        vcx.simulate_click(row.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("decision-detail").is_some(),
            "reopened history must render ask result"
        );
        assert!(
            vcx.debug_bounds("decision-answer-Unanswered (user skipped)")
                .is_some(),
            "history must show the skipped answer, not the recommended option"
        );
        panel.read_with(vcx, |panel, _| {
            let indices = panel.session_image_indices();
            assert_eq!(
                indices.len(),
                2,
                "distinct labels survive, replay is deduplicated"
            );
            assert!(
                !panel
                    .transcript_render_rows()
                    .iter()
                    .any(|row| indices.contains(&row.index))
            );
        });
        let chip = vcx
            .debug_bounds("ask-result-image")
            .expect("image appears inside ask result");
        assert!(vcx.debug_bounds("ask-result-image-thumbnail").is_some());
        vcx.simulate_click(chip.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("image-preview-full").is_some());
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
        let (request_id, first_attempt) = panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            (card.request_id.clone(), card.attempt_id.unwrap())
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id: request_id.clone(),
                    attempt_id: first_attempt,
                    result: Err(AskResponseError::Failed("offline".into())),
                },
                cx,
            )
        });
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            assert!(!card.submitting());
            assert_eq!(card.submit_error.as_deref(), Some("offline"));
            assert_eq!(card.chosen[0], vec![1]);
        });
        panel.update(vcx, |panel, cx| {
            let response = panel.ask_card.as_ref().unwrap().response();
            panel.answer_ask(response, cx);
        });
        let attempt_id = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().attempt_id.unwrap()
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id,
                    attempt_id,
                    result: Ok(()),
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
    fn ask_draft_restores_if_request_arrives_before_snapshot(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace.push_test_panel("session-b", cx);
            workspace
        });
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
        let snapshot = first.read_with(vcx, |panel, cx| panel.snapshot(cx));
        second.update(vcx, |panel, cx| {
            panel.apply(&event, cx);
            panel.restore_snapshot(snapshot, cx);
        });
        second.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(
                card.comment_inputs[0][1].read(cx).snapshot().content,
                "important note"
            );
        });
    }

    #[gpui::test]
    fn decline_sends_empty_response_and_renders_question_status(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        for _ in 0..120 {
            if vcx.debug_bounds("ask-decline").is_some() {
                break;
            }
            vcx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            vcx.update(|window, cx| window.simulate_next_frame(cx));
            vcx.run_until_parked();
        }
        let decline = vcx.debug_bounds("ask-decline").unwrap();
        let tabs = vcx.debug_bounds("ask-tabs").unwrap();
        let card = vcx.debug_bounds("ask-card").unwrap();
        assert!(decline.top() < tabs.bottom() && decline.bottom() <= tabs.bottom() + px(1.));
        assert!(card.right() - decline.right() <= px(12.));
        assert!(
            decline.size.width <= px(28.),
            "the discard control is an icon"
        );
        vcx.simulate_click(decline.center(), gpui::Modifiers::default());
        assert!(
            commands.try_iter().any(|command| matches!(command,
                crate::harness::Command::AskResponse { response, .. } if response.answers.is_none()
            )),
            "decline must send an empty response"
        );
        panel.update(vcx, |panel, cx| {
            panel.apply(&jcode_sdk::ApiEvent::ToolDone {
                session_id: "session-a".into(), call_id: "toolu_01J29QR2NCg12HueHJkMWtZU".into(),
                name: "ask".into(), output: "User declined to answer the questions.".into(), error: None,
                codemode: None,
            }, cx);
            assert!(panel.ask_card.is_none());
            let Item::Tool { input, output, error, .. } = panel.items.iter().find(|item| matches!(item, Item::Tool { call_id, .. } if call_id == "toolu_01J29QR2NCg12HueHJkMWtZU")).unwrap() else { unreachable!() };
            let text = result_text(input, output, error.as_deref());
            assert!(text.contains("Which database") && text.contains("declined"), "{text}");
        });
    }

    #[gpui::test]
    fn cancelled_ask_closes_and_renders_questions_and_status(cx: &mut gpui::TestAppContext) {
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
            panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "unrelated".into(), name: "ask".into(), output: "cancelled".into(), error: Some("cancelled".into()), codemode: None }, cx);
            assert!(panel.ask_card.is_some());
            panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "toolu_01J29QR2NCg12HueHJkMWtZU".into(), name: "ask".into(), output: "cancelled by the user".into(), error: Some("cancelled by the user".into()), codemode: None }, cx);
            assert!(panel.ask_card.is_none(), "completed tool must close its card");
            let Item::Tool { input, output, error, .. } = panel.items.iter().find(|item| matches!(item, Item::Tool { call_id, .. } if call_id == "toolu_01J29QR2NCg12HueHJkMWtZU")).unwrap() else { unreachable!() };
            assert!(result_text(input, output, error.as_deref()).contains("Which database"));
        });
    }

    #[gpui::test]
    fn ask_comment_image_survives_draft_and_explicit_submit(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            let event = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
            panel.apply(&event, cx);
            panel.ask_card.as_mut().unwrap().toggle(0, 0);
            panel.ask_card.as_mut().unwrap().comment_open[0][0] = true;
            cx.notify();
        });
        vcx.run_until_parked();
        let input = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().comment_inputs[0][0].clone()
        });
        input.update(vcx, |input, cx| {
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: vec![2],
                    width: 1,
                    height: 1,
                },
                cx,
            )
        });
        let draft = panel.read_with(vcx, |panel, cx| panel.ask_card.as_ref().unwrap().draft(cx));
        let draft = serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
        panel.update(vcx, |panel, cx| {
            panel.ask_card.as_mut().unwrap().restore_draft(draft, cx)
        });
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(
                card.comment_inputs[0][0].read(cx).snapshot().attachments[0].encoded,
                "Ag=="
            );
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| window.focus(&input.read(cx).focus_handle.clone(), cx));
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(
            !commands
                .try_iter()
                .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
        );
        let submit = vcx.debug_bounds("ask-submit").unwrap().center();
        vcx.simulate_click(submit, gpui::Modifiers::default());
        vcx.run_until_parked();
        let response = commands
            .try_iter()
            .find_map(|command| match command {
                crate::harness::Command::AskResponse { response, .. } => {
                    Some(serde_json::to_value(response).unwrap())
                }
                _ => None,
            })
            .expect("ask response was sent");
        assert_eq!(
            response["answers"][0]["comment_images"]["Postgres"][0][1],
            "Ag=="
        );
        assert_eq!(response["answers"][0]["comments"]["Postgres"], "[#image 1]");
    }

    #[gpui::test]
    fn ask_answer_image_is_sent_and_survives_incomplete_enter(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        let mut value: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let question = value["questions"][0].clone();
        value["questions"].as_array_mut().unwrap().push(question);
        let event = serde_json::from_value(value).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let input = panel.read(cx).ask_card.as_ref().unwrap().other_inputs[0].clone();
            window.focus(&input.read(cx).focus_handle.clone(), cx);
            input.update(cx, |input, cx| {
                input.attach_image(
                    crate::clipboard_image::ClipboardImage {
                        media_type: "image/png".into(),
                        bytes: vec![1],
                        width: 1,
                        height: 1,
                    },
                    cx,
                )
            });
        });
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            let snapshot = card.other_inputs[0].read(cx).snapshot();
            assert_eq!(
                snapshot.attachments.len(),
                1,
                "incomplete Enter must retain the image"
            );
        });
        panel.update(vcx, |panel, cx| {
            let card = panel.ask_card.as_mut().unwrap();
            card.toggle(1, 0);
            cx.notify();
        });
        vcx.run_until_parked();
        let next = vcx.debug_bounds("ask-next").unwrap().center();
        vcx.simulate_click(next, gpui::Modifiers::default());
        vcx.run_until_parked();
        let submit = vcx.debug_bounds("ask-submit").unwrap().center();
        vcx.simulate_click(submit, gpui::Modifiers::default());
        vcx.run_until_parked();
        let response = commands
            .try_iter()
            .find_map(|command| match command {
                crate::harness::Command::AskResponse { response, .. } => {
                    Some(serde_json::to_value(response).unwrap())
                }
                _ => None,
            })
            .expect("ask response was sent");
        assert_eq!(response["answers"][0]["other_images"][0][1], "AQ==");
        panel.update(vcx, |panel, cx| {
            let request_id = panel.ask_card.as_ref().unwrap().request_id.clone();
            settle_test_ask(
                panel,
                &request_id,
                Err(AskResponseError::Failed("network error".into())),
                cx,
            );
        });
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert!(!card.submitting());
            assert_eq!(
                card.other_inputs[0].read(cx).snapshot().attachments[0].encoded,
                "AQ=="
            );
        });
    }

    #[gpui::test]
    fn ask_editor_receives_space_and_printable_shortcuts(cx: &mut gpui::TestAppContext) {
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
        panel.update(vcx, |panel, cx| panel.test_settled_conversation(1, cx));
        vcx.run_until_parked();
        let event: jcode_sdk::ApiEvent = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let focus = panel.read(cx).ask_card.as_ref().unwrap().other_inputs[0]
                .read(cx)
                .focus_handle
                .clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_input("a");
        vcx.simulate_keystrokes("space");
        vcx.simulate_input("b");
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.other_inputs[0].read(cx).snapshot().content, "a b");
            assert!(card.chosen[0].is_empty());
        });
        vcx.simulate_keystrokes("n 1 j k shift-space shift-enter");
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(
                card.other_inputs[0].read(cx).snapshot().content,
                "a bn1jk \n"
            );
            assert!(card.chosen[0].is_empty());
            assert!(!card.submitting());
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).focus_handle.clone();
            window.focus(&focus, cx);
        });
        vcx.simulate_keystrokes("space n");
        vcx.simulate_input("a");
        vcx.simulate_keystrokes("space");
        vcx.simulate_input("bn1jk");
        vcx.simulate_keystrokes("shift-enter");
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![0]);
            assert_eq!(
                card.comment_inputs[0][0].read(cx).snapshot().content,
                "a bn1jk\n"
            );
            assert!(!card.submitting());
        });
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(!panel.ask_card.as_ref().unwrap().submitting())
        });
        vcx.simulate_keystrokes("escape enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(panel.ask_card.as_ref().unwrap().submitting())
        });
    }

    fn stream_ask(panel: &mut Panel, cx: &mut Context<Panel>) -> ApiEvent {
        let request: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        let call_id = request["tool_call_id"].as_str().unwrap();
        let input = serde_json::json!({"questions": request["questions"]}).to_string();
        for event in [
            serde_json::json!({"ev":"tool_start", "session_id":"session-a", "call_id":call_id, "name":"ask"}),
            serde_json::json!({"ev":"tool_input_delta", "session_id":"session-a", "call_id":call_id, "delta":input}),
            serde_json::json!({"ev":"tool_exec", "session_id":"session-a", "call_id":call_id, "name":"ask"}),
        ] {
            panel.apply(&serde_json::from_value(event).unwrap(), cx);
        }
        serde_json::from_value(request).unwrap()
    }

    #[gpui::test]
    fn missing_ask_request_recovers_from_live_tool_and_merges_late_event(
        cx: &mut gpui::TestAppContext,
    ) {
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
        let event = panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            stream_ask(panel, cx)
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-card").is_none(),
            "wait for live AskRequest first"
        );
        vcx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-card").is_some(),
            "the running tool must be answerable without an AskRequest"
        );
        panel.update(vcx, |panel, cx| {
            let card = panel.ask_card.as_mut().unwrap();
            let ApiEvent::AskRequest {
                request_id,
                tool_call_id,
                ..
            } = &event
            else {
                unreachable!()
            };
            assert_eq!(&card.request_id, request_id);
            assert_eq!(&card.tool_call_id, tool_call_id);
            card.toggle(0, 1);
            let editor = card.comment_inputs[0][1].clone();
            panel.apply(&event, cx);
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(card.comment_inputs[0][1], editor);
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel, request_id, Ok(()), cx);
            panel.apply(&event, cx);
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("ask-card").is_none(),
            "neither replay nor the unfinished transcript row may reopen an acknowledged ask"
        );
    }

    #[gpui::test]
    fn every_replayed_ask_stays_answerable_in_order(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
        let ask = |request_id: &str, call_id: &str, header: &str| -> ApiEvent {
            serde_json::from_value(serde_json::json!({
                "ev": "ask_request", "session_id": "session-a",
                "request_id": request_id, "tool_call_id": call_id,
                "questions": [{"header": header, "question": format!("{header}?"),
                    "options": [{"label": "A"}, {"label": "B"}]}]
            }))
            .unwrap()
        };
        // A reconnect replays every pending question: an earlier `ask` and a
        // computer-use consent prompt that were both waiting on the owner.
        let team = ask("ask-team", "team", "Teams");
        let consent = ask("ask-computer-consent", "consent", "Computer use");
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&team, cx);
            panel.apply(&consent, cx);
            panel.apply(&team, cx);
            assert_eq!(
                panel.ask_card.as_ref().unwrap().request_id,
                "ask-team",
                "the first question stays on screen"
            );
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel, "ask-team", Ok(()), cx);
            assert_eq!(
                panel.ask_card.as_ref().map(|card| card.request_id.as_str()),
                Some("ask-computer-consent"),
                "a second pending question must not be lost behind the first"
            );
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel, "ask-computer-consent", Ok(()), cx);
            assert!(panel.ask_card.is_none());
        });
        let answered: Vec<_> = commands
            .try_iter()
            .filter_map(|command| match command {
                crate::harness::Command::AskResponse { request_id, .. } => Some(request_id),
                _ => None,
            })
            .collect();
        assert_eq!(answered, ["ask-team", "ask-computer-consent"]);

        // A finished tool retires its queued question without showing it.
        let first = ask("ask-one", "one", "One");
        let second = ask("ask-two", "two", "Two");
        panel.update(vcx, |panel, cx| {
            panel.apply(&first, cx);
            panel.apply(&second, cx);
            panel.apply(
                &serde_json::from_value(serde_json::json!({
                    "ev": "tool_done", "session_id": "session-a", "call_id": "two",
                    "name": "ask", "output": "User declined"
                }))
                .unwrap(),
                cx,
            );
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel, "ask-one", Ok(()), cx);
            assert!(
                panel.ask_card.is_none(),
                "a finished ask is not shown again"
            );
        });
    }

    #[gpui::test]
    fn rejected_answer_stays_on_the_card_with_the_reason(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
            let request = panel.ask_card.as_ref().unwrap().request_id.clone();
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(
                panel,
                &request,
                Err(AskResponseError::Failed(
                    "Could not send your answer: your answer was not accepted".into(),
                )),
                cx,
            );
            let card = panel
                .ask_card
                .as_ref()
                .expect("a rejected answer keeps the card");
            assert!(!card.submitting());
            assert!(
                card.submit_error
                    .as_deref()
                    .unwrap()
                    .contains("not accepted")
            );
        });
    }

    #[gpui::test]
    fn stale_cold_wake_releases_composer_without_another_response(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
        let request = "ask-cold-wake-expired";
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.input.update(cx, |input, cx| {
                input.set_content("draft to keep".into(), cx)
            });
            let mut event: serde_json::Value = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
            event["request_id"] = request.into();
            panel.apply(&serde_json::from_value(event).unwrap(), cx);
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let choice = panel.read(cx).ask_card.as_ref().unwrap().option_focus[0][0].clone();
            window.focus(&choice, cx);
        });
        panel.update(vcx, |panel, cx| {
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel,
                request,
                Err(AskResponseError::Stale),
                cx,
            );
            assert!(panel.ask_card.is_none(), "a stale answer must not leave a blocking card");
            assert!(panel.items.iter().any(|item| matches!(item, super::super::Item::Error(message) if message.contains("Your answer was not accepted"))));
            assert_eq!(panel.input.read(cx).snapshot().content, "draft to keep");
        });
        vcx.run_until_parked();
        assert!(vcx.update(|window, cx| {
            panel
                .read(cx)
                .input
                .read(cx)
                .focus_handle
                .is_focused(window)
        }));
        let responses: Vec<_> = commands
            .try_iter()
            .filter(|command| matches!(command, Command::AskResponse { .. }))
            .collect();
        assert_eq!(
            responses.len(),
            1,
            "closing a stale card must not send another answer"
        );
    }

    #[gpui::test]
    fn ask_recovery_after_snapshot_ignores_only_expired_tools(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "session-a".into(),
                None,
                None,
                crate::harness::spawn_recording().0,
                cx,
            )
        });
        let fallback_tool = |id: &str| super::super::Item::Tool {
            call_id: id.into(),
            name: "ask".into(),
            input: serde_json::json!({"questions": [question(false)]}).to_string(),
            output: String::new(),
            done: false,
            error: None,
        };
        panel.update(vcx, |panel, cx| {
            panel.items.push(fallback_tool("expired"));
            panel.reconcile_asks(1, Some(&[]), cx);
            assert!(panel.recoverable_ask().is_none());
            panel.items.push(fallback_tool("fresh"));
            panel.schedule_ask_recovery(cx);
        });
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert_eq!(panel.ask_card.as_ref().unwrap().request_id, "ask-fresh");
        });
    }

    #[gpui::test]
    fn reconnect_retires_missing_asks_and_late_ack_cannot_close_the_current_attempt(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
        panel.update(vcx, |panel, cx| {
            let expired = "ask-cold-wake-old";
            let live = "ask-cold-wake-live";
            let live_pending = [live.into()];
            panel.reconcile_asks(1, None, cx);
            panel.show_live_ask(expired, expired, &[question(false)], cx);
            panel.show_live_ask(live, live, &[question(true)], cx);
            panel.answer_ask(AskResponse::declined(), cx);
            let expired_attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
            panel.reconcile_asks(2, Some(&live_pending), cx);
            assert_eq!(panel.ask_card.as_ref().unwrap().request_id, live);
            panel.ask_card.as_mut().unwrap().toggle(0, 1);
            panel.answer_ask(panel.ask_card.as_ref().unwrap().response(), cx);
            let previous_attempt = panel.ask_card.as_ref().unwrap().attempt_id.unwrap();
            panel.reconcile_asks(3, Some(&live_pending), cx);
            assert_eq!(panel.ask_card.as_ref().unwrap().chosen[0], [1]);
            assert!(!panel.ask_card.as_ref().unwrap().submitting());
            panel.answer_ask(panel.ask_card.as_ref().unwrap().response(), cx);
            panel.ask_response_settled(expired, expired_attempt, Ok(()), cx);
            panel.ask_response_settled(live, previous_attempt, Ok(()), cx);
            assert!(panel.ask_card.as_ref().unwrap().submitting());
            settle_test_ask(panel, live, Ok(()), cx);
            assert!(panel.ask_card.is_none());
        });
        let answer_attachments: Vec<_> = commands
            .try_iter()
            .filter_map(|command| match command {
                Command::AskResponse { attachment_id, .. } => Some(attachment_id),
                _ => None,
            })
            .collect();
        assert_eq!(answer_attachments, [1, 2, 3]);
    }

    #[gpui::test]
    fn acknowledged_ask_returns_keyboard_focus_to_composer(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, _) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |w, _| w.test_panel(0)).unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
        });
        vcx.run_until_parked();
        let choice = vcx.update(|window, cx| {
            let handle = panel.read(cx).ask_card.as_ref().unwrap().option_focus[0][0].clone();
            window.focus(&handle, cx);
            handle
        });
        vcx.run_until_parked();
        assert!(vcx.update(|window, _| choice.is_focused(window)));
        panel.update(vcx, |panel, cx| {
            let request = panel.ask_card.as_ref().unwrap().request_id.clone();
            panel.answer_ask(AskResponse::declined(), cx);
            settle_test_ask(panel, &request, Ok(()), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.update(|window, cx| {
            panel
                .read(cx)
                .input
                .read(cx)
                .focus_handle
                .is_focused(window)
        }));
    }

    #[gpui::test]
    fn restored_in_flight_ask_recovers_without_an_event(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("session-a", cx);
            workspace.push_test_panel("session-restored", cx);
            workspace
        });
        let (live, restored) = workspace.read_with(vcx, |workspace, _| {
            (
                workspace.test_panel(0).unwrap(),
                workspace.test_panel(1).unwrap(),
            )
        });
        let snapshot = live.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            stream_ask(panel, cx);
            panel.snapshot_for_reload(cx)
        });
        restored.update(vcx, |panel, cx| panel.restore_snapshot(snapshot, cx));
        vcx.run_until_parked();
        vcx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        vcx.run_until_parked();
        restored.read_with(vcx, |panel, _| {
            assert_eq!(
                panel.ask_card.as_ref().unwrap().questions[0]
                    .header
                    .as_deref(),
                Some("Database")
            );
        });
    }

    #[gpui::test]
    fn typed_comment_selects_its_option_without_submitting(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("session-a", cx);
            workspace
        });
        let panel = workspace
            .read_with(vcx, |workspace, _| workspace.test_panel(0))
            .unwrap();
        panel.update(vcx, |panel, cx| {
            panel.test_settled_conversation(1, cx);
            panel.apply(&serde_json::from_str(REAL_ASK_REQUEST).unwrap(), cx);
        });
        vcx.run_until_parked();

        let option = vcx.debug_bounds("ask-option-0-0").unwrap().center();
        vcx.simulate_click(option, gpui::Modifiers::default());
        let comment = vcx.debug_bounds("ask-comment-button-0-1").unwrap().center();
        vcx.simulate_click(comment, gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
            assert_eq!(
                answer.selected,
                ["Postgres"],
                "empty comment focus changed the answer"
            );
        });
        vcx.simulate_input("reason");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let answer = &panel.ask_card.as_ref().unwrap().response().answers.unwrap()[0];
            assert_eq!(
                answer.selected,
                ["SQLite"],
                "nonempty comment must select its radio option"
            );
        });
        vcx.simulate_keystrokes("shift-enter");
        vcx.simulate_input("second line");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(
            !commands
                .try_iter()
                .any(|cmd| matches!(cmd, crate::harness::Command::AskResponse { .. }))
        );
        let submit = vcx.debug_bounds("ask-submit").unwrap().center();
        vcx.simulate_click(submit, gpui::Modifiers::default());
        vcx.run_until_parked();
        let response = commands
            .try_iter()
            .find_map(|cmd| match cmd {
                crate::harness::Command::AskResponse { response, .. } => Some(response),
                _ => None,
            })
            .expect("explicit Submit must send the answer");
        let answers = response.answers.unwrap();
        assert_eq!(answers[0].selected, ["SQLite"]);
        assert_eq!(answers[0].comments["SQLite"], "reason\nsecond line");
    }

    #[gpui::test]
    fn real_ask_request_event_renders_card_and_submits(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
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
        let bar_before = vcx.debug_bounds("composer-tabs").unwrap();

        panel.update(vcx, |panel, cx| {
            panel.seed_screenshot_queued(2, 0);
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
        let ApiEvent::AskRequest { tool_call_id, .. } = &event else {
            unreachable!();
        };
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &serde_json::from_value(serde_json::json!({
                    "ev":"tool_start", "session_id": panel.session_id,
                    "call_id":tool_call_id, "name":"ask"
                })).unwrap(),
                cx,
            );
            panel.apply(&event, cx);
        });
        vcx.run_until_parked();

        assert!(vcx.debug_bounds("ask-card").is_some(), "ask card paints");
        assert!(
            vcx.debug_bounds("composer-tabs").is_some()
                && vcx.debug_bounds("session-status-ask").is_some()
                && vcx.debug_bounds("composer-text-slot").is_none(),
            "ask replaces only the text slot and leaves the composer bar visible"
        );
        assert!(vcx.debug_bounds("composer-ask-pending").is_none());
        assert!(vcx.debug_bounds("session-status-tools").is_none());
        assert!(vcx.debug_bounds("transcript-activity").is_none());
        let ask = vcx.debug_bounds("ask-card").unwrap();
        let queue = vcx
            .debug_bounds("queue-card")
            .expect("visible prompt queue");
        let bar = vcx.debug_bounds("composer-tabs").unwrap();
        assert!(queue.bottom() <= ask.top() && ask.bottom() <= bar.top());
        assert_eq!(
            bar.bottom(),
            bar_before.bottom(),
            "the bar stays anchored to the bottom"
        );
        assert!(
            vcx.debug_bounds("ask-scroll").is_some(),
            "questions have their own scroll area"
        );
        assert!(
            vcx.debug_bounds("ask-other-editor-0").is_some(),
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
        assert!(commands.try_iter().any(|command| matches!(command,
            crate::harness::Command::AskResponse { session_id, request_id: id, response, .. }
            if session_id == "session-a" && id == request_id && response.answers.as_ref().is_some_and(|answers| answers[0].other.as_deref() == Some("my own answer"))
        )), "Submit must send the typed answer through the bridge");
        let attempt_id = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().attempt_id.unwrap()
        });
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                crate::harness::Update::AskResponseSettled {
                    session_id: "session-a".into(),
                    request_id,
                    attempt_id,
                    result: Ok(()),
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
        panel.update(vcx, |panel, cx| {
            let next_tool = serde_json::from_value(serde_json::json!({
                "ev":"tool_start", "session_id":"session-a", "call_id":"next-tool", "name":"bash"
            })).unwrap();
            panel.apply(&next_tool, cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("session-status-tools").is_some());
        assert!(vcx.debug_bounds("transcript-activity").is_some());
    }

    #[gpui::test]
    fn cancelled_turn_leaves_no_ask_even_if_the_request_replays(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new("session-a".into(), None, None, crate::harness::spawn_inert(), cx)
        });
        let ask = serde_json::from_str(REAL_ASK_REQUEST).unwrap();
        panel.update(vcx, |panel, cx| panel.apply(&ask, cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("session-status-ask").is_some());

        panel.update(vcx, |panel, cx| {
            panel.apply(&ApiEvent::TurnStopped {
                session_id: "session-a".into(),
                reason: jcode_sdk::api::TurnStopReason::Interrupted,
                message: "Cancelled".into(),
                provider_stop_reason: None,
            }, cx);
            panel.apply(&ask, cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("session-status-stopped").is_some());
        assert!(vcx.debug_bounds("ask-card").is_none());
        assert!(vcx.debug_bounds("composer-text-slot").is_some());
    }
}
