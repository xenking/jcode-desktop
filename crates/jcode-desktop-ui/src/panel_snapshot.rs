//! A hot-reload cache, not authoritative runtime history or a crash checkpoint.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSnapshot {
    items: Vec<Item>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    applet_positions: HashMap<String, usize>,
    streaming_text: String,
    #[serde(default)]
    live_assistant_output: Option<assistant_output::LiveAssistantOutput>,
    streaming_reasoning: String,
    history_loaded: bool,
    status: String,
    connection_phase: String,
    #[serde(default)]
    turn_outcome: Option<turn::TurnOutcome>,
    #[serde(default)]
    expanded_regions: HashSet<String>,
    #[serde(default)]
    user_times: HashMap<usize, i64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    codemode_summaries: HashMap<String, Box<jcode_sdk::api::CodemodeSummary>>,
    #[serde(default)]
    history_paging: Option<history_paging::HistoryPaging>,
}

impl PartialEq for TranscriptImage {
    fn eq(&self, other: &Self) -> bool {
        self.media_type == other.media_type
            && self.data == other.data
            && self.label == other.label
            && self.session_image == other.session_image
            && self.source == other.source
            && self.anchor == other.anchor
    }
}

impl Panel {
    pub fn snapshot_for_reload(&self, cx: &App) -> PanelSnapshot {
        let mut snapshot = self.snapshot(cx);
        snapshot.transcript = Some(self.transcript_snapshot());
        snapshot
    }

    pub(super) fn transcript_snapshot(&self) -> TranscriptSnapshot {
        TranscriptSnapshot {
            items: self.items.to_vec(),
            applet_positions: self.applet_positions.clone(),
            streaming_text: self.streaming_text.clone(),
            live_assistant_output: self.live_assistant_output.clone(),
            streaming_reasoning: self.streaming_reasoning.clone(),
            history_loaded: self.history_loaded,
            status: self.status.clone(),
            connection_phase: self.connection_phase.clone(),
            turn_outcome: self.turn_outcome,
            expanded_regions: self.expanded_regions.clone(),
            user_times: self.user_times.clone(),
            codemode_summaries: self.codemode_summaries.clone(),
            history_paging: self.history_paging.clone(),
        }
    }

    pub(super) fn restore_transcript(&mut self, snapshot: TranscriptSnapshot) {
        eprintln!(
            "jcode desktop: restored transcript items={} text_bytes={} reasoning_bytes={}",
            snapshot.items.len(),
            snapshot.streaming_text.len(),
            snapshot.streaming_reasoning.len()
        );
        self.restored_transcript = !snapshot.history_loaded
            && (!snapshot.items.is_empty()
                || !snapshot.streaming_text.is_empty()
                || !snapshot.streaming_reasoning.is_empty());
        self.applet_positions = snapshot.applet_positions;
        self.expanded_regions = snapshot.expanded_regions;
        self.user_times = snapshot.user_times;
        self.codemode_summaries = snapshot.codemode_summaries;
        self.history_paging = snapshot.history_paging;
        if let Some(paging) = &mut self.history_paging {
            paging.in_flight = false;
        }
        self.clear_pinned_prompt();
        *self.items = snapshot.items;
        for item in &mut self.items {
            if let Item::Image(image) = item {
                // GPU/image-cache handles never cross the plugin boundary.
                image.preview = TranscriptImage::new(
                    image.media_type.clone(),
                    image.data.clone(),
                    image.label.clone(),
                )
                .preview;
            }
        }
        self.streaming_text = snapshot.streaming_text;
        self.live_assistant_output = snapshot.live_assistant_output;
        self.streaming_reasoning = snapshot.streaming_reasoning;
        // Restored text was already read. Do not replay its reveal.
        self.text_reveal.snap(self.streaming_text.len());
        self.reasoning_reveal.snap(self.streaming_reasoning.len());
        self.history_loaded = snapshot.history_loaded;
        self.status = snapshot.status;
        self.connection_phase = snapshot.connection_phase;
        self.turn_outcome = snapshot.turn_outcome;
        self.transcript_measurements.dirty = true;
    }

