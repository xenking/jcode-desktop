use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct LiveAssistantOutput {
    id: String,
    closed: bool,
}

impl Panel {
    pub(super) fn append_assistant_delta(&mut self, text: &str, id: Option<&str>) {
        if text.is_empty() {
            return;
        }
        self.flush_reasoning();
        let id = id.filter(|id| !id.is_empty());
        if let Some(id) = id
            && let Some(index) = self.assistant_output_index(id)
            && let Item::AssistantOutput { text: anchored, .. } = &mut self.items[index]
        {
            anchored.push_str(text);
            self.transcript_measurements.invalidate();
            return;
        }
        if self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| id != Some(output.id.as_str()) || output.closed)
        {
            self.settle_assistant_output(AssistantOutputPhase::Update);
        }
        if let Some(id) = id
            && self.live_assistant_output.is_none()
        {
            self.live_assistant_output = Some(LiveAssistantOutput {
                id: id.to_owned(),
                closed: false,
            });
        }
        self.streaming_text.push_str(text);
    }

    pub(super) fn assistant_output_item(&self, text: String) -> Item {
        match &self.live_assistant_output {
            Some(output) => Item::AssistantOutput {
                text,
                phase: AssistantOutputPhase::Pending,
                stream_id: Some(output.id.clone()),
            },
            None => Item::Assistant(text),
        }
    }

    pub(super) fn close_assistant_output(&mut self, id: Option<&str>) {
        if self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| Some(output.id.as_str()) == id)
        {
            self.flush_streaming();
            if let Some(output) = &mut self.live_assistant_output {
                output.closed = true;
            }
        }
    }

    pub(super) fn continue_after_closed_output(&mut self) {
        if self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| output.closed)
        {
            self.settle_assistant_output(AssistantOutputPhase::Update);
        }
    }

    pub(super) fn settle_idle_output(&mut self) {
        if self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| !output.closed)
        {
            self.settle_assistant_output(AssistantOutputPhase::Update);
        }
    }

    pub(super) fn settle_assistant_output(&mut self, phase: AssistantOutputPhase) {
        self.flush_streaming();
        let Some(output) = self.live_assistant_output.take() else {
            return;
        };
        let phase = if phase == AssistantOutputPhase::Final && !output.closed {
            AssistantOutputPhase::Update
        } else {
            phase
        };
        for item in self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
        {
            if let Item::AssistantOutput {
                phase: current,
                stream_id: Some(id),
                ..
            } = item
                && *id == output.id
                && *current == AssistantOutputPhase::Pending
            {
                *current = phase;
            }
        }
        self.transcript_measurements.invalidate();
    }

    pub(super) fn replace_assistant_output(&mut self, id: Option<&str>, text: &str) {
        let Some(id) = id else { return };
        if let Some(index) = self.assistant_output_index(id)
            && let Item::AssistantOutput { text: existing, .. } = &mut self.items[index]
        {
            existing.clear();
            existing.push_str(text);
            self.transcript_measurements.invalidate();
            return;
        }
        let live = self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| output.id == id && !output.closed);
        if live {
            self.streaming_text.clear();
            self.streaming_text.push_str(text);
            self.text_reveal.snap(text.len());
            self.transcript_measurements.invalidate();
        } else if !text.is_empty() {
            self.append_assistant_delta(text, Some(id));
        }
    }

    fn assistant_output_index(&self, id: &str) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, item)| !matches!(item, Item::User(_)))
            .find_map(|(index, item)| {
                matches!(item, Item::AssistantOutput {
                stream_id: Some(candidate), ..
            } if candidate == id)
                .then_some(index)
            })
    }

    pub(super) fn flush_streaming(&mut self) {
        let text = std::mem::take(&mut self.streaming_text);
        self.text_reveal.snap(0);
        if text.is_empty() || (self.live_assistant_output.is_none() && text.trim().is_empty()) {
            return;
        }
        if let Some(output) = &self.live_assistant_output
            && let Some(index) = self.assistant_output_index(&output.id)
            && let Item::AssistantOutput { text: anchored, .. } = &mut self.items[index]
        {
            anchored.push_str(&text);
        } else {
            let item = self.assistant_output_item(text);
            self.items.push(item);
        }
        self.transcript_measurements.invalidate();
    }

    pub(super) fn recover_history_assistant(&mut self, response: &jcode_sdk::HistoryMessage) {
        self.recover_response(&response.content);
        if response.response_stats.is_none() {
            return;
        }
        self.flush_streaming();
        if let Some(item) = self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .find(|item| item.assistant_text() == Some(&response.content))
        {
            match item {
                Item::AssistantOutput { phase, .. } => *phase = AssistantOutputPhase::Final,
                Item::Assistant(text) => {
                    *item = Item::history_assistant(std::mem::take(text), true)
                }
                _ => {}
            }
        }
    }
}
