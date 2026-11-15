//! Prompts explicitly queued by Ctrl+Enter wait for the current turn to finish.
use super::*;

#[path = "panel_auto_poke.rs"]
mod auto_poke;
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
    #[serde(skip)]
    steered: Vec<QueuedPrompt>,
    /// Durable server state plus submissions not yet acknowledged in a snapshot.
    #[serde(skip)]
    remote: Vec<jcode_sdk::api::QueuedPrompt>,
    #[serde(skip)]
    remote_supported: bool,
    #[serde(default)]
    pending_remote: Vec<jcode_sdk::api::QueuedPrompt>,
    #[serde(default)]
    default_after_yield: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct QueuedPrompt {
    content: String,
    images: Vec<(String, String)>,
}

impl Panel {
    pub(super) fn has_queued_prompts(&self) -> bool {
        !self.prompt_queue.prompts.is_empty()
            || !self.prompt_queue.remote.is_empty()
            || !self.prompt_queue.pending_remote.is_empty()
    }

    /// Prompts handed to the runtime as steering and not yet delivered.
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
            // Nothing is left that could still deliver them: the turn ended
            // with no interrupt waiting, or stopped. Keep the text, in order.
            ApiEvent::TurnDone {
                pending_soft_interrupts: None | Some(0),
                ..
            }
            | ApiEvent::TurnStopped { .. }
            | ApiEvent::Error { .. } => self.deliver_steered_prompts(),
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
        for index in 0..count {
            let sample = SAMPLES[index % SAMPLES.len()];
            self.prompt_queue.steered.push(QueuedPrompt {
                content: if round == 0 {
                    sample.to_string()
                } else {
                    format!("{sample} (round {})", round + 1)
                },
                images: Vec::new(),
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
            && self.activity_active()
            && !self.prompt_queue.waiting_for_connection
        {
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
                mode: if queued || self.prompt_queue.default_after_yield {
                    jcode_sdk::api::QueuedPromptMode::AfterYield
                } else {
                    jcode_sdk::api::QueuedPromptMode::Steer
                },
            };
            self.prompt_queue.pending_remote.push(prompt.clone());
            self.bridge.send(Command::QueuePrompt {
                session_id: self.session_id.clone(),
                prompt,
            });
            crate::sounds::play(crate::sounds::Cue::Sent, cx);
            cx.notify();
            return;
        }
        if self.is_pending_session()
            || self.prompt_queue.waiting_for_connection
            || (queued && (self.activity_active() || !self.pending_users.is_empty()))
        {
            self.prompt_queue.waiting_for_connection |= self.is_pending_session();
            self.prompt_queue
                .prompts
                .push(QueuedPrompt { content, images });
            self.prompt_queue.paused = false;
            self.send_queued_prompts(cx);
        } else {
            self.send_composer_prompt(content, images, cx);
        }
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
        if !self.items.iter().any(|item| matches!(item, Item::User(_)))
            && custom_session_title(&self.session_id, self.title.as_ref()).is_none()
            && let Some(title) = first_prompt_title(&super::voice::tag::strip(&content).0)
        {
            self.title = title.into();
        }
        if let Some(layout) = &mut self.startup_layout {
            layout.committed = true;
        }
        if steering {
            // Not delivered yet: show it as queued, and let the transcript
            // take it once the model has seen it.
            self.prompt_queue
                .steered
                .push(QueuedPrompt { content, images });
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

    pub(super) fn observe_prompt_queue(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        match event {
            ApiEvent::QueuedPrompts { prompts, .. } => {
                self.prompt_queue.remote_supported = true;
                self.prompt_queue
                    .pending_remote
                    .retain(|pending| !prompts.iter().any(|p| p.client_id == pending.client_id));
                self.prompt_queue.remote = prompts.clone();
            }
            ApiEvent::SoftInterruptInjected {
                client_ids,
                display_role: None,
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
                    self.push_user_prompt(prompt.content, prompt.images);
                }
            }
            _ => {}
        }
        self.observe_steered_prompts(event);
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
        if self.prompt_queue.paused
            || !(self.status == "idle"
                || (self.prompt_queue.waiting_for_connection && self.status == "connected"))
            || self.activity_active()
            || !self.pending_users.is_empty()
            || self.is_pending_session()
            || !self.history_loaded
        {
            return;
        }
        // Match the TUI: combine waiting prompts into one follow-up turn, in
        // order, retaining every attachment. Draining atomically also makes a
        // TurnDone followed by idle incapable of sending the same queue twice.
        let prompts = std::mem::take(&mut self.prompt_queue.prompts);
        self.prompt_queue.waiting_for_connection = false;
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
        if index >= self.prompt_queue.prompts.len() || self.is_pending_session() {
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
        let remote = self.render_remote_prompts(cx);
        let steered = self.render_steered_prompts();
        let waiting = self.render_waiting_prompts(cx);
        if remote.is_none() && steered.is_none() && waiting.is_none() {
            return None;
        }
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap_2()
                .min_w_0()
                .children(remote)
                .children(steered)
                .children(waiting)
                .into_any_element(),
        )
    }

    fn render_remote_prompts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.prompt_queue.remote_supported
            || (!self.activity_active()
                && self.prompt_queue.remote.is_empty()
                && self.prompt_queue.pending_remote.is_empty())
        {
            return None;
        }
        let theme = Theme::global();
        let default = if self.prompt_queue.default_after_yield {
            "Follow-up"
        } else {
            "Steer"
        };
        let rows = self
            .prompt_queue
            .remote
            .iter()
            .map(|prompt| (prompt, false))
            .chain(
                self.prompt_queue
                    .pending_remote
                    .iter()
                    .map(|prompt| (prompt, true)),
            );
        Some(
            div()
                .id("daemon-prompt-queue")
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .mx_2()
                .p_2()
                .rounded_md()
                .bg(theme.TOOL_BG)
                .text_size(px(12.))
                .text_color(theme.TEXT_DIM)
                .child(
                    div()
                        .id("queue-default-mode")
                        .cursor_pointer()
                        .child(format!("Default: {default} ▾"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.prompt_queue.default_after_yield =
                                !this.prompt_queue.default_after_yield;
                            cx.notify();
                        })),
                )
                .children(rows.map(|(prompt, pending)| {
                    let id = prompt.client_id.clone();
                    let id2 = id.clone();
                    let retry = prompt.clone();
                    let mode = prompt.mode;
                    let label = match mode {
                        jcode_sdk::api::QueuedPromptMode::Steer => "Steer",
                        jcode_sdk::api::QueuedPromptMode::AfterYield => "Follow-up",
                    };
                    let next = match mode {
                        jcode_sdk::api::QueuedPromptMode::Steer => {
                            jcode_sdk::api::QueuedPromptMode::AfterYield
                        }
                        jcode_sdk::api::QueuedPromptMode::AfterYield => {
                            jcode_sdk::api::QueuedPromptMode::Steer
                        }
                    };
                    div()
                        .id(SharedString::from(format!("daemon-queued-{id}")))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("queue-mode-{id}")))
                                .cursor_pointer()
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, _| {
                                    if !pending {
                                        this.bridge.send(Command::SetQueuedPromptMode {
                                            session_id: this.session_id.clone(),
                                            client_id: id.clone(),
                                            mode: next,
                                        });
                                    }
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(steered_preview(&prompt.content, prompt.images.len())),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("queue-interrupt-{id2}")))
                                .cursor_pointer()
                                .child(if pending { "Retry" } else { "Interrupt ↵" })
                                .on_click(cx.listener(move |this, _, _, _| {
                                    if pending {
                                        this.bridge.send(Command::QueuePrompt {
                                            session_id: this.session_id.clone(),
                                            prompt: retry.clone(),
                                        });
                                    } else {
                                        this.bridge.send(Command::InterruptQueuedPrompt {
                                            session_id: this.session_id.clone(),
                                            client_id: id2.clone(),
                                        });
                                    }
                                })),
                        )
                }))
                .into_any_element(),
        )
    }

    /// Dim `queued ⏎ <text>` rows for prompts the runtime has not delivered.
    fn render_steered_prompts(&self) -> Option<gpui::AnyElement> {
        let steered = &self.prompt_queue.steered;
        if steered.is_empty() {
            return None;
        }
        const SHOWN: usize = 3;
        let theme = Theme::global();
        let count = steered.len();
        Some(
            div()
                .id("steered-prompts")
                .debug_selector(|| "steered-prompts".into())
                .flex_none()
                .flex()
                .flex_col()
                .min_w_0()
                .mx_2()
                .p_2()
                .rounded_md()
                // The composer sits on a see-through veil, so unbacked text
                // would read against transcript rows streaming underneath.
                .bg(theme.TOOL_BG)
                .gap_1()
                .text_size(px(12.))
                .text_color(theme.TEXT_DIM)
                .children(
                    steered
                        .iter()
                        .take(SHOWN)
                        .enumerate()
                        .map(|(index, prompt)| {
                            let preview = steered_preview(&prompt.content, prompt.images.len());
                            div()
                                .debug_selector(move || format!("steered-prompt-{index}"))
                                .flex()
                                .flex_none()
                                .min_w_0()
                                .items_center()
                                .gap_2()
                                .child(div().flex_none().child("queued ⏎"))
                                .child(div().flex_1().min_w_0().truncate().child(preview))
                                .when(index == 0 && count > 1, |row| {
                                    row.child(
                                        div()
                                            .debug_selector(|| "steered-count".into())
                                            .flex_none()
                                            .child(format!("⏎ {count}")),
                                    )
                                })
                        }),
                )
                .when(count > SHOWN, |el| {
                    el.child(div().child(format!("+{} more queued", count - SHOWN)))
                })
                .into_any_element(),
        )
    }

    fn render_waiting_prompts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.prompt_queue.prompts.is_empty() {
            return None;
        }
        let theme = Theme::global();
        Some(
            div()
                .id("prompt-queue")
                .debug_selector(|| "prompt-queue".into())
                .flex_none()
                .flex()
                .flex_col()
                .gap_2()
                .min_w_0()
                .mx_2()
                .p_2()
                .rounded_md()
                .bg(theme.TOOL_BG)
                .text_size(px(12.))
                .child(
                    div()
                        .text_color(theme.TEXT_DIM)
                        .child(if self.prompt_queue.paused {
                            "Queue paused · waiting prompts are preserved"
                        } else if self.is_pending_session()
                            || self.prompt_queue.waiting_for_connection
                        {
                            "Queued · sends when connected"
                        } else {
                            "Queued · sends after this response"
                        }),
                )
                .child(
                    div()
                        .id("prompt-queue-items")
                        .debug_selector(|| "prompt-queue-items".into())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .max_h(px(144.))
                        .overflow_y_scroll()
                        // The dock passes wheel input to the transcript; the
                        // queue scrolls on its own.
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .children(self.prompt_queue.prompts.iter().enumerate().map(
                            |(index, prompt)| {
                                let label = if prompt.images.is_empty() {
                                    prompt.content.clone()
                                } else {
                                    format!("{} · {} image(s)", prompt.content, prompt.images.len())
                                };
                                div()
                                    .debug_selector(move || format!("queued-prompt-{index}"))
                                    .flex()
                                    .flex_none()
                                    .min_w_0()
                                    .items_center()
                                    .gap_2()
                                    .py_1()
                                    .child(
                                        div()
                                            .flex_none()
                                            .min_w(px(20.))
                                            .text_color(theme.TEXT_DIM)
                                            .child(format!("{}.", index + 1)),
                                    )
                                    .child(div().flex_1().min_w_0().truncate().child(label))
                                    .child(queue_action(
                                        "send-queued-prompt",
                                        index,
                                        include_bytes!("../../../assets/icons/arrow-up.svg"),
                                        cx.listener(move |this, _, _, cx| {
                                            this.send_queued_prompt_now(index, cx)
                                        }),
                                    ))
                                    .child(queue_action(
                                        "recall-queued-prompt",
                                        index,
                                        include_bytes!("../../../assets/icons/arrow-down.svg"),
                                        cx.listener(move |this, _, window, cx| {
                                            this.recall_queued_prompt(index, window, cx)
                                        }),
                                    ))
                            },
                        )),
                )
                .when(self.prompt_queue.paused, |el| {
                    el.child(
                        div()
                            .id("resume-prompt-queue")
                            .cursor_pointer()
                            .child("Resume queue")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prompt_queue.paused = false;
                                this.send_queued_prompts(cx);
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }
}

/// First characters of a steered prompt, on one line.
fn steered_preview(content: &str, images: usize) -> String {
    const CHARS: usize = 100;
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview: String = flat.chars().take(CHARS).collect();
    if flat.chars().count() > CHARS {
        preview.push('…');
    }
    if images > 0 {
        preview.push_str(&format!(" · {images} image(s)"));
    }
    preview
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
        .rounded_sm()
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
