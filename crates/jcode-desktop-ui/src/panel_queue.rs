//! Prompts explicitly queued by Ctrl+Enter wait for the current turn to finish.
use super::*;

#[path = "panel_auto_poke.rs"]
mod auto_poke;
#[path = "queue_card.rs"]
mod card;
pub(in crate::panel) use auto_poke::{PokeCommand, PokeText};

/// Prompt attachments on the wire and back as submitted files.
pub(super) struct Files;

impl Files {
    pub(super) fn wire(files: &[crate::input::SubmittedFile]) -> Vec<jcode_sdk::api::Attachment> {
        files.iter().map(|file| jcode_sdk::api::Attachment::File {
            name: file.name.clone(),
            mime: file.mime.clone(),
            size: file.size,
            data: Some(file.data.clone()),
            path: None,
        }).collect()
    }

    pub(super) fn restorable(attachments: &[jcode_sdk::api::Attachment]) -> Option<Vec<crate::input::SubmittedFile>> {
        attachments.iter().map(|attachment| match attachment {
            jcode_sdk::api::Attachment::File { name, mime, size, data: Some(data), path: None } => Some(crate::input::SubmittedFile {
                name: name.clone(), mime: mime.clone(), size: *size, data: data.clone(),
            }),
            _ => None,
        }).collect()
    }

    /// Files a local queue entry can hold: every attachment with inline bytes.
    pub(super) fn captured(attachments: &[jcode_sdk::api::Attachment]) -> Vec<crate::input::SubmittedFile> {
        attachments
            .iter()
            .filter_map(|attachment| match attachment {
                jcode_sdk::api::Attachment::File { name, mime, size, data: Some(data), .. } => {
                    Some(crate::input::SubmittedFile { name: name.clone(), mime: mime.clone(), size: *size, data: data.clone() })
                }
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptQueue {
    // Never resume automatic work merely by restoring a window.
    #[serde(skip)]
    auto_poke: auto_poke::AutoPoke,
    prompts: Vec<QueuedPrompt>,
    #[serde(default)]
    pub(super) paused: bool,
    // Retry state must not survive a window restore or a user stop.
    #[serde(skip)]
    resume_after_retry: bool,
    #[serde(default)]
    pub(super) waiting_for_connection: bool,
    /// Prompts sent mid-turn as steering. The runtime holds them until the
    /// model can see them, so they show above the composer, not in the
    /// transcript, until `soft_interrupt_injected` says they were delivered.
    #[serde(default)]
    steered: Vec<QueuedPrompt>,
    /// Injection ids already observed on this connection, for replayed events.
    #[serde(skip)]
    delivered_ids: std::collections::HashSet<String>,
    /// Durable server state plus submissions not yet acknowledged in a snapshot.
    #[serde(skip)]
    remote: Vec<jcode_sdk::api::QueuedPrompt>,
    #[serde(skip)]
    remote_supported: bool,
    #[serde(skip)]
    remote_mutations_supported: bool,
    #[serde(skip)]
    remote_removing: std::collections::HashSet<String>,
    #[serde(skip)]
    queued_edit: Option<QueuedPromptEditState>,
    #[serde(skip)]
    remote_editing_supported: bool,
    #[serde(default)]
    pending_remote: Vec<jcode_sdk::api::QueuedPrompt>,
    #[serde(skip)]
    pub(super) replay_pending_on_connect: bool,
    /// Count matching history rows at submission, for lost-ack reconciliation.
    #[serde(default)]
    pending_remote_history_counts: std::collections::HashMap<String, usize>,
    #[serde(default)]
    remote_failures: std::collections::HashMap<String, String>,
    /// The user asked for "Send all now" while the session was reconnecting.
    /// The intent survives the outage and runs as soon as the queue is back,
    /// instead of leaving a permanent error on screen.
    #[serde(skip)]
    send_all_on_connect: bool,
    #[serde(skip)]
    send_now_generation: u64,
    /// The latest Send now request that names each prompt. Its escalation
    /// timer owns the prompt; an older timer for the same prompt stands down.
    #[serde(skip)]
    send_now_pending: std::collections::HashMap<String, u64>,
    #[serde(default)]
    default_after_yield: bool,
    #[serde(skip)]
    pub(in crate::panel) card_menu: Option<String>,
    #[serde(skip)]
    card_expanded: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct QueuedPrompt {
    content: String,
    images: Vec<(String, String)>,
    #[serde(default)]
    files: Vec<crate::input::SubmittedFile>,
    #[serde(default)]
    active_skill: Option<String>,
    #[serde(default = "default_follow_up")]
    after_yield: bool,
    /// Sent with plain Enter over a visible queue: deliver this prompt now,
    /// as soon as it can reach the runtime, without promoting the others.
    #[serde(default)]
    send_now: bool,
}



fn default_follow_up() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq)]
enum QueueEditTarget {
    Local(usize),
    Remote(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum QueueEditPhase {
    Editing,
    Saving,
    /// The daemon could not take the edit. The text stays in the editor with
    /// this neutral note, so nothing typed is lost.
    Recovered(&'static str),
}

/// One queued message open for editing inside its own queue row.
#[derive(Clone, Debug, PartialEq)]
struct QueuedPromptEditState {
    target: QueueEditTarget,
    input: Entity<PromptInput>,
    phase: QueueEditPhase,
    active_skill: Option<String>,
    after_yield: bool,
}


impl Panel {
    pub(crate) fn set_queue_mutations_supported(
        &mut self,
        supported: bool,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.remote_mutations_supported = supported;
        cx.notify();
    }

    pub(crate) fn set_queue_editing_supported(&mut self, supported: bool, cx: &mut Context<Self>) {
        self.prompt_queue.remote_editing_supported = supported;
        cx.notify();
    }

    /// The inline queue editor has keyboard focus.
    pub(super) fn queue_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.prompt_queue
            .queued_edit
            .as_ref()
            .is_some_and(|edit| edit.input.read(cx).focus_handle.is_focused(window))
    }

    /// Esc in the inline editor first closes an attachment preview it opened.
    pub(super) fn queue_editor_has_preview(&self, cx: &App) -> bool {
        self.prompt_queue
            .queued_edit
            .as_ref()
            .is_some_and(|edit| edit.input.read(cx).has_open_preview())
    }

    /// Up in an empty composer opens the newest queued message for editing in
    /// its own row. The composer keeps focus-free and its draft untouched.
    pub(super) fn queue_recall_prev(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.prompt_queue.queued_edit.is_some()
            || !self.input.read(cx).focus_handle.is_focused(window)
            || !self.input.read(cx).content.trim().is_empty()
        {
            return false;
        }
        if !self.prompt_queue.prompts.is_empty() {
            self.edit_local_queued_prompt(self.prompt_queue.prompts.len() - 1, window, cx);
        } else if self.prompt_queue.remote_editing_supported && !self.prompt_queue.remote.is_empty() {
            self.begin_queued_prompt_edit(self.prompt_queue.remote.len() - 1, window, cx);
        } else {
            return false;
        }
        true
    }

    /// Open a daemon-queued message for editing in its row. The daemon holds
    /// it so it cannot be delivered mid-edit; it keeps its slot and mode.
    pub(super) fn begin_queued_prompt_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt_queue.remote.get(index).cloned() else { return };
        let after_yield = prompt.mode == jcode_sdk::api::QueuedPromptMode::AfterYield;
        self.open_queue_editor(
            QueueEditTarget::Remote(prompt.client_id),
            (prompt.content, prompt.images, prompt.attachments),
            (prompt.active_skill, after_yield),
            Some(window),
            cx,
        );
    }

    /// Open a locally held message for editing in its row. It stays in the
    /// queue at the same index; the local queue waits until the edit ends.
    pub(super) fn edit_local_queued_prompt(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt_queue.prompts.get(index).cloned() else { return };
        self.open_queue_editor(
            QueueEditTarget::Local(index),
            (prompt.content, prompt.images, Files::wire(&prompt.files)),
            (prompt.active_skill, prompt.after_yield),
            Some(window),
            cx,
        );
    }

    #[allow(clippy::type_complexity)]
    fn open_queue_editor(
        &mut self,
        target: QueueEditTarget,
        (content, images, attachments): (String, Vec<(String, String)>, Vec<jcode_sdk::api::Attachment>),
        (active_skill, after_yield): (Option<String>, bool),
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.prompt_queue.queued_edit.is_some() {
            return;
        }
        let input = cx.new(|cx| PromptInput::queued_editor(content, images, attachments, cx));
        if let Some(window) = window {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
            // Bring the row into the card's view once it has been laid out.
            let reveal = self.queue_edit_anchor.clone();
            window.on_next_frame(move |window, cx| reveal.scroll_to(window, cx));
        }
        if let QueueEditTarget::Remote(prompt_id) = &target {
            self.set_queue_edit_hold(prompt_id, true);
        }
        self.prompt_queue.card_menu = None;
        self.prompt_queue.card_expanded = None;
        self.prompt_queue.queued_edit = Some(QueuedPromptEditState {
            target,
            input,
            phase: QueueEditPhase::Editing,
            active_skill,
            after_yield,
        });
        cx.notify();
    }

    /// Enter or Save: replace the message in place, applying its chosen mode.
    pub(super) fn save_queued_prompt_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.prompt_queue.queued_edit.as_mut() else {
            return;
        };
        if matches!(edit.phase, QueueEditPhase::Recovered(_)) {
            self.requeue_recovered_edit(window, cx);
            return;
        }
        if edit.phase != QueueEditPhase::Editing {
            return;
        }
        let (content, images, attachments) = edit.input.read(cx).queued_edit_payload();
        if content.trim().is_empty() && images.is_empty() && attachments.is_empty() {
            return;
        }
        let after_yield = edit.after_yield;
        match edit.target.clone() {
            QueueEditTarget::Remote(prompt_id) => {
                edit.phase = QueueEditPhase::Saving;
                let mode = if after_yield {
                    jcode_sdk::api::QueuedPromptMode::AfterYield
                } else {
                    jcode_sdk::api::QueuedPromptMode::Steer
                };
                // The ordered session worker applies the mode while the edit still
                // holds the row. EditQueuedPrompt releases that hold on save.
                if self
                    .prompt_queue
                    .remote
                    .iter()
                    .any(|prompt| prompt.client_id == prompt_id && prompt.mode != mode)
                {
                    self.set_remote_queue_mode(&prompt_id, mode);
                }
                self.bridge.send(Command::EditQueuedPrompt {
                    session_id: self.session_id.clone(),
                    prompt_id,
                    content,
                    images,
                    attachments,
                });
            }
            QueueEditTarget::Local(index) => {
                self.prompt_queue.queued_edit = None;
                if let Some(prompt) = self.prompt_queue.prompts.get_mut(index) {
                    prompt.content = content;
                    prompt.images = images;
                    prompt.after_yield = after_yield;
                    // Local entries carry their bytes, as do files pasted into
                    // the editor, so every attachment converts.
                    prompt.files = Files::captured(&attachments);
                }
                self.resume_local_queue_after_edit(cx);
            }
        }
        self.focus_input(window, cx);
        cx.notify();
    }

    /// Esc or Cancel: leave the row exactly as it was and release the hold.
    pub(super) fn cancel_queued_prompt_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(edit) = self
            .prompt_queue
            .queued_edit
            .take_if(|edit| edit.phase != QueueEditPhase::Saving)
        else {
            return false;
        };
        match edit.target {
            QueueEditTarget::Remote(id) if edit.phase == QueueEditPhase::Editing => {
                self.set_queue_edit_hold(&id, false)
            }
            QueueEditTarget::Remote(_) => {}
            QueueEditTarget::Local(_) => self.resume_local_queue_after_edit(cx),
        }
        self.focus_input(window, cx);
        cx.notify();
        true
    }

    /// The edited message could not replace the queued one (it was already
    /// delivered, or the save failed). Queue the edited text as a new message.
    fn requeue_recovered_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.queue_connection_ready() || !self.prompt_queue.remote_supported {
            return;
        }
        let Some(edit) = self.prompt_queue.queued_edit.take() else { return };
        let (content, images, attachments) = edit.input.read(cx).queued_edit_payload();
        if content.trim().is_empty() && images.is_empty() && attachments.is_empty() {
            self.prompt_queue.queued_edit = Some(edit);
            return;
        }
        self.send_remote_prompt_with_attachments(content, images, attachments, edit.after_yield, edit.active_skill, cx);
        self.focus_input(window, cx);
    }

    fn set_queue_edit_hold(&self, prompt_id: &str, editing: bool) {
        self.bridge.send(Command::SetQueuedPromptEditing {
            session_id: self.session_id.clone(),
            prompt_id: prompt_id.to_owned(),
            editing,
        });
    }

    /// The panel is closing: let the daemon deliver the message again.
    pub(crate) fn release_queue_edit_hold(&mut self) {
        if let Some(edit) = self.prompt_queue.queued_edit.take()
            && let QueueEditTarget::Remote(id) = edit.target
            && edit.phase == QueueEditPhase::Editing
        {
            self.set_queue_edit_hold(&id, false);
        }
    }

    fn resume_local_queue_after_edit(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.prompt_queue.send_all_on_connect) {
            self.interrupt_all_queued_prompts(cx);
        } else {
            self.send_queued_prompts(cx);
        }
    }

