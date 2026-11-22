use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssistantOutputPhase {
    Pending,
    Update,
    Final,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct LiveAssistantOutput {
    id: String,
    closed: bool,
}

impl Item {
    pub(super) fn assistant_text(&self) -> Option<&str> {
        match self {
            Self::Assistant(text) | Self::AssistantOutput { text, .. } => Some(text),
            _ => None,
        }
    }

    pub(super) fn history_assistant(text: String, final_marker: bool) -> Self {
        if final_marker {
            Self::AssistantOutput {
                text,
                phase: AssistantOutputPhase::Final,
                stream_id: None,
            }
        } else {
            Self::Assistant(text)
        }
    }
}

impl Panel {
    pub(super) fn append_assistant_delta(&mut self, text: &str, id: Option<&str>) {
        if text.is_empty() {
            return;
        }
        self.flush_reasoning();
        if self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| id != Some(output.id.as_str()) || output.closed)
        {
            self.settle_assistant_output(AssistantOutputPhase::Update);
        }
        if let Some(id) = id.filter(|id| !id.is_empty())
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
        self.transcript_measurements.dirty = true;
    }

    pub(super) fn replace_assistant_output(&mut self, id: Option<&str>, text: &str) {
        let Some(id) = id else { return };
        let live = self
            .live_assistant_output
            .as_ref()
            .is_some_and(|output| output.id == id && !output.closed);
        let mut fragments = self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .filter_map(|item| match item {
                Item::AssistantOutput {
                    text,
                    stream_id: Some(old),
                    phase,
                } if old == id && *phase != AssistantOutputPhase::Final => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        if fragments.is_empty() && !live {
            if !text.is_empty() {
                self.append_assistant_delta(text, Some(id));
            }
            return;
        }
        let mut remaining = text;
        let mut fragments = fragments.drain(..).rev().peekable();
        while let Some(fragment) = fragments.next() {
            let mut end = if live || fragments.peek().is_some() {
                fragment.len().min(remaining.len())
            } else {
                remaining.len()
            };
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            fragment.replace_range(.., &remaining[..end]);
            remaining = &remaining[end..];
        }
        if live {
            self.streaming_text.replace_range(.., remaining);
        }
        self.transcript_measurements.dirty = true;
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
