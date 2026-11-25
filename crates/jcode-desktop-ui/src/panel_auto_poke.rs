//! Desktop-owned continuation of locally submitted work, not attached sessions.
//!
//! The decision itself is the SDK's [`FollowUpPolicy`], the same state machine
//! the TUI runs: incomplete-todo pokes, the long-session review, the deferred
//! quality-gate digest, ownership and completion-confidence gates, and the
//! final-response handoff. Desktop only supplies the turn boundary, fetches
//! the session's todo state through the harness, and renders the notices.
use super::*;
use jcode_sdk::todo::{FollowUpPolicy, TodoSnapshot};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AssistantTail {
    #[default]
    Empty,
    Streaming,
    Complete,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AutoPoke {
    /// Created lazily from `features.auto_poke` so a snapshot restore or a
    /// default-constructed queue never silently disables the feature.
    policy: Option<FollowUpPolicy>,
    /// Transcript index of the local request that owns this cycle. Todo cards
    /// before it belong to older requests and must not drive a continuation.
    start_index: Option<usize>,
    /// A turn finished and the follow-up decision has not been taken yet.
    ready: bool,
    assistant_tail: AssistantTail,
    /// While todo state is in flight, keep the completed turn's evidence.
    awaiting_state: Option<bool>,
}

pub(in crate::panel) enum PokeCommand {
    Trigger,
    On,
    Off,
    Status,
}

pub(in crate::panel) fn parse_poke_command(trimmed: &str) -> Option<Result<PokeCommand, String>> {
    match trimmed {
        "/poke" => Some(Ok(PokeCommand::Trigger)),
        "/poke on" => Some(Ok(PokeCommand::On)),
        "/poke off" => Some(Ok(PokeCommand::Off)),
        "/poke status" => Some(Ok(PokeCommand::Status)),
        _ if trimmed.starts_with("/poke ") => Some(Err("Usage: `/poke [on|off|status]`".into())),
        _ => None,
    }
}

impl AutoPoke {
    fn policy(&mut self) -> &mut FollowUpPolicy {
        self.policy.get_or_insert_with(|| {
            FollowUpPolicy::new(jcode_base::config::config().features.auto_poke)
        })
    }

    pub(super) fn start(&mut self, index: usize) {
        self.start_index = Some(index);
        self.ready = false;
        self.assistant_tail = AssistantTail::Empty;
        self.awaiting_state = None;
    }

    /// A manual `/poke`: start a cycle at `index` that is already due.
    pub(super) fn trigger(&mut self, index: usize) {
        self.start(index);
        self.ready = true;
    }

    /// At a turn boundary: whether to ask the harness for todo state. When it
    /// does, the cycle waits for the answer and keeps the finished turn's
    /// evidence of a delivered final response. `blocked` is the user having
    /// work of their own queued or running.
    pub(super) fn request_state(&mut self, blocked: bool, turn_done: bool) -> bool {
        if !std::mem::take(&mut self.ready)
            || self.start_index.is_none()
            || self.awaiting_state.is_some()
        {
            return false;
        }
        if blocked || !self.policy().is_enabled() {
            return false;
        }
        let tail = std::mem::take(&mut self.assistant_tail);
        self.awaiting_state = Some(tail == AssistantTail::Complete && turn_done);
        true
    }

    /// The todo-state answer arrived. `None` when nothing was awaited or the
    /// user's own work comes first; otherwise what to do next. `fallback`
    /// reads the todo state off the transcript from the cycle's first row
    /// when the harness had none.
    pub(super) fn answer(
        &mut self,
        state: Option<TodoSnapshot>,
        blocked: bool,
        fallback: impl FnOnce(usize) -> TodoSnapshot,
    ) -> Option<jcode_sdk::todo::Decision> {
        let delivered = self.awaiting_state.take()?;
        let start = self.start_index?;
        if blocked {
            return None;
        }
        let snapshot = state.unwrap_or_else(|| fallback(start));
        Some(self.policy().decide_after_turn(&snapshot, delivered))
    }

    pub(super) fn observe(&mut self, event: &ApiEvent) {
        match event {
            ApiEvent::TurnDone { .. } => self.ready = self.start_index.is_some(),
            ApiEvent::Error { .. } | ApiEvent::TurnStopped { .. } => self.stop(),
            ApiEvent::SessionStatus { status, .. }
                if matches!(status.as_str(), "cancelled" | "canceled" | "disconnected") =>
            {
                self.stop();
            }
            ApiEvent::TextDelta { text, .. } if !text.trim().is_empty() => {
                self.assistant_tail = AssistantTail::Streaming;
            }
            ApiEvent::TextDone { .. } if self.assistant_tail == AssistantTail::Streaming => {
                self.assistant_tail = AssistantTail::Complete;
            }
            ApiEvent::TextReplace { text, .. } if text.trim().is_empty() => {
                self.assistant_tail = AssistantTail::Empty;
            }
            ApiEvent::TextReplace { .. } if self.assistant_tail == AssistantTail::Empty => {
                self.assistant_tail = AssistantTail::Streaming;
            }
            // Commentary before a tool or before a new input is not a final answer.
            ApiEvent::ToolStart { .. }
            | ApiEvent::ToolExec { .. }
            | ApiEvent::ToolDone { .. }
            | ApiEvent::ToolCall { .. }
            | ApiEvent::MessageAccepted { .. }
            | ApiEvent::PeerUserMessage { .. }
            | ApiEvent::SoftInterruptInjected {
                display_role: None, ..
            }
            | ApiEvent::ProviderGuardrailRetry { .. } => {
                self.assistant_tail = AssistantTail::Empty;
            }
            _ => {}
        }
    }

    /// A stop, failure, or cancel ends the cycle until the next local request.
    pub(super) fn stop(&mut self) {
        self.start_index = None;
        self.ready = false;
        self.assistant_tail = AssistantTail::Empty;
        self.awaiting_state = None;
        if let Some(policy) = &mut self.policy {
            policy.reset_cycle();
        }
    }
}

/// Snapshot from the latest todo card, for runtimes without `todo_state`.
/// Lacks deferred gate observations and the review clock, so only the poke,
/// ownership, confidence, and final-response gates can run.
fn snapshot_from_card(payload: TodoCardPayload) -> TodoSnapshot {
    TodoSnapshot {
        todos: payload
            .todos
            .into_iter()
            .map(|todo| jcode_sdk::todo::TodoItem {
                id: todo.content.clone(),
                content: todo.content,
                status: todo.status,
                priority: "medium".into(),
                group: todo.group,
                stage: todo.stage,
                assigned_to: todo.assigned_to,
                confidence: todo.confidence,
                completion_confidence: todo.completion_confidence,
                blocked_by: todo.blocked_by,
                ..Default::default()
            })
            .collect(),
        plan: jcode_sdk::todo::TodoPlan {
            user_intention: payload.plan.user_intention,
            understands_user_intent: payload.plan.understands_user_intent,
            ..Default::default()
        },
        goals: payload.goals,
        ..Default::default()
    }
}

/// User-facing one-liner for a synthetic continuation message.
pub(in crate::panel) fn auto_poke_notice_text(message: &str) -> String {
    jcode_sdk::todo::auto_poke_display_summary(message)
        .map(str::to_string)
        // The incomplete-todo poke is already short and its count is useful.
        .unwrap_or_else(|| format!("👉 {}", message.trim()))
}

impl Panel {
    fn auto_poke_blocked(&self) -> bool {
        self.prompt_queue.paused
            || self.prompt_queue.waiting_for_connection
            || !self.prompt_queue.prompts.is_empty()
            || !self.pending_users.is_empty()
            || self.status != "idle"
            || self.activity_active()
            || !self.history_loaded
            || self.is_pending_session()
    }

    /// Turn boundary: ask the harness for the session's todo state.
    pub(super) fn send_auto_poke(&mut self, _cx: &mut Context<Self>) {
        let blocked = self.auto_poke_blocked();
        let turn_done = self.turn_outcome == Some(turn::TurnOutcome::Done);
        if !self.prompt_queue.auto_poke.request_state(blocked, turn_done) {
            return;
        }
        self.bridge.send(Command::TodoState {
            session_id: self.session_id.clone(),
        });
    }

    /// The harness answered a todo-state request. Decide and act.
    pub(crate) fn todo_state_received(
        &mut self,
        state: Option<TodoSnapshot>,
        cx: &mut Context<Self>,
    ) {
        // The user typed, queued, or restarted work while the state was in
        // flight: their input goes first.
        let blocked = self.auto_poke_blocked();
        let items = &self.items;
        let Some(decision) = self
            .prompt_queue
            .auto_poke
            .answer(state, blocked, |start| {
                snapshot_from_card(
                    items
                        .get(start..)
                        .unwrap_or_default()
                        .iter()
                        .rev()
                        .find_map(|item| match item {
                            Item::Todos(payload) => Some(payload.clone()),
                            Item::Tool {
                                name,
                                output,
                                done: true,
                                error: None,
                                ..
                            } if name == "todo" => parse_todo_tool_output(output),
                            _ => None,
                        })
                        .unwrap_or_default(),
                )
            })
        else {
            return;
        };
        if !decision.effects.is_empty() {
            self.bridge.send(Command::AckTodoFollowUp {
                session_id: self.session_id.clone(),
                effects: decision.effects,
            });
        }
        if let Some(notice) = decision.notice {
            self.items.push(Item::Error(notice));
        }
        if let Some(follow_up) = decision.follow_up {
            // Keep the cycle state. This is not a new user request.
            self.send_prompt(follow_up.message, Vec::new(), cx);
        }
        cx.notify();
    }

    /// A synthetic continuation, shown as the short notice the TUI uses rather
    /// than the model-facing instructions. Selectable like other notices.
    pub(in crate::panel) fn render_auto_poke_notice(
        &self,
        index: usize,
        text: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        div()
            .debug_selector(|| "auto-poke-notice".into())
            .flex()
            .items_center()
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme.HEADER_BG)
            .text_size(px(11.))
            .text_color(theme.TEXT_DIM)
            .child(text_selection::plain(
                self.transcript_selection.clone(),
                format!("{index}-auto-poke"),
                auto_poke_notice_text(text),
                window,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::panel) fn handle_poke_command(
        &mut self,
        command: PokeCommand,
        cx: &mut Context<Self>,
    ) {
        let message = match command {
            PokeCommand::On | PokeCommand::Trigger => {
                self.prompt_queue.auto_poke.policy().enable();
                if matches!(command, PokeCommand::Trigger) && !self.auto_poke_blocked() {
                    self.prompt_queue.auto_poke.trigger(self.items.len());
                    self.send_auto_poke(cx);
                    "Auto-poke on. Checking todos now.".to_string()
                } else {
                    "Auto-poke on. Unfinished todos and quality checks continue automatically after each response. `/poke off` to stop.".to_string()
                }
            }
            PokeCommand::Off => {
                self.prompt_queue.auto_poke.policy().disable();
                self.prompt_queue.auto_poke.stop();
                "Auto-poke off for this session. `/poke on` to resume.".to_string()
            }
            PokeCommand::Status => format!(
                "Auto-poke is {} for this session.",
                if self.prompt_queue.auto_poke.policy().is_enabled() {
                    "on"
                } else {
                    "off"
                }
            ),
        };
        self.items.push(Item::Assistant(message));
    }
}

#[cfg(test)]
#[path = "panel_auto_poke_tests.rs"]
mod tests;
