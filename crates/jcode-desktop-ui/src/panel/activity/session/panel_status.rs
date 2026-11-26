//! What a panel reports about itself: its footer status, tab ring and activity.
use super::*;

impl Panel {

    pub(super) fn line(&self) -> String {
        if self.continuing_after_reload && self.activity_active() && self.status != "compacting" {
            return "Interrupted by update, continuing…".into();
        }
        let phase = self.connection_phase.trim();
        if !phase.is_empty() && !matches!(phase, "connected" | "streaming") {
            return activity_state::phase_label(phase);
        }
        // Memory recall runs before the model request, so while it works its
        // step is the most specific thing this session is doing. Streaming,
        // reasoning and running tools take over as soon as they start.
        if self.activity_active()
            && !matches!(
                self.status.as_str(),
                "thinking" | "streaming" | "running_tools" | "compacting"
            )
            && let Some(activity) = self.memory_activity.as_ref()
            && let Some(label) = activity.status_label()
        {
            return label;
        }
        if self.activity_active() {
            // Buffers may still contain an earlier block. The latest semantic
            // event remains authoritative even after ReasoningDone flushes it.
            match self.status.as_str() {
                "thinking" => return "Thinking".into(),
                "streaming" => return "Responding".into(),
                "running_tools" => return "Running tools".into(),
                "compacting" => return "Compacting context".into(),
                _ => {}
            }
            if !self.streaming_reasoning.is_empty() {
                return "Thinking".into();
            }
            if !self.streaming_text.is_empty() {
                return "Responding".into();
            }
            if phase == "streaming" && self.status != "running_tools" {
                return "Responding".into();
            }
        }
        match self.status.as_str() {
            "idle" | "connected" => "Ready".into(),
            "busy" | "running" => "Working".into(),
            "generating" => "Working".into(),
            "thinking" => "Thinking".into(),
            "streaming" => "Responding".into(),
            "running_tools" => "Running tools".into(),
            "compacting" => "Compacting context".into(),
            status => status.replace('_', " "),
        }
    }

    pub(super) fn minimap(&self) -> MinimapSessionState {
        let status = self.lowercase_status();
        if status.contains("error")
            || status.contains("crash")
            || matches!(
                self.items.last(),
                Some(Item::Error(_) | Item::RuntimeError { .. })
            )
        {
            MinimapSessionState::Error
        } else if !self.streaming_text.is_empty() || !self.streaming_reasoning.is_empty() {
            MinimapSessionState::Streaming
        } else if status != "idle" {
            MinimapSessionState::Working
        } else if self
            .latest_todo_progress()
            .is_some_and(|(completed, total)| total > 0 && completed == total)
        {
            MinimapSessionState::Complete
        } else {
            MinimapSessionState::Idle
        }
    }

    pub(super) fn active(&self) -> bool {
        let status = self.lowercase_status();
        if status.contains("error") || status.contains("crash") || status.starts_with("lost:") {
            return false;
        }
        // Historical transcript errors do not describe the current turn. A
        // transport streaming event can also arrive before its session status.
        self.continuing_after_reload
            || !self.streaming_text.is_empty()
            || !self.streaming_reasoning.is_empty()
            || self.connection_phase.trim() == "streaming"
            || activity_state::is_request_phase(&self.connection_phase)
            || matches!(
                status.as_ref(),
                "generating"
                    | "running"
                    | "busy"
                    | "thinking"
                    | "streaming"
                    | "running_tools"
                    | "compacting"
            )
    }

    pub(super) fn live_wait(&self) -> bool {
        self.items
            .iter()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .any(|item| {
                let Item::Tool {
                    name,
                    input,
                    done: false,
                    ..
                } = item
                else {
                    return false;
                };
                matches!(name.as_str(), "bg" | "swarm")
                    && serde_json::from_str::<serde_json::Value>(input).is_ok_and(|input| {
                        matches!(
                            (name.as_str(), input["action"].as_str()),
                            ("bg", Some("wait")) | ("swarm", Some("await_members"))
                        )
                    })
            })
    }

    pub(super) fn ring(&self) -> Option<TabRing> {
        // Only a live turn animates. `minimap_state` treats any non-idle
        // status as work, which would mark merely connected sessions.
        let state = match self.minimap_state() {
            MinimapSessionState::Error => MinimapSessionState::Error,
            _ if self
                .ask_card
                .as_ref()
                .is_some_and(|card| !card.submitting())
                || self
                    .routing_gate
                    .as_ref()
                    .is_some_and(|gate| !gate.view().submitting) =>
            {
                return Some(TabRing::question());
            }
            _ if self.activity_active() && self.has_live_wait_tool() => {
                return Some(TabRing::waiting());
            }
            _ if self.activity_active() => MinimapSessionState::Working,
            _ if self.tab_dependency_waiting || !self.awaiting_ids().is_empty() => {
                return Some(TabRing::waiting());
            }
            _ if self.finished_unseen => MinimapSessionState::Complete,
            _ => MinimapSessionState::Idle,
        };
        TabRing::resolve(state, self.latest_todo_progress())
    }
}
