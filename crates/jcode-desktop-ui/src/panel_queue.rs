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
    #[serde(default)]
    pending_remote: Vec<jcode_sdk::api::QueuedPrompt>,
    #[serde(skip)]
    pub(super) replay_pending_on_connect: bool,
    /// Count matching history rows at submission, for lost-ack reconciliation.
    #[serde(default)]
    pending_remote_history_counts: std::collections::HashMap<String, usize>,
    #[serde(default)]
    remote_failures: std::collections::HashMap<String, String>,
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
    #[serde(default = "default_follow_up")]
    after_yield: bool,
}

fn default_follow_up() -> bool {
    true
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
        if self.prompt_queue.remote_supported
            && !self.is_pending_session()
            && !self.prompt_queue.waiting_for_connection
        {
            self.send_remote_prompt(
                content,
                images,
                queued || self.prompt_queue.default_after_yield,
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
                after_yield: queued || self.prompt_queue.default_after_yield,
            });
            self.prompt_queue.paused = false;
            self.send_queued_prompts(cx);
        } else {
            self.send_composer_prompt(content, images, cx);
        }
        cx.notify();
    }

    fn send_remote_prompt(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        after_yield: bool,
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
            client_id: id,
            content,
            images,
            mode: if after_yield {
                jcode_sdk::api::QueuedPromptMode::AfterYield
            } else {
                jcode_sdk::api::QueuedPromptMode::Steer
            },
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
        self.prompt_queue.auto_poke.start(self.items.len());
        self.send_prompt(content, images, cx);
    }

    fn send_prompt(
        &mut self,
        content: String,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let steering = self.send_is_steering();
        self.bridge.send(Command::Send {
            session_id: self.session_id.clone(),
            content: content.clone(),
            images: images.clone(),
        });
        self.prepare_prompt(&content);
        if steering {
            // Not delivered yet: show it as queued, and let the transcript
            // take it once the model has seen it.
            self.prompt_queue.steered.push(QueuedPrompt {
                content,
                images,
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
                self.prompt_queue
                    .remote_failures
                    .retain(|id, _| !prompts.iter().any(|p| &p.client_id == id));
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
                self.send_remote_prompt(prompt.content, prompt.images, prompt.after_yield, cx);
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
        self.send_composer_prompt(content, images, cx);
        cx.notify();
    }

    /// Send one queued prompt immediately. During an active turn the harness
    /// delivers it as an urgent steer rather than waiting for the turn to end.
    pub(super) fn send_queued_prompt_now(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.prompt_queue.prompts.len()
            || self.is_pending_session()
            || self.prompt_queue.waiting_for_connection
        {
            return;
        }
        let prompt = self.prompt_queue.prompts.remove(index);
        self.send_prompt(prompt.content, prompt.images, cx);
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
