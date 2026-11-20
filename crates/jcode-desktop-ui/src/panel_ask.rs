//! Inline card for the `ask` tool: the agent blocks on the user's answers.
//!
// Each question owns its free-answer editor and each selected option its comment.
use super::*;
use jcode_sdk::{AskAnswer, AskQuestion, AskResponse};

const COMMENT_ICON: &[u8] = include_bytes!("../../../assets/icons/comment.svg");

struct AskActionTooltip(&'static str);

impl gpui::Render for AskActionTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(theme.TOOL_BG)
            .text_size(px(11.))
            .text_color(theme.TEXT)
            .child(self.0)
    }
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
    tab_focus: Vec<gpui::FocusHandle>,
    option_focus: Vec<Vec<gpui::FocusHandle>>,
    confirming_decline: bool,
    pub voice_target: Option<(usize, Option<usize>)>,
    pub submitting: bool,
    pub submit_error: Option<String>,
    scroll: gpui::ScrollHandle,
}

impl AskCard {
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
            tab_focus: Vec::new(),
            option_focus: Vec::new(),
            cursor_question: 0,
            cursor_option: 0,
            confirming_decline: false,
            voice_target: (n > 0).then_some((0, None)),
            submitting: false,
            submit_error: None,
            scroll: gpui::ScrollHandle::new(),
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

#[derive(Deserialize)]
struct AskToolInput {
    questions: Vec<AskQuestion>,
}

impl Panel {
    pub(super) fn show_ask(
        &mut self,
        request_id: &str,
        tool_call_id: &str,
        questions: &[AskQuestion],
        cx: &mut Context<Self>,
    ) {
        if self.answered_asks.contains(request_id) {
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
        let mut card = AskCard::new(request_id.to_owned(), questions.to_vec());
        tool_call_id.clone_into(&mut card.tool_call_id);
        self.ask_card = Some(card);
        self.prepare_ask_inputs(cx);
        if let (Some(draft), Some(card)) = (self.pending_ask_draft.take(), self.ask_card.as_mut()) {
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
            if name != "ask"
                || call_id.is_empty()
                || !seen.insert(call_id.as_str())
                || *done
                || self.answered_asks.contains(&format!("ask-{call_id}"))
            {
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

    pub(crate) fn capture_focused_ask_voice_target(&mut self, window: &Window, cx: &App) -> bool {
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        for (q, input) in card.other_inputs.iter().enumerate() {
            if input.read(cx).focus_handle.is_focused(window) {
                card.voice_target = Some((q, None));
                return true;
            }
        }
        for (q, inputs) in card.comment_inputs.iter().enumerate() {
            for (o, input) in inputs.iter().enumerate() {
                if input.read(cx).focus_handle.is_focused(window) {
                    card.voice_target = Some((q, Some(o)));
                    return true;
                }
            }
        }
        false
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
                            theme.BG.into()
                        } else {
                            theme.TEXT_DIM.into()
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
        if event.keystroke.key == "tab" || card.submitting {
            return false;
        }
        let editing = card
            .other_inputs
            .iter()
            .chain(card.comment_inputs.iter().flatten())
            .any(|input| input.read(cx).focus_handle.is_focused(window));
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
        let Some(card) = self.ask_card.as_mut() else {
            return false;
        };
        if card.confirming_decline && key != "escape" && key != "enter" {
            return true;
        }
        let q = card.cursor_question;
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
            "left" if !editing => {
                card.switch_tab((q + card.tab_count() - 1) % card.tab_count());
            }
            "right" if !editing => {
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
                if q + 1 < card.tab_count() {
                    card.switch_tab(q + 1);
                    window.focus(&self.focus_handle, cx);
                } else if card.complete() {
                    let response = card.response_with_images(cx);
                    self.answer_ask(response, cx);
                }
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

    fn submit_ask_editor(
        &mut self,
        request_id: &str,
        q: usize,
        option: Option<usize>,
        text: String,
        images: Vec<(String, String)>,
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
        // PromptInput clears itself before invoking its submit callback. Put
        // the whole draft back, including image chips, before sending so an
        // incomplete ask or a failed submission cannot discard attachments.
        let input = match option {
            Some(o) => card.comment_inputs.get(q).and_then(|inputs| inputs.get(o)),
            None => card.other_inputs.get(q),
        }
        .cloned();
        if let Some(input) = input {
            input.update(cx, |input, cx| input.recall_prompt(&text, images, cx));
        }
        if card.complete() {
            let response = card.response_with_images(cx);
            self.answer_ask(response, cx);
        } else {
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
                PromptInput::new(cx, "Your own answer…", move |text, images, _, app| {
                    let submit = submit.clone();
                    let id = submit_id.clone();
                    app.defer(move |app| {
                        let _ = submit.update(app, |panel, cx| {
                            panel.submit_ask_editor(&id, q, None, text, images, cx)
                        });
                    });
                })
                .with_compact_padding()
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
                    PromptInput::new(cx, "Add a comment…", move |text, images, _, app| {
                        let submit = submit.clone();
                        let id = submit_id.clone();
                        app.defer(move |app| {
                            let _ = submit.update(app, |panel, cx| {
                                panel.submit_ask_editor(&id, q, Some(o), text, images, cx)
                            });
                        });
                    })
                    .with_compact_padding()
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
            self.answered_asks.insert(request_id.to_owned());
            self.ask_card = None;
            self.pending_ask_draft = None;
            self.schedule_ask_recovery(cx);
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
        body = body.child(tabs);
        let mut questions = div()
            .id("ask-scroll")
            .debug_selector(|| "ask-scroll".into())
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .max_h(px((f32::from(window.viewport_size().height) * 0.4).max(60.)));
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
                    .items_start()
                    .flex()
                    .gap_2()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .rounded_lg()
                    .cursor_pointer()
                    .when(!selected, |el| el.hover(|el| el.bg(theme.INPUT_BG)))
                    .child(mark)
                    .child(
                        // The text owns the row's free width. Sized from its
                        // own measure beside a growing spacer, a stale narrow
                        // measure left it one character wide.
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .debug_selector(move || format!("ask-label-{q}-{o}"))
                                    .child(if o < 9 {
                                        format!("{}. {}", o + 1, option.label)
                                    } else {
                                        option.label.clone()
                                    }),
                            )
                            .when_some(option.description.clone(), |el, desc| {
                                el.child(
                                    div()
                                        .min_w_0()
                                        .debug_selector(move || format!("ask-description-{q}-{o}"))
                                        .text_color(theme.TEXT_DIM)
                                        .child(desc),
                                )
                            }),
                    )
                    .child(
                        crate::keyboard::button(div().id(("ask-comment-button", q * 16 + o)))
                            .debug_selector(move || format!("ask-comment-button-{q}-{o}"))
                            .flex_none()
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
                            .tooltip(|_, cx| cx.new(|_| AskActionTooltip("Comment")).into())
                            .child(gpui::svg().data(COMMENT_ICON).size(px(14.)).text_color(
                                if has_comment {
                                    theme.ACCENT
                                } else {
                                    theme.TEXT
                                },
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(card) = this.ask_card.as_mut() {
                                    card.cursor_option = o;
                                    card.open_comment(q, o);
                                    if let Some(input) =
                                        card.comment_inputs.get(q).and_then(|inputs| inputs.get(o))
                                    {
                                        let focus = input.read(cx).focus_handle.clone();
                                        window.focus(&focus, cx);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            })),
                    )
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
                        .when(
                            self.focus_handle.is_focused(window) && card.cursor_option == o,
                            |el| el.border_1().border_color(theme.ACCENT),
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
                                    .child(div().flex_1().min_w_0().child(input.clone()))
                                    .child(self.render_ask_microphone(q, Some(o), cx)),
                            )
                        }),
                );
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
                let text = if values.is_empty() {
                    "Not answered".to_string()
                } else {
                    values.join(", ")
                };
                let mut summary = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!("{}. {}", q + 1, question.question))
                    .child(div().text_color(theme.TEXT_DIM).child(text));
                for &o in &card.chosen[q] {
                    let comment = card.comments[q][o].trim();
                    if !comment.is_empty() {
                        summary = summary.child(
                            div()
                                .debug_selector(move || format!("ask-review-comment-{q}-{o}"))
                                .text_color(theme.TEXT_DIM)
                                .child(format!("{}: {}", question.options[o].label, comment)),
                        );
                    }
                }
                review = review.child(summary);
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
                .child(crate::transcript_scroll::tracked_vertical(questions, &card.scroll))
                .when(card.submitting, |el| {
                    el.child(div().absolute().inset_0().occlude())
                }),
        );
        let complete = card.complete();
        let last_tab = card.cursor_question + 1 >= card.tab_count();
        Some(
            body.child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        crate::keyboard::button(div().id("ask-decline"))
                            .debug_selector(|| "ask-decline".into())
                            .px_3()
                            .py_1()
                            .rounded_full()
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
                            .when(complete || !last_tab, crate::keyboard::button)
                            .debug_selector(|| "ask-submit".into())
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .when(complete || !last_tab, |el| {
                                el.cursor_pointer()
                                    .bg(theme.ACCENT_DIM)
                                    .text_color(theme.TEXT)
                            })
                            .when(!complete && last_tab, |el| el.text_color(theme.TEXT_FAINT))
                            .child(if card.submitting {
                                "Sending…"
                            } else if !last_tab {
                                "Next"
                            } else {
                                "Submit"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(card) = this.ask_card.as_mut()
                                    && card.cursor_question + 1 < card.tab_count()
                                {
                                    card.switch_tab(card.cursor_question + 1);
                                    window.focus(&this.focus_handle, cx);
                                    cx.notify();
                                    return;
                                }
                                if let Some(response) = this
                                    .ask_card
                                    .as_ref()
                                    .filter(|card| card.complete())
                                    .map(|card| card.response_with_images(cx))
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
        assert!(
            label.size.width > px(150.),
            "the label keeps its width: {:?} of {:?}",
            label.size.width,
            row.size.width
        );
        assert!(description.size.width > px(150.));
        assert!(label.size.height < px(40.), "the label stays on one line");
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
                panel.ask_card.as_ref().is_none_or(|card| card.submitting),
                "Enter on the only question submits it"
            );
        });
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
        let second = value["questions"][0].clone();
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
        vcx.simulate_keystrokes("left right n");
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
        vcx.simulate_keystrokes("1 left");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.chosen[0], vec![1]);
            assert_eq!(card.chosen[1], vec![0]);
            assert_eq!(card.comments[0][1], "comment");
            assert_eq!(card.comments[1][0], "second comment");
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
        let footer = vcx.debug_bounds("ask-submit").unwrap();
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
        let last = vcx.debug_bounds("ask-other-0").unwrap();
        assert!(
            last.top() >= viewport.top() && last.bottom() <= viewport.bottom() + px(1.),
            "last question must be reachable: {last:?}, {viewport:?}"
        );
        assert_eq!(
            vcx.debug_bounds("ask-submit"),
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
        let input = serde_json::json!({"questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]});
        let history: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
            {"role":"user","content":"Choose"},
            {"role":"tool","content":"1. Which? -> B (comment: reason)","tool":{"id":"ask-1","name":"ask","input":input}}
        ])).unwrap();
        let image = jcode_sdk::RenderedImage {
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
            panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "unrelated".into(), name: "ask".into(), output: "cancelled".into(), error: Some("cancelled".into()) }, cx);
            assert!(panel.ask_card.is_some());
            panel.apply(&jcode_sdk::ApiEvent::ToolDone { session_id: "session-a".into(), call_id: "toolu_01J29QR2NCg12HueHJkMWtZU".into(), name: "ask".into(), output: "cancelled by the user".into(), error: Some("cancelled by the user".into()) }, cx);
            assert!(panel.ask_card.is_none(), "completed tool must close its card");
            let Item::Tool { input, output, error, .. } = panel.items.iter().find(|item| matches!(item, Item::Tool { call_id, .. } if call_id == "toolu_01J29QR2NCg12HueHJkMWtZU")).unwrap() else { unreachable!() };
            assert!(result_text(input, output, error.as_deref()).contains("Which database"));
        });
    }

    #[gpui::test]
    fn ask_comment_image_survives_draft_and_enter(cx: &mut gpui::TestAppContext) {
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
        vcx.simulate_keystrokes("enter");
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
        let submit = vcx.debug_bounds("ask-submit").unwrap().center();
        vcx.simulate_click(submit, gpui::Modifiers::default());
        vcx.run_until_parked();
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
            panel.ask_response_settled(&request_id, Some("network error".into()), cx);
        });
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert!(!card.submitting);
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
            assert!(!card.submitting);
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
            assert!(!card.submitting);
        });
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(panel.ask_card.as_ref().unwrap().submitting)
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
            panel.ask_response_settled(request_id, None, cx);
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
        panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        vcx.run_until_parked();

        assert!(vcx.debug_bounds("ask-card").is_some(), "ask card paints");
        assert!(
            vcx.debug_bounds("composer-tabs").is_some()
                && vcx.debug_bounds("composer-ask-pending").is_some()
                && vcx.debug_bounds("composer-text-slot").is_none(),
            "ask replaces only the text slot and leaves the composer bar visible"
        );
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
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        let request_id = panel.read_with(vcx, |panel, _| {
            panel.ask_card.as_ref().unwrap().request_id.clone()
        });
        assert!(commands.try_iter().any(|command| matches!(command,
            crate::harness::Command::AskResponse { session_id, request_id: id, response }
            if session_id == "session-a" && id == request_id && response.answers.as_ref().is_some_and(|answers| answers[0].other.as_deref() == Some("my own answer"))
        )), "Submit must send the typed answer through the bridge");
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
