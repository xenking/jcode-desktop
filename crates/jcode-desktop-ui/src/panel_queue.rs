//! Prompts explicitly queued by Ctrl+Enter wait for the current turn to finish.
use super::*;

#[path = "panel_auto_poke.rs"]
mod auto_poke;
#[path = "queue_card.rs"]
mod card;
pub(in crate::panel) use auto_poke::{auto_poke_notice_text, parse_poke_command};

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
    /// Daemon prompt currently loaded into the composer for in-place editing.
    /// It stays queued (position and mode intact) but the daemon holds it so
    /// it cannot be delivered while the user edits it.
    #[serde(skip)]
    queued_edit: Option<QueuedPromptEditState>,
    /// A save whose outcome has not landed yet, so a rejection can put the
    /// edited text back instead of losing it.
    #[serde(skip)]
    queued_edit_pending: Option<QueuedPromptEditSave>,
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
    #[serde(skip)]
    send_now_error: Option<String>,
    #[serde(skip)]
    send_now_generation: u64,
    #[serde(default)]
    default_after_yield: bool,
    #[serde(skip)]
    card_menu: Option<String>,
    #[serde(skip)]
    card_expanded: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct QueuedPrompt {
    content: String,
    images: Vec<(String, String)>,
    #[serde(default)]
    active_skill: Option<String>,
    #[serde(default = "default_follow_up")]
    after_yield: bool,
}

fn default_follow_up() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq)]
struct QueuedPromptEditState {
    index: usize,
    prompt_id: String,
    draft: crate::input::PromptInputSnapshot,
    active_skill: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct QueuedPromptEditSave {
    prompt_id: String,
    content: String,
    images: Vec<(String, String)>,
    draft: crate::input::PromptInputSnapshot,
    active_skill: Option<String>,
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

    pub(super) fn queued_edit_active(&self) -> bool {
        self.prompt_queue.queued_edit.is_some()
    }

    /// Load a daemon-queued prompt into the composer for in-place editing.
    /// Clicking Edit on a queue card uses the same path as Up recall.
    pub(super) fn begin_queued_prompt_edit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load_queued_prompt_edit(index, window, cx);
    }

    /// Up with an empty composer recalls the newest daemon-queued prompt for
    /// editing; while editing, Up walks towards older queued prompts.
    pub(super) fn queue_recall_prev(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(edit) = self.prompt_queue.queued_edit.as_ref() {
            if edit.index == 0 {
                // Past the oldest queued prompt: give the draft back and let
                // ordinary prompt history take this Up.
                self.cancel_queued_prompt_edit(window, cx);
                return false;
            }
            let index = edit.index - 1;
            self.load_queued_prompt_edit(index, window, cx);
            return true;
        }
        if !self.prompt_queue.remote_editing_supported || self.prompt_queue.remote.is_empty() {
            return false;
        }
        if !self.input.read(cx).content.trim().is_empty() {
            return false;
        }
        let index = self.prompt_queue.remote.len() - 1;
        self.load_queued_prompt_edit(index, window, cx);
        true
    }

    /// Down while editing walks towards newer queued prompts, then restores
    /// the draft and hands the key back to normal prompt history.
    pub(super) fn queue_recall_next(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self
            .prompt_queue
            .queued_edit
            .as_ref()
            .map(|edit| edit.index)
        else {
            return false;
        };
        if index + 1 < self.prompt_queue.remote.len() {
            self.load_queued_prompt_edit(index + 1, window, cx);
        } else {
            self.cancel_queued_prompt_edit(window, cx);
        }
        true
    }

    /// Enter while editing: replace the queued prompt in place.
    pub(super) fn save_queued_prompt_edit(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = self.prompt_queue.queued_edit.take() else {
            return false;
        };
        self.prompt_queue.queued_edit_pending = Some(QueuedPromptEditSave {
            prompt_id: edit.prompt_id.clone(),
            content: content.clone(),
            images: images.clone(),
            draft: edit.draft,
            active_skill: edit.active_skill.clone(),
        });
        self.active_skill = edit.active_skill;
        self.bridge.send(Command::EditQueuedPrompt {
            session_id: self.session_id.clone(),
            prompt_id: edit.prompt_id,
            content,
            images,
        });
        cx.notify();
        true
    }

    /// Esc while editing: release the daemon hold and restore the draft.
    pub(super) fn cancel_queued_prompt_edit(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(edit) = self.prompt_queue.queued_edit.take() else {
            return false;
        };
        self.bridge.send(Command::SetQueuedPromptEditing {
            session_id: self.session_id.clone(),
            prompt_id: edit.prompt_id,
            editing: false,
        });
        self.input
            .update(cx, |input, cx| input.restore(edit.draft, cx));
        self.active_skill = edit.active_skill;
        self.focus_input(window, cx);
        cx.notify();
        true
    }