    // A reload can precede the first history reply. Preserve the local suffix
    // rather than feeding the cache to the normal prepend-history path.
    pub(super) fn hydrate_restored_prefix(
        &mut self,
        messages: &[jcode_sdk::HistoryMessage],
    ) -> bool {
        if messages.is_empty() {
            return false;
        }
        let local_users: Vec<_> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                Item::AgentMessage {
                    from_session,
                    message,
                    ..
                } if from_session.is_empty() => Some(message.as_str()),
                _ => None,
            })
            .collect();
        let history_users: Vec<_> = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect();
        if !history_users.ends_with(&local_users) {
            return false;
        }
        let prefix_users = history_users.len() - local_users.len();
        let end = messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "user")
            .nth(prefix_users)
            .map(|(index, _)| index)
            .unwrap_or_else(|| {
                messages
                    .iter()
                    .rposition(|m| m.role == "user")
                    .map_or(0, |index| index + 1)
            });
        let mut prefix = Vec::new();
        let mut prefix_times = HashMap::new();
        for message in &messages[..end] {
            match message.role.as_str() {
                "user" => {
                    let item = agent_message::history_user_item(
                        message.content.clone(),
                        !message.attachments.is_empty(),
                        message.timestamp_ms,
                    );
                    if let Some(at) = message.timestamp_ms
                        && matches!(item, Item::User(_))
                    {
                        prefix_times.insert(prefix.len(), at);
                    }
                    prefix.push(item);
                }
                "assistant" if !message.content.trim().is_empty() => {
                    prefix.push(Item::history_assistant(message.content.clone(), message.response_stats.is_some()))
                }
                _ => {}
            }
        }
        let offset = prefix.len();
        self.offscreen_prompt = self.offscreen_prompt.map(|index| index + offset);
        self.expanded_pinned_prompt = self.expanded_pinned_prompt.map(|index| index + offset);
        prefix.append(&mut self.items);
        *self.items = prefix;
        for index in &mut self.pending_users {
            *index += offset;
        }
        self.shift_user_times(offset, 0);
        self.user_times.extend(prefix_times);
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(index, at)| (index + offset, at))
            .collect();
        self.history_loaded = true;
        self.restored_transcript = false;
        true
    }

    pub(super) fn defer_reconnect_history(&mut self, messages: &[jcode_sdk::HistoryMessage]) {
        self.reconnect_response = None;
        let local_users: Vec<_> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Item::User(text) => Some(text.as_str()),
                Item::AgentMessage {
                    from_session,
                    message,
                    ..
                } if from_session.is_empty() => Some(message.as_str()),
                _ => None,
            })
            .collect();
        let history_users: Vec<_> = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .collect();
        // Content alone cannot identify repeated prompts. Compare the whole
        // sequence and never let a shorter persisted view roll a live turn back.
        if local_users.is_empty() || !history_users.starts_with(&local_users) {
            return;
        }
        if history_users.len() > local_users.len() {
            let next_user = messages
                .iter()
                .enumerate()
                .filter(|(_, m)| m.role == "user")
                .nth(local_users.len())
                .map(|(index, _)| index)
                .unwrap();
            let latest_user = messages.iter().rposition(|m| m.role == "user").unwrap();
            // A newer user boundary proves the preceding turn is historical.
            // Preserve its rich tools/images, then hydrate intervening settled
            // turns without copying the newest assistant over queued deltas.
            if let Some(response) = messages[..next_user]
                .iter()
                .rev()
                .take_while(|m| m.role != "user")
                .find(|m| m.role == "assistant" && !m.content.trim().is_empty())
            {
                self.recover_history_assistant(response);
            }
            self.flush_reasoning();
            self.flush_streaming();
            for message in &messages[next_user..=latest_user] {
                match message.role.as_str() {
                    "user" => {
                        let item = agent_message::history_user_item(
                            message.content.clone(),
                            !message.attachments.is_empty(),
                            message.timestamp_ms,
                        );
                        if let Some(at) = message.timestamp_ms
                            && matches!(item, Item::User(_))
                        {
                            self.user_times.insert(self.items.len(), at);
                        }
                        self.items.push(item);
                    }
                    "assistant" if !message.content.trim().is_empty() => {
                        self.items.push(Item::history_assistant(message.content.clone(), message.response_stats.is_some()))
                    }
                    _ => {}
                }
            }
        }
        self.reconnect_response = messages
            .iter()
            .rev()
            .take_while(|message| message.role != "user")
            .find(|message| message.role == "assistant" && !message.content.trim().is_empty())
            .cloned();
    }

    pub(super) fn reconcile_reconnect_response(&mut self) {
        if let Some(response) = self.reconnect_response.take() {
            // Do not flush a different live suffix and append an older stored
            // segment after it. Only equal/extended text can reconcile a stream.
            if self.streaming_text.is_empty() || response.content.starts_with(&self.streaming_text)
            {
                self.recover_history_assistant(&response);
                if let Some(stats) = response.response_stats {
                    self.finish_response();
                    let mut restored: response_stats::ResponseStats = stats.into();
                    restored.at_ms = response.timestamp_ms;
                    if let Some(Item::ResponseStats(existing)) = self.items.last_mut() {
                        restored.duration_secs = restored.duration_secs.or(existing.duration_secs);
                        restored.at_ms = restored.at_ms.or(existing.at_ms);
                        restored.tool_calls = existing.tool_calls;
                        *existing = restored;
                    } else if !restored.is_empty() {
                        self.items.push(Item::ResponseStats(restored));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "panel_snapshot_tests.rs"]
mod tests;