    fn local_edit_index(&self) -> Option<usize> {
        match self.prompt_queue.queued_edit.as_ref()?.target {
            QueueEditTarget::Local(index) => Some(index),
            QueueEditTarget::Remote(_) => None,
        }
    }

    /// Keep a local edit pointing at the same message after another local
    /// message left the queue.
    fn local_queue_removed(&mut self, removed: usize) {
        if let Some(QueuedPromptEditState { target: QueueEditTarget::Local(index), .. }) =
            self.prompt_queue.queued_edit.as_mut()
            && removed < *index
        {
            *index -= 1;
        }
    }

    fn local_queue_swapped(&mut self, a: usize, b: usize) {
        if let Some(QueuedPromptEditState { target: QueueEditTarget::Local(index), .. }) =
            self.prompt_queue.queued_edit.as_mut()
        {
            if *index == a {
                *index = b;
            } else if *index == b {
                *index = a;
            }
        }
    }

    /// Outcome of saving a daemon row. A rejection keeps the edited text in
    /// the row with a neutral note instead of losing it.
    pub(crate) fn queued_prompt_edited(
        &mut self,
        prompt_id: &str,
        result: Result<jcode_sdk::api::QueuedPromptEditOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        use jcode_sdk::api::QueuedPromptEditOutcome::*;
        let Some(edit) = self.prompt_queue.queued_edit.as_mut().filter(|edit| {
            edit.phase == QueueEditPhase::Saving
                && matches!(&edit.target, QueueEditTarget::Remote(id) if id == prompt_id)
        }) else {
            return;
        };
        match result {
            Ok(Edited) => self.prompt_queue.queued_edit = None,
            Ok(InFlight | AlreadyDelivered) => {
                edit.phase = QueueEditPhase::Recovered(
                    "This message was already sent, so it was not changed. Your edits are still here.",
                )
            }
            Ok(NotFound) => {
                edit.phase = QueueEditPhase::Recovered(
                    "This message is no longer in the queue. Your edits are still here.",
                )
            }
            Err(_) => {
                edit.phase = QueueEditPhase::Recovered(
                    "The change could not be confirmed. Your edits are still here.",
                );
                // The hold may still be in place if the request never landed.
                self.set_queue_edit_hold(prompt_id, false);
            }
        }
        cx.notify();
    }