    /// Outcome of an in-place edit. A rejection restores the edited text so it
    /// is never lost.
    pub(crate) fn queued_prompt_edited(
        &mut self,
        prompt_id: &str,
        result: Result<jcode_sdk::api::QueuedPromptEditOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        let pending = self
            .prompt_queue
            .queued_edit_pending
            .take_if(|save| save.prompt_id == prompt_id);
        match result {
            Ok(jcode_sdk::api::QueuedPromptEditOutcome::Edited) => {}
            Ok(outcome) => {
                self.items.push(Item::Error(format!(
                    "Could not save queued message: {outcome:?}"
                )));
                if let Some(save) = pending {
                    self.restore_rejected_queue_edit(save, cx);
                }
            }
            Err(reason) => {
                self.items.push(Item::Error(format!(
                    "Queue edit could not be confirmed: {reason}. The text is back in the composer; check whether the original was delivered before sending it again."
                )));
                if let Some(save) = pending {
                    self.restore_rejected_queue_edit(save, cx);
                }
            }
        }
        cx.notify();
    }

    fn restore_rejected_queue_edit(&mut self, save: QueuedPromptEditSave, cx: &mut Context<Self>) {
        self.active_skill = save.active_skill;
        self.input.update(cx, |input, cx| {
            input.restore(save.draft, cx);
            input.show_queued_prompt(&save.content, save.images, cx);
        });
    }

