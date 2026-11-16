//! Semantic data exposed by Desktop control, independent of painted pixels.
use super::*;

impl Panel {
    pub(crate) fn control_state(&self, cx: &App) -> serde_json::Value {
        let ask = self.ask_card.as_ref().map(|card| {
            serde_json::json!({
                "request_id": card.request_id,
                "tool_call_id": card.tool_call_id,
                "questions": card.questions,
                "chosen": card.chosen,
                "cursor_question": card.cursor_question,
                "cursor_option": card.cursor_option,
                "submitting": card.submitting,
                "error": card.submit_error,
            })
        });
        let workers = self
            .swarm_workers
            .iter()
            .map(|worker| {
                serde_json::json!({
                    "session_id": worker.session_id,
                    "name": worker.name,
                    "label": worker.label,
                    "state": worker.state.label(),
                    "age_ms": worker.age_ms,
                    "meta": worker.meta,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "session_id": self.session_id,
            "title": self.title.to_string(),
            "status": self.status,
            "connection_phase": self.connection_phase,
            "model": self.model,
            "provider": self.provider,
            "reasoning_effort": self.reasoning_effort,
            "composer": self.input.read(cx).snapshot().content,
            "loaded_transcript_items": &*self.items,
            "streaming_text": self.streaming_text,
            "streaming_reasoning": self.streaming_reasoning,
            "ask": ask,
            "workers": workers,
            "workers_open": self.swarm_list_open,
            "expanded_regions": self.expanded_regions,
        })
    }
}
