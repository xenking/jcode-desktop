//! Semantic data exposed by Desktop control, independent of painted pixels.
use super::*;

impl Panel {
    pub(crate) fn control_element_bounds(
        &self,
        selector: &str,
        window: &Window,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        let derived = self.transcript_derived.as_ref()?;
        if derived.key.items_revision != self.items.revision() {
            return None;
        }
        let viewport = self.transcript_list.viewport_bounds();
        let window_bounds = gpui::Bounds {
            origin: point(px(0.), px(0.)),
            size: window.viewport_size(),
        };
        let mut found = None;
        for (index, row) in derived
            .rows
            .iter()
            .enumerate()
            .skip(self.transcript_list.logical_scroll_top().item_ix)
        {
            let Some(bounds) = self.transcript_list.bounds_for_item(index) else {
                break;
            };
            if bounds.top() >= viewport.bottom() {
                break;
            }
            let item = match &row.source {
                TranscriptRowSource::Settled(index) => self.items.get(*index)?,
                TranscriptRowSource::Owned(item) => item.as_ref(),
            };
            let selected = selector.strip_prefix("tool:").is_some_and(
                |call_id| matches!(item, Item::Tool { call_id: id, .. } if id == call_id),
            ) || selector == format!("row:{}:{index}", self.items.revision());
            if selected && bounds.intersects(&viewport) && bounds.intersects(&window_bounds) {
                if found.is_some() {
                    return None;
                }
                found = Some(bounds.intersect(&viewport).intersect(&window_bounds));
            }
        }
        found
    }

    #[cfg(test)]
    pub(crate) fn control_test_tool(&mut self) {
        *self.items = vec![Item::Tool {
            call_id: "call-1".into(),
            name: "read".into(),
            input: "{}".into(),
            output: "result".into(),
            done: true,
            error: None,
        }];
    }
    pub(crate) fn control_toggle_tool(&mut self, call_id: &str, cx: &mut Context<Self>) -> bool {
        if self
            .items
            .iter()
            .filter(|item| matches!(item, Item::Tool { call_id: id, .. } if id == call_id))
            .count()
            != 1
        {
            return false;
        }
        self.toggle_region(call_id, cx);
        true
    }

    pub(crate) fn control_expand_tool(&mut self, call_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(name) = self.items.iter().find_map(|item| match item {
            Item::Tool {
                call_id: id, name, ..
            } if id == call_id => Some(name.as_str()),
            _ => None,
        }) else {
            return false;
        };
        let expanded = self.expanded_regions.contains(call_id)
            ^ (name == "agentgrep" && crate::config::display().show_agentgrep_output);
        if !expanded {
            self.toggle_region(call_id, cx);
        }
        true
    }

    pub(crate) fn control_state(&self, window: &Window, cx: &App) -> serde_json::Value {
        // Bounds refer to the last laid-out frame. Do not associate stale row
        // descriptors with a newer transcript after an event, before repaint.
        let visible = self
            .transcript_derived
            .as_ref()
            .filter(|derived| derived.key.items_revision == self.items.revision())
            .map(|derived| {
                let viewport = self.transcript_list.viewport_bounds();
                let window_bounds = gpui::Bounds {
                    origin: point(px(0.), px(0.)),
                    size: window.viewport_size(),
                };
                let mut visible = Vec::new();
                for (index, row) in derived
                    .rows
                    .iter()
                    .enumerate()
                    .skip(self.transcript_list.logical_scroll_top().item_ix)
                {
                    let Some(bounds) = self.transcript_list.bounds_for_item(index) else {
                        break;
                    };
                    if bounds.top() >= viewport.bottom() {
                        break;
                    }
                    if !bounds.intersects(&viewport) || !bounds.intersects(&window_bounds) {
                        continue;
                    }
                    let item = match &row.source {
                        TranscriptRowSource::Settled(index) => match self.items.get(*index) {
                            Some(item) => item,
                            None => continue,
                        },
                        TranscriptRowSource::Owned(item) => item.as_ref(),
                    };
                    let selector = match item {
                        Item::Tool { call_id, .. } => format!("tool:{call_id}"),
                        _ => format!("row:{}:{index}", self.items.revision()),
                    };
                    visible
                        .push(serde_json::json!({"row": index, "selector":selector, "item": item}));
                }
                visible
            });
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
            "visible_transcript_items": visible,
            "streaming_text": self.streaming_text,
            "streaming_reasoning": self.streaming_reasoning,
            "ask": ask,
            "workers": workers,
            "workers_open": self.swarm_list_open,
            "expanded_regions": self.expanded_regions,
        })
    }
}