    fn load_queued_prompt_edit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.prompt_queue.remote.get(index).cloned() else {
            return;
        };
        // Switching items keeps the draft captured when editing began and
        // moves the daemon hold to the newly selected prompt.
        let (draft, active_skill) = match self.prompt_queue.queued_edit.take() {
            Some(previous) => {
                self.bridge.send(Command::SetQueuedPromptEditing {
                    session_id: self.session_id.clone(),
                    prompt_id: previous.prompt_id,
                    editing: false,
                });
                (previous.draft, previous.active_skill)
            }
            None => (self.input.read(cx).snapshot(), self.active_skill.clone()),
        };
        self.active_skill = prompt.active_skill.clone();
        self.input.update(cx, |input, cx| {
            input.show_queued_prompt(&prompt.content, prompt.images.clone(), cx);
        });
        self.prompt_queue.queued_edit = Some(QueuedPromptEditState {
            index,
            prompt_id: prompt.client_id.clone(),
            draft,
            active_skill,
        });
        self.bridge.send(Command::SetQueuedPromptEditing {
            session_id: self.session_id.clone(),
            prompt_id: prompt.client_id,
            editing: true,
        });
        self.focus_input(window, cx);
        cx.notify();
    }

    /// Keep the edit target pointing at the same prompt after a queue
    /// snapshot, and give the draft back if the daemon no longer owns it.
    fn sync_queued_edit_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.prompt_queue.queued_edit.as_ref() else {
            return;
        };
        if let Some(index) = self
            .prompt_queue
            .remote
            .iter()
            .position(|prompt| prompt.client_id == edit.prompt_id)
        {
            if let Some(edit) = self.prompt_queue.queued_edit.as_mut() {
                edit.index = index;
            }
            return;
        }
        let Some(edit) = self.prompt_queue.queued_edit.take() else {
            return;
        };
        self.input
            .update(cx, |input, cx| input.restore(edit.draft, cx));
        self.active_skill = edit.active_skill;
        self.items.push(Item::Error(
            "The queued prompt you were editing is no longer queued. Draft restored; check the conversation before sending it again.".into(),
        ));
        cx.notify();
    }

    pub(crate) fn queue_removal_settled(
        &mut self,
        prompt: jcode_sdk::api::QueuedPrompt,
        edit: bool,
        result: Result<jcode_sdk::api::QueuedPromptRemovalOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.remote_removing.remove(&prompt.client_id);
        match result {
            Ok(jcode_sdk::api::QueuedPromptRemovalOutcome::Removed) => {
                self.prompt_queue
                    .remote
                    .retain(|p| p.client_id != prompt.client_id);
                if edit {
                    self.active_skill = prompt.active_skill.clone();
                    self.input.update(cx, |input, cx| {
                        input.recall_prompt(&prompt.content, prompt.images, cx)
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
                        input.recall_prompt(&prompt.content, prompt.images, cx)
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
                mode: if index % 2 == 0 {
                    jcode_sdk::api::QueuedPromptMode::Steer
                } else {
                    jcode_sdk::api::QueuedPromptMode::AfterYield
                },
                editing: false,
            });
        }
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
        if self.active_skill.is_some() && !self.prompt_queue.remote_supported {
            if !self.is_pending_session()
                && self.history_loaded
                && !self.activity_active()
                && self.pending_users.is_empty()
                && !self.has_visible_queued_prompts()
                && !self.prompt_queue.waiting_for_connection
            {
                self.send_composer_prompt(content, images, cx);
            } else {
                self.prompt_queue.prompts.push(QueuedPrompt {
                    content,
                    images,
                    active_skill: self.active_skill.clone(),
                    after_yield: true,
                });
                self.prompt_queue.paused = false;
                self.send_queued_prompts(cx);
            }
            cx.notify();
            return;
        }
        if !queued && self.has_visible_queued_prompts() {
            if !content.trim().is_empty() || !images.is_empty() {
                if self.prompt_queue.remote_supported
                    && !self.is_pending_session()
                    && !self.prompt_queue.waiting_for_connection
                {
                    self.send_remote_prompt(content, images, true, self.active_skill.clone(), cx);
                } else {
                    self.prompt_queue.prompts.push(QueuedPrompt {
                        content,
                        images,
                        active_skill: self.active_skill.clone(),
                        after_yield: true,
                    });
                }
            }
            self.interrupt_all_queued_prompts(cx);
            return;
        }
        if self.prompt_queue.remote_supported
            && !self.is_pending_session()
            && !self.prompt_queue.waiting_for_connection
        {
            self.send_remote_prompt(
                content,
                images,
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
                active_skill: self.active_skill.clone(),
                after_yield: queued || self.prompt_queue.default_after_yield,
            });
            self.prompt_queue.paused = false;
            self.send_queued_prompts(cx);
        } else {
            self.send_composer_prompt(content, images, cx);
        }
        cx.notify();
    }

    fn interrupt_all_queued_prompts(&mut self, cx: &mut Context<Self>) {
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
        if ids.is_empty()
            || !self.prompt_queue.prompts.is_empty()
            || !self.prompt_queue.steered.is_empty()
        {
            self.prompt_queue.send_now_error = Some("Cannot send this queue now until the runtime reconnects and supports batch delivery.".into());
        } else {
            self.prompt_queue.send_now_error = None;
            for id in &ids {
                self.prompt_queue.remote_failures.remove(id);
            }
            self.prompt_queue.send_now_generation =
                self.prompt_queue.send_now_generation.wrapping_add(1);
            let generation = self.prompt_queue.send_now_generation;
            let pending_ids = ids.clone();
            self.bridge.send(Command::InterruptQueuedPrompts {
                session_id: self.session_id.clone(),
                ids,
            });
            cx.spawn(async move |panel, cx| {
                cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                let _ = panel.update(cx, |this, cx| {
                    if this.prompt_queue.send_now_generation != generation { return; }
                    let remaining: Vec<_> = pending_ids.iter().filter(|id| {
                        !this.prompt_queue.delivered_ids.contains(*id)
                            && !this.prompt_queue.remote_failures.contains_key(*id)
                            && this.prompt_queue.remote.iter().chain(&this.prompt_queue.pending_remote).any(|p| &p.client_id == *id)
                    }).cloned().collect();
                    if !remaining.is_empty() {
                        let reason = "Delivery is still blocked. Escalating cancellation; retry Send now if it remains queued.".to_string();
                        for id in remaining { this.prompt_queue.remote_failures.insert(id, reason.clone()); }
                        this.bridge.send(Command::Cancel { session_id: this.session_id.clone() });
                        cx.notify();
                    }
                });
            }).detach();
        }
        cx.notify();
    }

    fn send_remote_prompt(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        after_yield: bool,
        active_skill: Option<String>,
        cx: &mut Context<Self>,
    ) {
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
        self.bridge.send(Command::QueuePrompt {
            session_id: self.session_id.clone(),
            prompt,
        });
        crate::sounds::play(crate::sounds::Cue::Sent, cx);
        cx.notify();
    }

    fn send_composer_prompt(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        self.prompt_queue.paused = false;
        // A new prompt means the user has moved on from what they searched for.
        self.search_terms.clear();
        self.prompt_queue.auto_poke.start(self.items.len());
        self.send_prompt(content, images, cx);
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
        let steering = self.send_is_steering();
        if let Some(skill) = active_skill {
            self.bridge.send(Command::SendSkill {
                session_id: self.session_id.clone(),
                content: content.clone(),
                images: images.clone(),
                skill,
            });
        } else {
            self.bridge.send(Command::Send {
                session_id: self.session_id.clone(),
                content: content.clone(),
                images: images.clone(),
            });
        }
        self.prepare_prompt(&content);
        if steering {
            // Not delivered yet: show it as queued, and let the transcript
            // take it once the model has seen it.
            self.prompt_queue.steered.push(QueuedPrompt {
                content,
                images,
                active_skill: None,
                after_yield: false,
            });
            crate::sounds::play(crate::sounds::Cue::Sent, cx);
            return;
        }
        let index = self.items.len();
        self.items.push(Item::User(content));
        self.stamp_user(index);
        self.items.extend(
            images.into_iter().map(|(media_type, data)| {
                Item::Image(TranscriptImage::new(media_type, data, None))
            }),
        );
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
                if activity_state::is_retry_phase(phase) && self.prompt_queue.paused =>
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
            }
            _ => {}
        }
    }

    pub(super) fn send_queued_prompts(&mut self, cx: &mut Context<Self>) {
        if self.prompt_queue.prompts.is_empty() {
            if !self.is_pending_session() && self.history_loaded {
                self.prompt_queue.waiting_for_connection = false;
            }
            return;
        }
        if self.prompt_queue.paused || self.is_pending_session() || !self.history_loaded {
            return;
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
                return;
            }
            let prompt = self.prompt_queue.prompts.remove(0);
            self.prompt_queue.waiting_for_connection = false;
            self.prompt_queue.paused = false;
            self.prompt_queue.auto_poke.start(self.items.len());
            self.send_prompt_with_skill(prompt.content, prompt.images, prompt.active_skill, cx);
            cx.notify();
            return;
        }
        if !self.prompt_queue.remote_supported
            && (!(self.status == "idle"
                || (self.prompt_queue.waiting_for_connection && self.status == "connected"))
                || self.activity_active()
                || !self.pending_users.is_empty())
        {
            return;
        }
        let prompts = std::mem::take(&mut self.prompt_queue.prompts);
        self.prompt_queue.waiting_for_connection = false;
        if self.prompt_queue.remote_supported {
            // Preserve independent follow-up turns when the daemon owns the
            // queue, including prompts typed before the session attached.
            for prompt in prompts {
                self.send_remote_prompt(
                    prompt.content,
                    prompt.images,
                    prompt.after_yield,
                    prompt.active_skill,
                    cx,
                );
            }
            cx.notify();
            return;
        }
        // Legacy daemons have only the local queue. Keep their historical
        // behavior of combining prompts into one follow-up turn.
        let content = prompts
            .iter()
            .map(|p| p.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let images = prompts.into_iter().flat_map(|p| p.images).collect();
        self.prompt_queue.paused = false;
        self.prompt_queue.auto_poke.start(self.items.len());
        self.send_prompt_with_skill(content, images, None, cx);
        cx.notify();
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
        {
            return;
        }
        let prompt = self.prompt_queue.prompts.remove(index);
        self.send_prompt_with_skill(prompt.content, prompt.images, prompt.active_skill, cx);
        cx.notify();
    }

    /// Move one queued prompt back into the composer for editing.
    pub(super) fn recall_queued_prompt(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.prompt_queue.prompts.len() {
            return;
        }
        let prompt = self.prompt_queue.prompts.remove(index);
        self.active_skill = prompt.active_skill;
        self.input.update(cx, |input, cx| {
            input.recall_prompt(&prompt.content, prompt.images, cx)
        });
        let focus = self.input.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
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
        let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
        let ranges = crate::input::image_marker_ranges(&flat);
        if ranges.is_empty() && images.is_empty() {
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
                    this.open_image_preview(image.clone(), window, cx)
                }));
            }
            row = row.child(chip);
            offset = range.end;
        }
        row = row.child(flat[offset..].to_owned());
        for (index, (media, data)) in images.iter().enumerate().skip(ranges.len()) {
            let image = TranscriptImage::new(media.clone(), data.clone(), None);
            row = row.child(
                crate::image_chip::chip(
                    image.preview.clone(),
                    &format!("[#image {}]", index + 1),
                    "queue-image-chip",
                    "queue-image-thumbnail",
                    "queue-image-label",
                )
                .id(("queue-image-chip", index))
                .flex_none()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_image_preview(image.clone(), window, cx)
                })),
            );
        }
        row.into_any_element()
    }
}

fn queue_action(
    id: &'static str,
    index: usize,
    icon: &'static [u8],
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let theme = Theme::global();
    div()
        .id((id, index))
        .debug_selector(move || format!("{id}-{index}"))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded_full()
        .cursor_pointer()
        .hover(|el| el.bg(theme.QUOTE_BG))
        .child(
            gpui::svg()
                .data(icon)
                .size(px(13.))
                .text_color(theme.TEXT_DIM),
        )
        .on_click(on_click)
}

#[cfg(test)]
#[path = "panel_queue_tests.rs"]
mod tests;