    /// Keep an open editor consistent with a fresh daemon snapshot: re-take a
    /// hold the daemon dropped with a lost connection, and keep the text with
    /// a note if the message left the queue before the edit was saved.
    fn sync_queued_edit_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(QueuedPromptEditState { target: QueueEditTarget::Remote(id), phase, .. }) =
            self.prompt_queue.queued_edit.as_mut()
        else {
            return;
        };
        if *phase != QueueEditPhase::Editing {
            return;
        }
        match self.prompt_queue.remote.iter().find(|prompt| &prompt.client_id == id) {
            Some(prompt) if !prompt.editing => {
                let id = id.clone();
                self.set_queue_edit_hold(&id, true);
            }
            Some(_) => {}
            None => {
                *phase = QueueEditPhase::Recovered(
                    "This message was sent before your edit. Your edits are still here.",
                );
                cx.notify();
            }
        }
    }

    pub(crate) fn queue_removal_settled(
        &mut self,
        prompt: jcode_sdk::api::QueuedPrompt,
        edit: bool,
        result: Result<jcode_sdk::api::QueuedPromptRemovalOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.remote_removing.remove(&prompt.client_id);
        let files = if edit {
            match Files::restorable(&prompt.attachments) {
                Some(files) => files,
                None => {
                    self.items.push(Item::Error("The queued message contains unsupported attachments and could not be restored. Check the conversation before retrying.".into()));
                    cx.notify();
                    return;
                }
            }
        } else {
            Vec::new()
        };
        match result {
            Ok(jcode_sdk::api::QueuedPromptRemovalOutcome::Removed) => {
                self.prompt_queue
                    .remote
                    .retain(|p| p.client_id != prompt.client_id);
                if edit {
                    self.active_skill = prompt.active_skill.clone();
                    self.input.update(cx, |input, cx| {
                        input.recall_prompt(&prompt.content, prompt.images, cx);
                        input.append_queued_files(files, cx);
                    });
                }
            }
            Ok(outcome) => self.items.push(Item::Error(format!(
                "Could not remove queued message: {outcome:?}"
            ))),
            Err(reason) => {
                if edit {
                    self.active_skill = prompt.active_skill.clone();
                    self.input.update(cx, |input, cx| {
                        input.recall_prompt(&prompt.content, prompt.images, cx);
                        input.append_queued_files(files, cx);
                    });
                    self.items.push(Item::Error(format!("Queue removal could not be confirmed: {reason}. A copy is in the composer; check whether the original was delivered before sending.")));
                } else {
                    self.items.push(Item::Error(format!(
                        "Queue removal could not be confirmed: {reason}"
                    )));
                }
            }
        }
        cx.notify();
    }

    fn remove_remote_card_prompt(
        &mut self,
        prompt: jcode_sdk::api::QueuedPrompt,
        edit: bool,
        cx: &mut Context<Self>,
    ) {
        if edit && Files::restorable(&prompt.attachments).is_none() {
            self.items.push(Item::Error("This queued message has attachments that cannot be edited here; it remains queued.".into()));
            cx.notify();
            return;
        }
        if self.prompt_queue.remote_mutations_supported
            && self
                .prompt_queue
                .remote_removing
                .insert(prompt.client_id.clone())
        {
            self.bridge.send(Command::RemoveQueuedPrompt {
                session_id: self.session_id.clone(),
                prompt,
                edit,
            });
            self.prompt_queue.card_menu = None;
            cx.notify();
        }
    }

    fn reorder_remote_card_prompt(&mut self, index: usize, up: bool) {
        let mut order: Vec<String> = self
            .prompt_queue
            .remote
            .iter()
            .map(|p| p.client_id.clone())
            .collect();
        let other = if up {
            index.checked_sub(1)
        } else {
            (index + 1 < order.len()).then_some(index + 1)
        };
        if self.prompt_queue.remote_mutations_supported
            && let Some(other) = other
        {
            order.swap(index, other);
            self.bridge.send(Command::ReorderQueuedPrompts {
                session_id: self.session_id.clone(),
                order,
            });
        }
    }

    pub(crate) fn queue_prompt_failed(
        &mut self,
        client_id: &str,
        reason: String,
        cx: &mut Context<Self>,
    ) {
        if self
            .prompt_queue
            .pending_remote
            .iter()
            .chain(&self.prompt_queue.remote)
            .any(|prompt| prompt.client_id == client_id)
        {
            self.prompt_queue
                .remote_failures
                .insert(client_id.to_owned(), reason);
            cx.notify();
        }
    }
    pub(crate) fn default_send_after_yield(&self) -> bool {
        self.prompt_queue.default_after_yield
    }

    pub(crate) fn set_default_send_after_yield(
        &mut self,
        after_yield: bool,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.default_after_yield = after_yield;
        cx.notify();
    }

    pub(super) fn has_queued_prompts(&self) -> bool {
        !self.prompt_queue.prompts.is_empty()
            || !self.prompt_queue.remote.is_empty()
            || !self.prompt_queue.pending_remote.is_empty()
    }

    pub(crate) fn has_visible_queued_prompts(&self) -> bool {
        self.has_queued_prompts() || !self.prompt_queue.steered.is_empty()
    }

    /// Prompts handed to the runtime as steering and not yet delivered.
    #[cfg(test)]
    pub(crate) fn steered_prompt_count(&self) -> usize {
        self.prompt_queue.steered.len()
            + self
                .prompt_queue
                .remote
                .iter()
                .chain(&self.prompt_queue.pending_remote)
                .filter(|prompt| prompt.mode == jcode_sdk::api::QueuedPromptMode::Steer)
                .count()
    }

    /// True when a send goes to a turn that is already running, so the model
    /// only sees it at its next safe point. Before a turn is acknowledged the
    /// prompt still echoes at once, as it always did.
    fn send_is_steering(&self) -> bool {
        self.activity_active()
    }

    /// Delivered: the steered prompts become ordinary user messages.
    fn deliver_steered_prompts(&mut self) {
        for prompt in std::mem::take(&mut self.prompt_queue.steered) {
            self.push_user_prompt(prompt.content, prompt.images);
        }
    }

    fn push_user_prompt(&mut self, content: String, images: Vec<(String, String)>) {
        let index = self.items.len();
        self.items.push(Item::User(content));
        self.stamp_user(index);
        self.items.extend(
            images.into_iter().map(|(media_type, data)| {
                Item::Image(TranscriptImage::new(media_type, data, None))
            }),
        );
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
    }

    fn observe_steered_prompts(&mut self, event: &ApiEvent) {
        if self.prompt_queue.steered.is_empty() {
            return;
        }
        match event {
            // An acceptance ack only proves the daemon took some `message`
            // command. Our text left as a steering interrupt, so the ack says
            // nothing about it: it stays queued until an injection reports it.
            // The runtime drains its whole interrupt queue when it injects, and
            // user messages carry no display role.
            ApiEvent::SoftInterruptInjected {
                display_role: None, ..
            } => {
                // Output before the injection belongs above the prompt.
                self.flush_reasoning();
                self.flush_streaming();
                self.deliver_steered_prompts();
            }
            // An ended or failed turn is not a delivery receipt. Keep the
            // unconfirmed text visible and persisted for an explicit resend.
            _ => {}
        }
    }

    /// Screenshot fixture: `JCODE_DESKTOP_SCREENSHOT_QUEUED=N` seeds N messages
    /// the runtime has not delivered yet, so the queued rows can be looked at
    /// offline in the real app. `round` labels later seeds, because the demo
    /// turn delivers an earlier round into the transcript and identical text in
    /// both places would read as a duplication bug.
    pub(crate) fn seed_screenshot_queued(&mut self, count: usize, round: usize) {
        const SAMPLES: [&str; 3] = [
            "also update the changelog before you finish",
            "and re-run the desktop tests after that",
            "ping me when the build is green",
        ];
        self.prompt_queue.remote_mutations_supported = true;
        self.prompt_queue.remote_editing_supported = true;
        for index in 0..count {
            let sample = SAMPLES[index % SAMPLES.len()];
            self.prompt_queue.remote.push(jcode_sdk::api::QueuedPrompt {
                active_skill: None,
                client_id: format!("screenshot-{round}-{index}"),
                content: if round == 0 {
                    sample.to_string()
                } else {
                    format!("{sample} (round {})", round + 1)
                },
                images: Vec::new(),
                attachments: Vec::new(),
                mode: if index % 2 == 0 {
                    jcode_sdk::api::QueuedPromptMode::Steer
                } else {
                    jcode_sdk::api::QueuedPromptMode::AfterYield
                },
                editing: false,
            });
        }
    }

    pub(super) fn seed_screenshot_expanded_queue_image(&mut self, image: (String, String)) {
        self.prompt_queue.remote.push(jcode_sdk::api::QueuedPrompt {
            active_skill: None,
            client_id: "screenshot-image".into(),
            content: "[image]".into(),
            images: vec![image],
            attachments: vec![],
            mode: jcode_sdk::api::QueuedPromptMode::Steer,
            editing: false,
        });
        self.prompt_queue.card_expanded = Some("remote-screenshot-image".into());
    }

    /// Screenshot fixture: open the first seeded row in its inline editor,
    /// with a multi-line message, a screenshot, a file sent by path and a
    /// file sent inline, as a user would see it while editing.
    pub(crate) fn seed_screenshot_queue_edit(&mut self, cx: &mut Context<Self>) {
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAABgAAAAQCAIAAACDRijCAAAAKElEQVR42mN4tioADbnlnUBDxKhhGMYGkacNU81wNmg0HY2mIyoYBADRwYUfM0edPAAAAABJRU5ErkJggg==";
        let Some(prompt) = self.prompt_queue.remote.first_mut() else { return };
        prompt.content = "Compare the layout in [#image 1] with the spec, then:\n- fix the spacing in the header\n- keep the footer as it is\n- check the log for the crash".into();
        prompt.images = vec![("image/png".into(), PNG.into())];
        prompt.attachments = vec![
            jcode_sdk::api::Attachment::File {
                name: "crash.log".into(),
                mime: "text/plain".into(),
                size: 48_213,
                data: None,
                path: Some("/tmp/crash.log".into()),
            },
            jcode_sdk::api::Attachment::File {
                name: "spec.md".into(),
                mime: "text/markdown".into(),
                size: 11,
                data: Some("IyBTcGVjCgpPSwo=".into()),
                path: None,
            },
        ];
        let prompt = prompt.clone();
        let after_yield = prompt.mode == jcode_sdk::api::QueuedPromptMode::AfterYield;
        self.open_queue_editor(
            QueueEditTarget::Remote(prompt.client_id),
            (prompt.content, prompt.images, prompt.attachments),
            (prompt.active_skill, after_yield),
            None,
            cx,
        );
    }

    pub(super) fn pause_queue_for_stop(&mut self) {
        self.prompt_queue.paused = true;
        self.prompt_queue.resume_after_retry = false;
        // Keep the session's /poke on|off choice; only end this cycle.
        self.prompt_queue.auto_poke.stop();
    }

    pub(crate) fn submit_or_queue(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        queued: bool,
        cx: &mut Context<Self>,
    ) {
        self.submit_or_queue_with_files(content, images, Vec::new(), queued, cx);
    }

    pub(crate) fn submit_or_queue_with_files(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        files: Vec<crate::input::SubmittedFile>,
        queued: bool,
        cx: &mut Context<Self>,
    ) {
        self.request_deferred_session(cx);
        if self.active_skill.is_some() && !self.prompt_queue.remote_supported {
            if !self.is_pending_session()
                && self.history_loaded
                && !self.activity_active()
                && self.pending_users.is_empty()
                && !self.has_visible_queued_prompts()
                && !self.prompt_queue.waiting_for_connection
            {
                self.send_composer_prompt_with_files(content, images, files, cx);
            } else {
                self.prompt_queue.prompts.push(QueuedPrompt {
                    content,
                    images,
                    files,
                    active_skill: self.active_skill.clone(),
                    after_yield: true,
                    send_now: false,
                });
                self.prompt_queue.paused = false;
                self.send_queued_prompts(cx);
            }
            cx.notify();
            return;
        }
        if !queued && self.has_visible_queued_prompts() {
            if content.trim().is_empty() && images.is_empty() && files.is_empty() {
                // Enter on an empty composer is the keyboard Send all now.
                self.interrupt_all_queued_prompts(cx);
            } else {
                // Enter with text sends that text now. Everything already
                // queued keeps its mode: an after-yield prompt still waits
                // for the turn to yield.
                self.send_prompt_now(content, images, files, cx);
            }
            cx.notify();
            return;
        }
        if self.prompt_queue.remote_supported
            && !self.is_pending_session()
            && !self.prompt_queue.waiting_for_connection
        {
            self.send_remote_prompt_with_files(
                content,
                images,
                files,
                queued || self.prompt_queue.default_after_yield,
                self.active_skill.clone(),
                cx,
            );
            return;
        }
        let waiting_for_bootstrap = !self.prompt_queue.remote_supported && !self.history_loaded;
        if self.is_pending_session()
            || self.prompt_queue.waiting_for_connection
            || waiting_for_bootstrap
            || (queued && (self.activity_active() || !self.pending_users.is_empty()))
        {
            self.prompt_queue.waiting_for_connection |=
                self.is_pending_session() || waiting_for_bootstrap;
            self.prompt_queue.prompts.push(QueuedPrompt {
                content,
                images,
                files,
                active_skill: self.active_skill.clone(),
                after_yield: queued || self.prompt_queue.default_after_yield,
                send_now: false,
            });
            self.prompt_queue.paused = false;
            self.send_queued_prompts(cx);
        } else {
            self.send_composer_prompt_with_files(content, images, files, cx);
        }
        cx.notify();
    }

    fn set_remote_queue_mode(&self, client_id: &str, mode: jcode_sdk::api::QueuedPromptMode) {
        self.bridge.send(Command::SetQueuedPromptMode {
            session_id: self.session_id.clone(),
            client_id: client_id.to_owned(),
            mode,
        });
    }

    fn queued_remote_sendable(&self, prompt: &jcode_sdk::api::QueuedPrompt) -> bool {
        !prompt.editing
            && !self.prompt_queue.remote_removing.contains(&prompt.client_id)
            && !self.prompt_queue.queued_edit.as_ref().is_some_and(|edit| {
                matches!(&edit.target, QueueEditTarget::Remote(id) if *id == prompt.client_id)
            })
    }

    fn send_remote_queue_row(
        &mut self,
        client_id: &str,
        mode: jcode_sdk::api::QueuedPromptMode,
        cx: &mut Context<Self>,
    ) {
        use jcode_sdk::api::QueuedPromptMode::{AfterYield, Steer};
        if !self.queue_connection_ready()
            || !self.prompt_queue.remote.iter().any(|prompt| {
                prompt.client_id == client_id
                    && prompt.mode == mode
                    && self.queued_remote_sendable(prompt)
            })
        {
            return;
        }
        self.prompt_queue.remote_failures.remove(client_id);
        match mode {
            AfterYield => self.set_remote_queue_mode(client_id, Steer),
            Steer => self.bridge.send(Command::InterruptQueuedPrompt {
                session_id: self.session_id.clone(),
                client_id: client_id.to_owned(),
            }),
        }
        cx.notify();
    }

    fn send_local_queue_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.local_edit_index() == Some(index) || !self.queue_connection_ready() {
            return;
        }
        let Some(prompt) = self.prompt_queue.prompts.get_mut(index) else {
            return;
        };
        if prompt.after_yield {
            prompt.after_yield = false;
            cx.notify();
        } else {
            self.send_queued_prompt_now(index, cx);
        }
    }

    fn interrupt_queue_section(
        &mut self,
        mode: jcode_sdk::api::QueuedPromptMode,
        cx: &mut Context<Self>,
    ) {
        if !self.queue_connection_ready() {
            return;
        }
        let mut ids = Vec::new();
        for prompt in self
            .prompt_queue
            .remote
            .iter()
            .chain(&self.prompt_queue.pending_remote)
        {
            if prompt.mode == mode
                && self.queued_remote_sendable(prompt)
                && !ids.contains(&prompt.client_id)
            {
                ids.push(prompt.client_id.clone());
            }
        }
        let after_yield = mode == jcode_sdk::api::QueuedPromptMode::AfterYield;
        let mut index = 0;
        while let Some(prompt) = self.prompt_queue.prompts.get(index) {
            if prompt.after_yield != after_yield || self.local_edit_index() == Some(index) {
                index += 1;
                continue;
            }
            if self.prompt_queue.remote_supported {
                let prompt = self.prompt_queue.prompts.remove(index);
                self.local_queue_removed(index);
                ids.push(self.send_remote_prompt_with_files(
                    prompt.content,
                    prompt.images,
                    prompt.files,
                    prompt.after_yield,
                    prompt.active_skill,
                    cx,
                ));
            } else {
                let before = self.prompt_queue.prompts.len();
                self.send_queued_prompt_now(index, cx);
                index += usize::from(self.prompt_queue.prompts.len() == before);
            }
        }
        self.interrupt_queued_prompts(ids, cx);
    }

    fn interrupt_all_queued_prompts(&mut self, cx: &mut Context<Self>) {
        // A reconnect is transient, so an unavailable transport is not an
        // error: remember the intent and deliver once the queue is back.
        if self.is_pending_session() || self.queue_transport_lost() {
            self.prompt_queue.send_all_on_connect = true;
            cx.notify();
            return;
        }
        // "Send all now" is an explicit resume of a stopped queue.
        self.prompt_queue.paused = false;
        // Local prompts must reach the daemon queue before the batch
        // interrupt can name them. The batch covers prompts marked to send
        // now, so the flush must not interrupt them separately.
        if !self.prompt_queue.prompts.is_empty() {
            self.flush_local_queue(cx);
        }
        if !self.prompt_queue.prompts.is_empty() {
            // Still held locally (history not loaded yet): finish on reconnect.
            self.prompt_queue.send_all_on_connect = true;
            cx.notify();
            return;
        }
        let mut ids = Vec::new();
        for prompt in self
            .prompt_queue
            .remote
            .iter()
            .chain(&self.prompt_queue.pending_remote)
        {
            if !ids.contains(&prompt.client_id) {
                ids.push(prompt.client_id.clone());
            }
        }
        self.prompt_queue.send_all_on_connect = false;
        if ids.is_empty() {
            // Nothing daemon-owned to interrupt: a legacy runtime already got
            // the prompts through the ordinary send above.
            cx.notify();
            return;
        }
        self.interrupt_queued_prompts(ids, cx);
    }

    /// Plain Enter over a visible queue: deliver this prompt now. Prompts
    /// already queued keep their mode, so an after-yield prompt still waits
    /// for the running turn to yield.
    fn send_prompt_now(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        files: Vec<crate::input::SubmittedFile>,
        cx: &mut Context<Self>,
    ) {
        let reachable = !self.is_pending_session()
            && !self.prompt_queue.waiting_for_connection
            && !self.queue_transport_lost();
        if reachable && self.prompt_queue.remote_supported {
            let id = self.send_remote_prompt_with_files(content, images, files, true, self.active_skill.clone(), cx);
            self.interrupt_queued_prompts(vec![id], cx);
        } else if reachable && self.history_loaded && self.active_skill.is_none() {
            // A legacy runtime has no daemon queue to name a prompt in. A
            // direct send steers the running turn, which is its Send now.
            self.prompt_queue.paused = false;
            self.send_prompt_with_skill_files(content, images, files, None, cx);
        } else {
            // Not deliverable yet: hold it locally, marked to be sent now as
            // soon as the daemon owns it.
            self.prompt_queue.waiting_for_connection |= self.is_pending_session();
            self.prompt_queue.prompts.push(QueuedPrompt {
                content,
                images,
                files,
                active_skill: self.active_skill.clone(),
                after_yield: true,
                send_now: true,
            });
            self.prompt_queue.paused = false;
            self.send_queued_prompts(cx);
        }
    }

    /// Interrupt exactly these daemon-owned prompts, and escalate to a turn
    /// cancel if they are still blocked two seconds later.
    fn interrupt_queued_prompts(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.prompt_queue.remote_failures.remove(id);
        }
        self.prompt_queue.send_now_generation =
            self.prompt_queue.send_now_generation.wrapping_add(1);
        let generation = self.prompt_queue.send_now_generation;
        for id in &ids {
            self.prompt_queue.send_now_pending.insert(id.clone(), generation);
        }
        let pending_ids = ids.clone();
        self.bridge.send(Command::InterruptQueuedPrompts {
            session_id: self.session_id.clone(),
            ids,
        });
        cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
            let _ = panel.update(cx, |this, cx| {
                let owned: Vec<_> = pending_ids
                    .into_iter()
                    .filter(|id| this.prompt_queue.send_now_pending.get(id) == Some(&generation))
                    .collect();
                for id in &owned {
                    this.prompt_queue.send_now_pending.remove(id);
                }
                let remaining: Vec<_> = owned.into_iter().filter(|id| {
                    !this.prompt_queue.delivered_ids.contains(id)
                        && !this.prompt_queue.remote_failures.contains_key(id)
                        && this.prompt_queue.remote.iter().chain(&this.prompt_queue.pending_remote).any(|p| &p.client_id == id)
                }).collect();
                if !remaining.is_empty() {
                    let reason = "Delivery is still blocked. Escalating cancellation; retry Send now if it remains queued.".to_string();
                    for id in remaining { this.prompt_queue.remote_failures.insert(id, reason.clone()); }
                    this.bridge.send(Command::Cancel { session_id: this.session_id.clone() });
                    cx.notify();
                }
            });
        }).detach();
        cx.notify();
    }

    fn send_remote_prompt_with_files(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        files: Vec<crate::input::SubmittedFile>,
        after_yield: bool,
        active_skill: Option<String>,
        cx: &mut Context<Self>,
    ) -> String {
        self.send_remote_prompt_with_attachments(content, images, Files::wire(&files), after_yield, active_skill, cx)
    }

    /// Hand a prompt to the daemon queue and return its client id.
    fn send_remote_prompt_with_attachments(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
        after_yield: bool,
        active_skill: Option<String>,
        cx: &mut Context<Self>,
    ) -> String {
        if !self.activity_active() {
            self.prompt_queue.paused = false;
            self.prompt_queue.auto_poke.start(self.items.len());
        }
        self.prepare_prompt(&content);
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = format!(
            "desktop-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let prompt = jcode_sdk::api::QueuedPrompt {
            active_skill,
            client_id: id,
            content,
            images,
            attachments,
            mode: if after_yield {
                jcode_sdk::api::QueuedPromptMode::AfterYield
            } else {
                jcode_sdk::api::QueuedPromptMode::Steer
            },
            editing: false,
        };
        let history_count = self
            .items
            .iter()
            .filter(|item| matches!(item, Item::User(text) if text == &prompt.content))
            .count();
        self.prompt_queue
            .pending_remote_history_counts
            .insert(prompt.client_id.clone(), history_count);
        self.prompt_queue.pending_remote.push(prompt.clone());
        let id = prompt.client_id.clone();
        self.bridge.send(Command::QueuePrompt {
            session_id: self.session_id.clone(),
            prompt,
        });
        crate::sounds::play(crate::sounds::Cue::Sent, cx);
        cx.notify();
        id
    }

    fn send_composer_prompt_with_files(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        files: Vec<crate::input::SubmittedFile>,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.paused = false;
        // A new prompt means the user has moved on from what they searched for.
        self.search_terms.clear();
        self.prompt_queue.auto_poke.start(self.items.len());
        self.send_prompt_with_skill_files(content, images, files, self.active_skill.clone(), cx);
    }

    pub(super) fn send_prompt(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        self.send_prompt_with_skill(content, images, self.active_skill.clone(), cx);
    }

    fn send_prompt_with_skill(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        active_skill: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.send_prompt_with_skill_files(content, images, Vec::new(), active_skill, cx);
    }

    fn send_prompt_with_skill_files(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        files: Vec<crate::input::SubmittedFile>,
        active_skill: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let steering = self.send_is_steering();
        if let Some(skill) = active_skill {
            self.bridge.send(Command::SendSkill {
                session_id: self.session_id.clone(),
                content: content.clone(),
                images: images.clone(),
                attachments: Files::wire(&files),
                skill,
            });
        } else {
            self.bridge.send(Command::Send {
                session_id: self.session_id.clone(),
                content: content.clone(),
                images: images.clone(),
                attachments: Files::wire(&files),
            });
        }
        self.prepare_prompt(&content);
        let image_base = self.session_image_base;
        if steering {
            // Not delivered yet: show it as queued, and let the transcript
            // take it once the model has seen it.
            self.prompt_queue.steered.push(QueuedPrompt {
                content,
                images,
                files,
                active_skill: None,
                after_yield: false,
                send_now: false,
            });
            crate::sounds::play(crate::sounds::Cue::Sent, cx);
            return;
        }
        let index = self.items.len();
        self.items.push(Item::User(content));
        self.stamp_user(index);
        self.items.extend(images.into_iter().enumerate().map(|(offset, (media_type, data))| {
            let mut image = TranscriptImage::new(media_type, data, None);
            image.provisional_image_ordinal = Some(image_base.saturating_add(offset + 1));
            Item::Image(image)
        }));
        self.pending_users.push_back(index);
        crate::sounds::play(crate::sounds::Cue::Sent, cx);
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
    }

    fn prepare_prompt(&mut self, content: &str) {
        if !self.items.iter().any(|item| matches!(item, Item::User(_)))
            && custom_session_title(&self.session_id, self.title.as_ref()).is_none()
            && let Some(title) = first_prompt_title(&super::voice::tag::strip(content).0)
        {
            self.title = title.into();
        }
        if let Some(layout) = &mut self.startup_layout {
            layout.committed = true;
        }
    }

    pub(super) fn observe_prompt_queue(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        let users_before = matches!(
            event,
            ApiEvent::SoftInterruptInjected {
                content: Some(_),
                display_role: None,
                ..
            }
        )
        .then(|| {
            self.items
                .iter()
                .filter(|item| matches!(item, Item::User(_)))
                .count()
        });
        // One injection may combine prompts from this window and other clients.
        // In that case only the daemon's combined content preserves every prompt.
        let mixed_delivery = match event {
            ApiEvent::SoftInterruptInjected {
                client_ids,
                content: Some(_),
                display_role: None,
                ..
            } => client_ids.iter().any(|id| {
                !self.prompt_queue.delivered_ids.contains(id)
                    && !self
                        .prompt_queue
                        .remote
                        .iter()
                        .chain(&self.prompt_queue.pending_remote)
                        .any(|prompt| &prompt.client_id == id)
            }),
            _ => false,
        };
        let mut mixed_images = Vec::new();
        match event {
            ApiEvent::QueuedPrompts { prompts, .. } => {
                self.prompt_queue.remote_supported = true;
                self.prompt_queue.remote_failures.retain(|id, _| {
                    prompts.iter().any(|p| &p.client_id == id)
                        || self
                            .prompt_queue
                            .pending_remote
                            .iter()
                            .any(|p| &p.client_id == id)
                });
                self.prompt_queue
                    .pending_remote
                    .retain(|pending| !prompts.iter().any(|p| p.client_id == pending.client_id));
                self.prompt_queue
                    .pending_remote_history_counts
                    .retain(|id, _| {
                        self.prompt_queue
                            .pending_remote
                            .iter()
                            .any(|prompt| &prompt.client_id == id)
                    });
                self.prompt_queue.remote = prompts.clone();
                if std::mem::take(&mut self.prompt_queue.replay_pending_on_connect) {
                    for prompt in self.prompt_queue.pending_remote.clone() {
                        self.prompt_queue.remote_failures.remove(&prompt.client_id);
                        self.bridge.send(Command::QueuePrompt {
                            session_id: self.session_id.clone(),
                            prompt,
                        });
                    }
                }
                self.sync_queued_edit_snapshot(cx);
                // A fresh snapshot means the daemon is reachable again.
                self.queue_reconnected(cx);
            }
            ApiEvent::SoftInterruptInjected {
                client_ids,
                display_role: None,
                point,
                ..
            } if !client_ids.is_empty() => {
                let mut delivered = Vec::new();
                self.prompt_queue.remote.retain(|prompt| {
                    if client_ids.contains(&prompt.client_id) {
                        delivered.push(prompt.clone());
                        false
                    } else {
                        true
                    }
                });
                self.prompt_queue.pending_remote.retain(|prompt| {
                    if client_ids.contains(&prompt.client_id) {
                        if !delivered.iter().any(|p| p.client_id == prompt.client_id) {
                            delivered.push(prompt.clone());
                        }
                        false
                    } else {
                        true
                    }
                });
                if !delivered.is_empty() {
                    self.flush_reasoning();
                    self.flush_streaming();
                }
                for prompt in delivered {
                    self.prompt_queue.remote_failures.remove(&prompt.client_id);
                    // A retry after a lost acknowledgement can report a receipt
                    // already present in history restored before this event.
                    // The daemon has committed this exact input, so avoid a
                    // second local echo when the loaded transcript has it.
                    let previous = self
                        .prompt_queue
                        .pending_remote_history_counts
                        .remove(&prompt.client_id)
                        .unwrap_or(0);
                    let current = self
                        .items
                        .iter()
                        .filter(|item| matches!(item, Item::User(text) if text == &prompt.content))
                        .count();
                    let already_in_history =
                        point == "already_delivered" && self.history_loaded && current > previous;
                    if mixed_delivery {
                        mixed_images.extend(prompt.images);
                    } else if !already_in_history {
                        self.push_user_prompt(prompt.content, prompt.images);
                    }
                }
            }
            _ => {}
        }
        self.observe_steered_prompts(event);
        if let ApiEvent::SoftInterruptInjected {
            client_ids,
            content: Some(content),
            display_role: None,
            point,
            ..
        } = event
        {
            let already_seen = !client_ids.is_empty()
                && client_ids
                    .iter()
                    .all(|id| self.prompt_queue.delivered_ids.contains(id));
            let users_after = self
                .items
                .iter()
                .filter(|item| matches!(item, Item::User(_)))
                .count();
            let history_contains_retry = point == "already_delivered"
                && self
                    .items
                    .iter()
                    .any(|item| matches!(item, Item::User(text) if text == content));
            if !already_seen
                && users_before == Some(users_after)
                && !history_contains_retry
                && !content.trim().is_empty()
            {
                self.flush_reasoning();
                self.flush_streaming();
                self.push_user_prompt(content.clone(), mixed_images);
            }
            if self.prompt_queue.delivered_ids.len() > 4096 {
                self.prompt_queue.delivered_ids.clear();
            }
            self.prompt_queue
                .delivered_ids
                .extend(client_ids.iter().cloned());
        }
        self.prompt_queue.auto_poke.observe(event);
        match event {
            ApiEvent::Error { .. } | ApiEvent::TurnStopped { .. } => {
                self.prompt_queue.paused = true;
                self.prompt_queue.resume_after_retry = false;
            }
            ApiEvent::ConnectionPhase { phase, .. }
                if classify::is_retry_phase(phase) && self.prompt_queue.paused =>
            {
                self.prompt_queue.resume_after_retry = true;
            }
            ApiEvent::MessageAccepted { .. }
            | ApiEvent::TextDelta { .. }
            | ApiEvent::ReasoningDelta { .. }
            | ApiEvent::ToolStart { .. }
                if self.prompt_queue.resume_after_retry =>
            {
                // Resume only once the retry starts, not on the countdown or
                // TurnDone, which also terminates failed attempts. Hidden
                // continuation reminders may stream without MessageAccepted.
                self.prompt_queue.resume_after_retry = false;
                self.prompt_queue.paused = false;
            }
            ApiEvent::ConnectionPhase { phase, .. } if phase.is_empty() => {
                // An abandoned retry must not re-arm the queue on unrelated
                // output arriving later (or after a user's Stop).
                self.prompt_queue.resume_after_retry = false;
            }
            ApiEvent::SessionStatus { status, .. }
                if matches!(status.as_str(), "cancelled" | "canceled") =>
            {
                self.prompt_queue.paused = true;
                self.prompt_queue.resume_after_retry = false;
            }
            ApiEvent::TurnDone { .. } => {
                let panel = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = panel.update(cx, |this, cx| {
                        this.send_queued_prompts(cx);
                        this.send_auto_poke(cx);
                    });
                });
            }
            ApiEvent::SessionStatus { status, .. } if status == "idle" => {
                let panel = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = panel.update(cx, |this, cx| this.send_queued_prompts(cx));
                });
                self.queue_reconnected(cx);
            }
            ApiEvent::SessionStatus { status, .. }
                if matches!(status.as_str(), "connected" | "attached") =>
            {
                self.queue_reconnected(cx);
            }
            _ => {}
        }
    }

    /// The transport dropped. Prompts typed now are held for reconnect, and
    /// sending is pointless until the session returns.
    fn queue_transport_lost(&self) -> bool {
        self.status.starts_with("lost:") || self.status.eq_ignore_ascii_case("disconnected")
    }

    /// True while the queue is held back by the connection rather than by the
    /// user: a pending startup, a lost session, or prompts typed while the
    /// runtime was still attaching.
    pub(super) fn queue_waiting_for_connection(&self) -> bool {
        self.prompt_queue.waiting_for_connection
            || self.is_pending_session()
            || self.queue_transport_lost()
    }

    /// A per-row queue action can reach the runtime now.
    pub(super) fn queue_connection_ready(&self) -> bool {
        !self.queue_waiting_for_connection()
    }

    /// The transport is back: release prompts typed during the outage and
    /// finish a Send all now the user asked for while it was down. Nothing
    /// here reports a failure, because a reconnect is expected to happen.
    pub(super) fn queue_reconnected(&mut self, cx: &mut Context<Self>) {
        self.prompt_queue.waiting_for_connection = false;
        if self.prompt_queue.prompts.is_empty() && !self.prompt_queue.send_all_on_connect {
            return;
        }
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = panel.update(cx, |this, cx| {
                if std::mem::take(&mut this.prompt_queue.send_all_on_connect) {
                    this.interrupt_all_queued_prompts(cx);
                } else {
                    this.send_queued_prompts(cx);
                }
            });
        });
    }

    pub(super) fn send_queued_prompts(&mut self, cx: &mut Context<Self>) {
        let send_now = self.flush_local_queue(cx);
        self.interrupt_queued_prompts(send_now, cx);
    }

    /// Move deliverable local prompts to the runtime. Returns the daemon ids
    /// of prompts the user asked to send now, for the caller to interrupt.
    fn flush_local_queue(&mut self, cx: &mut Context<Self>) -> Vec<String> {
        // A message open for editing keeps its place: the local queue waits
        // until the edit is saved or cancelled, then resumes.
        if self.local_edit_index().is_some() {
            return Vec::new();
        }
        if self.prompt_queue.prompts.is_empty() {
            if !self.is_pending_session() && self.history_loaded {
                self.prompt_queue.waiting_for_connection = false;
            }
            return Vec::new();
        }
        if self.prompt_queue.paused
            || self.is_pending_session()
            || !self.history_loaded
            || self.queue_transport_lost()
        {
            return Vec::new();
        }
        if !self.prompt_queue.remote_supported {
            self.send_legacy_prompts_now(cx);
        }
        if !self.prompt_queue.remote_supported
            && self
                .prompt_queue
                .prompts
                .iter()
                .any(|prompt| prompt.active_skill.is_some())
        {
            if self.activity_active()
                || !self.pending_users.is_empty()
                || !(self.status == "idle" || self.status == "connected")
            {
                return Vec::new();
            }
            let prompt = self.prompt_queue.prompts.remove(0);
            self.prompt_queue.waiting_for_connection = false;
            self.prompt_queue.paused = false;
            self.prompt_queue.auto_poke.start(self.items.len());
            self.send_prompt_with_skill_files(prompt.content, prompt.images, prompt.files, prompt.active_skill, cx);
            cx.notify();
            return Vec::new();
        }
        if !self.prompt_queue.remote_supported
            && (!(self.status == "idle"
                || (self.prompt_queue.waiting_for_connection && self.status == "connected"))
                || self.activity_active()
                || !self.pending_users.is_empty())
        {
            return Vec::new();
        }
        let prompts = std::mem::take(&mut self.prompt_queue.prompts);
        self.prompt_queue.waiting_for_connection = false;
        if prompts.is_empty() {
            return Vec::new();
        }
        if self.prompt_queue.remote_supported {
            // Preserve independent follow-up turns when the daemon owns the
            // queue, including prompts typed before the session attached.
            let mut send_now = Vec::new();
            for prompt in prompts {
                let id = self.send_remote_prompt_with_files(
                    prompt.content,
                    prompt.images,
                    prompt.files,
                    prompt.after_yield,
                    prompt.active_skill,
                    cx,
                );
                if prompt.send_now {
                    send_now.push(id);
                }
            }
            cx.notify();
            return send_now;
        }
        // Legacy daemons have only the local queue. Keep their historical
        // behavior of combining prompts into one follow-up turn.
        let content = prompts
            .iter()
            .map(|p| p.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut images = Vec::new();
        let mut files = Vec::new();
        for prompt in prompts {
            images.extend(prompt.images);
            files.extend(prompt.files);
        }
        self.prompt_queue.paused = false;
        self.prompt_queue.auto_poke.start(self.items.len());
        self.send_prompt_with_skill_files(content, images, files, None, cx);
        cx.notify();
        Vec::new()
    }

    /// A legacy runtime has no daemon queue, so a prompt marked to send now
    /// goes out directly once the session is reachable, steering a running
    /// turn. The rest of the local queue keeps waiting for the turn to end.
    fn send_legacy_prompts_now(&mut self, cx: &mut Context<Self>) {
        let reachable = self.status == "idle"
            || self.status == "connected"
            || self.activity_active();
        if !reachable {
            return;
        }
        let mut index = 0;
        while index < self.prompt_queue.prompts.len() {
            let prompt = &self.prompt_queue.prompts[index];
            if !prompt.send_now || prompt.active_skill.is_some() {
                index += 1;
                continue;
            }
            let prompt = self.prompt_queue.prompts.remove(index);
            self.prompt_queue.waiting_for_connection = false;
            self.prompt_queue.auto_poke.start(self.items.len());
            self.send_prompt_with_skill_files(prompt.content, prompt.images, prompt.files, None, cx);
            cx.notify();
        }
    }

    /// Send one queued prompt immediately. During an active turn the harness
    /// delivers it as an urgent steer rather than waiting for the turn to end.
    pub(super) fn send_queued_prompt_now(&mut self, index: usize, cx: &mut Context<Self>) {
        if self
            .prompt_queue
            .prompts
            .get(index)
            .is_some_and(|prompt| prompt.active_skill.is_some())
            && self.activity_active()
        {
            self.items.push(Item::Error(
                "Skill prompts wait until the current turn finishes.".into(),
            ));
            cx.notify();
            return;
        }
        if index >= self.prompt_queue.prompts.len()
            || self.is_pending_session()
            || self.prompt_queue.waiting_for_connection
            || self.local_edit_index() == Some(index)
        {
            return;
        }
        let prompt = self.prompt_queue.prompts.remove(index);
        self.local_queue_removed(index);
        self.send_prompt_with_skill_files(prompt.content, prompt.images, prompt.files, prompt.active_skill, cx);
        cx.notify();
    }

    /// Everything waiting above the composer: prompts already handed to the
    /// runtime as steering, then prompts held back until the turn ends.
    pub(super) fn render_prompt_queue(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        self.render_queue_card(cx)
    }

    fn render_queued_content(
        &self,
        content: &str,
        images: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (content, _) = crate::image_chip::numbered_text(content, images.len());
        let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
        let ranges = crate::input::image_marker_ranges(&flat);
        if ranges.is_empty() {
            return div().min_w_0().truncate().child(flat).into_any_element();
        }
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .min_w_0()
            .overflow_hidden();
        let mut offset = 0;
        for (index, range) in ranges.iter().enumerate() {
            row = row.child(flat[offset..range.start].to_owned());
            let image = images
                .get(index)
                .map(|(media, data)| TranscriptImage::new(media.clone(), data.clone(), None));
            let mut chip = crate::image_chip::chip(
                image.as_ref().and_then(|image| image.preview.clone()),
                &flat[range.clone()],
                "queue-image-chip",
                "queue-image-thumbnail",
                "queue-image-label",
            )
            .id(("queue-image-chip", index))
            .flex_none()
            .cursor_pointer();
            if let Some(image) = image {
                chip = chip.on_click(cx.listener(move |this, _, window, cx| {
                    this.open_image_preview(image.clone(), window, cx);
                    cx.stop_propagation();
                }));
            }
            row = row.child(chip);
            offset = range.end;
        }
        row.child(flat[offset..].to_owned()).into_any_element()
    }

    fn render_expanded_queued_content(
        &self,
        content: &str,
        images: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (content, _) = crate::image_chip::numbered_text(content, images.len());
        let ranges = crate::input::image_marker_ranges(&content);
        let body = div()
            .debug_selector(|| "queue-expanded-content".into())
            .px_4()
            .pb_2()
            .min_w_0()
            .text_color(Theme::global().TEXT);
        if ranges.is_empty() {
            return body.child(content.into_owned()).into_any_element();
        }
        let images: Vec<_> = images
            .iter()
            .map(|(media, data)| TranscriptImage::new(media.clone(), data.clone(), None))
            .collect();
        let chips = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| {
                (
                    range.clone(),
                    content[range.clone()].to_owned(),
                    images.get(index).and_then(|image| image.preview.clone()),
                )
            })
            .collect();
        let display = crate::image_chip::mask_markers(&content, ranges.iter().cloned());
        let highlights = ranges
            .iter()
            .cloned()
            .map(|range| (range, crate::image_chip::hidden_marker()));
        let text = gpui::StyledText::new(display).with_highlights(highlights);
        let layout = text.layout().clone();
        let panel = cx.entity().downgrade();
        let text = gpui::InteractiveText::new("queue-expanded-images", text)
            .on_click(ranges, move |index, window, cx| {
                if let Some(image) = images.get(index) {
                    let _ = panel.update(cx, |panel, cx| {
                        panel.open_image_preview(image.clone(), window, cx);
                    });
                    cx.stop_propagation();
                }
            });
        body.child(
            div().relative().text_size(px(13.)).child(text).child(
                crate::image_chip::PositionedChips::with_selectors(
                    layout,
                    chips,
                    (
                        "queue-expanded-image-chip",
                        "queue-expanded-image-thumbnail",
                        "queue-expanded-image-label",
                    ),
                ),
            ),
        )
        .into_any_element()
    }
}

#[cfg(test)]
#[path = "panel_queue_tests.rs"]
mod tests;
