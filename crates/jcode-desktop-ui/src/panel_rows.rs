//! The transcript as rows: deriving them from items and drawing each one.
use super::*;

impl Panel {
    /// Refresh transcript applet placement from the shared runtime. Cards
    /// anchored to a tool call render in that call's row. Everything else
    /// (`end`, `after_message`, and tool anchors whose call is not in this
    /// transcript, such as batch subcalls) is pinned at the position where
    /// it first appeared so it scrolls with the conversation.
    pub(crate) fn sync_applet_rows(&mut self, cx: &mut Context<Self>) {
        use jcode_applet_types::{Placement, placement::Anchor};
        let runtime = crate::applet_runtime::get(cx);
        let generation = runtime.generation.get();
        let clean = self.clean_chat_enabled();
        let key = (generation, self.items.revision(), clean);
        if key == self.applet_generation {
            return;
        }
        self.applet_generation = key;
        let host = runtime.host.borrow();
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        let mut receipts = None;
        for mounted in host.inline_for(&self.session_id) {
            let id = mounted.instance.id.clone();
            if let Placement::Inline {
                anchor: Anchor::ToolCall { call_id },
                ..
            } = &mounted.instance.placement
            {
                if clean
                    && let Some(index) = self.items.iter().position(
                        |item| matches!(item, Item::Tool { call_id: c, .. } if c == call_id),
                    )
                {
                    rows.push((index, id));
                }
                continue;
            }
            // Older snapshots only stored End. Recover the original mount, not a later update.
            let position = if mounted.instance.applet == jcode_applet_types::agent::APPLET_ID {
                let origins = receipts.get_or_insert_with(|| {
                    let mut origins = HashMap::new();
                    for (index, item) in self.items.iter().enumerate() {
                        if let Item::Tool { name, output, .. } = item
                            && name == "applet"
                            && let Some((local, rest)) = output
                                .trim_start()
                                .strip_prefix("Applet `")
                                .and_then(|s| s.split_once("` revision "))
                            && rest.starts_with("1.")
                        {
                            origins
                                .entry(crate::applet_runtime::agent_instance_id(
                                    &self.session_id,
                                    local,
                                ))
                                .or_insert(index + 1);
                        }
                    }
                    origins
                });
                let Some(&position) = origins.get(&id) else {
                    continue;
                };
                position
            } else {
                *self
                    .applet_positions
                    .entry(id.clone())
                    .or_insert(self.items.len())
            };
            seen.insert(id.clone());
            rows.push((position, id));
        }
        drop(host);
        // The panel snapshot may arrive before the first runtime snapshot.
        if !rows.is_empty() || !seen.is_empty() {
            self.applet_positions.retain(|id, _| seen.contains(id));
        }
        rows.sort_by_key(|(position, _)| *position);
        if rows.is_empty() && self.applet_rows.is_empty() {
            return;
        }
        // Any card change (mount, patch, close) can change row heights.
        self.transcript_measurements.invalidate();
        self.applet_rows = rows;
        self.applet_rows_version = self.applet_rows_version.wrapping_add(1);
    }

    /// Focus and scroll handles are created when a tool or agent-message row
    /// first renders. Drop the ones whose row left the transcript, so a history
    /// reload or rewind does not leave a handle behind for every past call.
    pub(crate) fn prune_row_state(&self) {
        if self.swarm_row_focus.borrow().is_empty() && self.swarm_scrolls.borrow().is_empty() {
            return;
        }
        let mut live: std::collections::HashSet<std::borrow::Cow<str>> = Default::default();
        for (index, item) in self.items.iter().enumerate() {
            match item {
                Item::Tool { call_id, .. } => {
                    live.insert(call_id.as_str().into());
                }
                Item::AgentMessage { .. } => {
                    live.insert(expandable::Key::details(index).into());
                }
                _ => {}
            }
        }
        self.swarm_row_focus
            .borrow_mut()
            .retain(|key, _| live.contains(key.as_str()));
        self.swarm_scrolls
            .borrow_mut()
            .retain(|key, _| live.contains(key.as_str()));
    }

    /// Rows, pinnable prompt rows and the latest todo snapshot, rebuilt only
    /// when the transcript, live text, applet rows or disclosure state change.
    /// The selectable text document is synced on the same schedule.
    pub(crate) fn transcript_derived(&mut self, cx: &mut Context<Self>) -> &TranscriptDerived {
        use std::hash::{Hash, Hasher};
        let display = crate::config::display();
        let clean_chat = self.clean_chat_with_display(&display);
        let streaming = {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            if !clean_chat {
                self.reasoning_reveal
                    .visible(&self.streaming_reasoning)
                    .hash(&mut state);
            }
            self.text_reveal
                .visible(&self.streaming_text)
                .hash(&mut state);
            state.finish()
        };
        let expansions = {
            // Order-independent: the set is small and unordered.
            let mut sum = 0u64;
            for entry in &self.expanded_regions {
                let mut state = std::collections::hash_map::DefaultHasher::new();
                entry.hash(&mut state);
                sum = sum.wrapping_add(state.finish().rotate_left(17));
            }
            sum ^ self.expanded_regions.len() as u64
        };
        let key = TranscriptDerivedKey {
            items_revision: self.items.revision(),
            applet_rows_version: self.applet_rows_version,
            streaming,
            expansions,
            display,
            clean_chat,
        };
        if self
            .transcript_derived
            .as_ref()
            .is_none_or(|derived| derived.key != key)
        {
            if self.transcript_derived.as_ref().is_some_and(|derived| {
                derived.key.display != key.display || derived.key.clean_chat != clean_chat
            }) {
                self.transcript_measurements.invalidate();
            }
            let rows = Arc::new(self.transcript_render_rows_with_display(&key.display));
            if let Some(document) = self.transcript_text_document.sync(
                &self.items,
                &rows,
                &self.expanded_regions,
            ) {
                self.transcript_selection
                    .update(cx, |selection, _| selection.set_document(document));
            }
            // Live rows are appended after settled rows. Their growth and
            // disclosure changes cannot move a settled prompt's row index.
            let prompt_rows = self
                .transcript_derived
                .as_ref()
                .filter(|derived| {
                    derived.key.items_revision == key.items_revision
                        && derived.key.applet_rows_version == key.applet_rows_version
                        && derived.key.display == key.display
                        && derived.key.clean_chat == clean_chat
                })
                .map(|derived| derived.prompt_rows.clone())
                .unwrap_or_else(|| {
                    Arc::new(
                        rows.iter()
                            .enumerate()
                            .filter_map(|(row, entry)| match entry.source {
                                TranscriptRowSource::Settled(index)
                                    if classify::is_pinnable_prompt(&self.items[index]) =>
                                {
                                    Some((row, index))
                                }
                                _ => None,
                            })
                            .collect(),
                    )
                });
            if self
                .transcript_derived
                .as_ref()
                .is_none_or(|derived| derived.key.items_revision != key.items_revision)
            {
                self.prune_row_state();
            }
            // Live text and disclosure changes rebuild rows, not settled todo
            // state. Do not rescan history, clone the payload or format its
            // fingerprint for every streaming chunk.
            let (latest_todo, tail_headers) = self
                .transcript_derived
                .as_ref()
                .filter(|derived| derived.key.items_revision == key.items_revision)
                .map(|derived| (derived.latest_todo.clone(), derived.tail_headers))
                .unwrap_or_else(|| {
                    let latest_todo = self.latest_todo_payload().map(Arc::new);
                    let tail_headers = transcript_tail_headers(latest_todo.as_deref());
                    (latest_todo, tail_headers)
                });
            let active_error = clean_chat
                .then(|| clean_chat::active_error(&self.items))
                .flatten();
            self.transcript_derived = Some(TranscriptDerived {
                key,
                rows,
                prompt_rows,
                latest_todo,
                tail_headers,
                active_error,
            });
        }
        self.transcript_derived.as_ref().unwrap()
    }

    #[cfg(test)]
    pub(crate) fn test_transcript_rows_ptr(&self) -> Option<usize> {
        self.transcript_derived
            .as_ref()
            .map(|derived| Arc::as_ptr(&derived.rows) as usize)
    }

    #[cfg(test)]
    pub(crate) fn test_transcript_row_count(&self) -> usize {
        self.transcript_derived
            .as_ref()
            .map_or(0, |derived| derived.rows.len())
    }

    pub(crate) fn transcript_render_rows_with_display(
        &self,
        display: &crate::config::DisplayConfig,
    ) -> Vec<TranscriptRenderRow> {
        let clean_chat = self.clean_chat_with_display(display);
        let detail_items = if clean_chat {
            &[]
        } else {
            self.items.as_slice()
        };
        let mut rows: Vec<TranscriptRenderRow> = Vec::with_capacity(self.items.len() + 2);
        let mut previous_role = None;
        let mut inline_images = HashSet::new();
        let mut user_attachments_end = 0;
        let completed_asks: HashSet<&str> = detail_items
            .iter()
            .filter_map(|item| match item {
                Item::Tool {
                    call_id,
                    name,
                    done: true,
                    ..
                } if name == "ask" => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        // A completion notice is redundant only when a completed bg call
        // already contains the same task, status and actual output preview.
        // Keep nonmatching previews and ongoing tasks: they carry distinct data.
        let completed_bg: Vec<(String, String, String)> = detail_items
            .iter()
            .filter_map(|item| {
                let Item::Tool {
                    name,
                    input,
                    output,
                    done: true,
                    error: None,
                    ..
                } = item
                else {
                    return None;
                };
                let result = tool_output::background_output(name, output)?;
                let task = serde_json::from_str::<serde_json::Value>(input)
                    .ok()?
                    .get("task_id")?
                    .as_str()?
                    .to_owned();
                if !result
                    .fields
                    .iter()
                    .any(|line| line == &format!("Task: {task}"))
                {
                    return None;
                }
                let status = result
                    .fields
                    .iter()
                    .find_map(|line| line.strip_prefix("Status: "))?
                    .trim()
                    .to_owned();
                let preview = result.preview?;
                if status.is_empty() || preview.trim().is_empty() {
                    return None;
                }
                Some((task, status, preview))
            })
            .collect();
        let mut applets = self.applet_rows.iter().enumerate().peekable();
        let applet_row = |ordinal: usize, instance: &String| TranscriptRenderRow {
            // Derived rows need indices that never collide with items or the
            // live streaming sentinels at the top of the range.
            index: usize::MAX / 2 + ordinal,
            source: TranscriptRowSource::Owned(Box::new(Item::Applet(instance.clone()))),
            role: None,
            show_label: false,
        };

        for (index, item) in self.items.iter().enumerate() {
            while let Some((ordinal, (_, instance))) =
                applets.next_if(|(_, (position, _))| *position <= index)
            {
                rows.push(applet_row(ordinal, instance));
            }
            let mut source = TranscriptRowSource::Settled(index);
            if !clean_chat && matches!(item, Item::SideQuestion(_)) {
                continue;
            }
            if clean_chat {
                if !clean_chat::conversation_item(item)
                    && !matches!(item, Item::Tool { name, input, output, call_id, done: true, error }
                        if error.is_none() && swarm_card::is_external_send(name, input, output)
                            || self.codemode_summaries.get(call_id).is_some_and(|summary|
                                Self::external_codemode_messages(summary).next().is_some()))
                {
                    continue;
                }
                if let Some(text) = item.assistant_text() {
                    let public = markdown::public_transcript(text);
                    if public.trim().is_empty() {
                        continue;
                    }
                    if let std::borrow::Cow::Owned(text) = public {
                        let public = match item {
                            Item::AssistantOutput { phase, stream_id, .. } => Item::AssistantOutput {
                                text,
                                phase: *phase,
                                stream_id: stream_id.clone(),
                            },
                            _ => Item::Assistant(text),
                        };
                        source = TranscriptRowSource::Owned(Box::new(public));
                    }
                }
            }
            if matches!(item, Item::User(_)) {
                user_attachments_end = if classify::is_pinnable_prompt(item) {
                    self.user_attachment_range(index).end
                } else {
                    index + 1
                };
                inline_images = self
                    .user_image_markers(index)
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect();
            }
            if inline_images.contains(&index) {
                continue;
            }
            if matches!(item, Item::AssistantOutput { text, .. } if text.trim().is_empty()) {
                continue;
            }
            // A cached transcript from before the history merge fix can still
            // contain detached copies appended after an unrelated tool row.
            if index >= user_attachments_end
                && matches!(item, Item::Image(image)
                    if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput)
                        && matches!(image.anchor, Some(jcode_sdk::RenderedImageAnchor::UserPrompt { .. })))
            {
                continue;
            }
            if let Item::BackgroundTask {
                task_id,
                summary,
                detail: Some(detail),
                done: true,
                ..
            } = item
                && let Some((_, rest)) = detail.split_once("```text\n")
                && let Some((preview, _)) = rest.rsplit_once("\n```")
                && completed_bg.iter().any(|(id, status, output)| {
                    id == task_id
                        && summary.contains(status)
                        && preview.trim_end() == output.trim_end()
                })
            {
                continue;
            }
            // Ask-result attachments render inside their answer card. Keep
            // the image items for the session image browser and history.
            if let Item::Image(image) = item
                && matches!(&image.source, jcode_sdk::RenderedImageSource::ToolResult { tool_name } if tool_name == "ask")
                && let Some(jcode_sdk::RenderedImageAnchor::ToolCall { id }) = &image.anchor
                && completed_asks.contains(id.as_str())
            {
                continue;
            }
            if matches!(item, Item::Reasoning(_))
                && display.thinking_display != crate::config::ThinkingDisplay::Full
            {
                continue;
            }
            if display.screenshot_mode && matches!(item, Item::ResponseStats(_)) {
                continue;
            }
            if matches!(item, Item::Todos(_))
                || matches!(item, Item::Tool { name, .. } if name == "todo")
            {
                continue;
            }

            if let Item::Reasoning(text) = item
                && let Some(previous) = rows.last_mut()
            {
                match &mut previous.source {
                    TranscriptRowSource::Settled(previous_index) => {
                        if let Item::Reasoning(existing) = &self.items[*previous_index] {
                            let mut joined = existing.clone();
                            append_reasoning_text(&mut joined, text);
                            previous.source =
                                TranscriptRowSource::Owned(Box::new(Item::Reasoning(joined)));
                            continue;
                        }
                    }
                    TranscriptRowSource::Owned(item) => {
                        if let Item::Reasoning(existing) = item.as_mut() {
                            append_reasoning_text(existing, text);
                            continue;
                        }
                    }
                }
            }

            let role = role_of(item);
            rows.push(TranscriptRenderRow {
                index,
                source,
                role,
                show_label: role.is_some() && role != previous_role,
            });
            previous_role = role.or(previous_role);
        }

        for (ordinal, (_, instance)) in applets {
            rows.push(applet_row(ordinal, instance));
        }

        for (index, item) in [
            (!clean_chat
                && !self.streaming_reasoning.is_empty()
                && display.thinking_display != crate::config::ThinkingDisplay::Off)
                .then(|| {
                    (
                        usize::MAX - 1,
                        Item::Reasoning(
                            self.reasoning_reveal
                                .visible(&self.streaming_reasoning)
                                .to_owned(),
                        ),
                    )
                }),
            (!self.streaming_text.is_empty()).then(|| {
                (
                    usize::MAX,
                    self.assistant_output_item({
                        let text = self.text_reveal.visible(&self.streaming_text);
                        if clean_chat {
                            markdown::public_transcript(text).into_owned()
                        } else {
                            text.to_owned()
                        }
                    }),
                )
            }),
        ]
        .into_iter()
        .flatten()
        {
            let role = role_of(&item);
            rows.push(TranscriptRenderRow {
                index,
                source: TranscriptRowSource::Owned(Box::new(item)),
                role,
                show_label: role.is_some() && role != previous_role,
            });
            previous_role = role.or(previous_role);
        }

        rows
    }

    /// Flip one disclosure region (`expandable::Open`). A tool card fades its
    /// detail in and out; every other region appears at once.
    pub(crate) fn toggle_region(&mut self, key: &str, cx: &mut Context<Self>) {
        let was_expanded = self.expanded_regions.contains(key);
        if !self.expanded_regions.remove(key) {
            self.expanded_regions.insert(key.to_owned());
        }
        let duration = crate::transition::policy(crate::transition::Transition::Overlay).duration;
        if cx.reduce_motion() || duration.is_zero() {
            self.tool_detail_motion.remove(key);
        } else {
            self.tool_detail_motion
                .entry(key.to_owned())
                .or_insert_with(|| {
                    crate::transition::AnimatedValue::new(
                        if was_expanded { 1.0 } else { 0.0 },
                        duration,
                    )
                })
                .set(if was_expanded { 0.0 } else { 1.0 }, Instant::now());
        }
        self.transcript_measurements.invalidate();
        cx.notify();
    }

    /// Flip one disclosure region that has no animated detail: a notice body,
    /// a background task, a swarm line or an error card's raw message.
    pub(crate) fn request_background_output(&self, task_id: &str) {
        if !self.is_side_document() && self.preview_state.is_none() {
            self.bridge.send(Command::RuntimeCommand {
                session_id: self.session_id.clone(),
                command: jcode_sdk::api::RuntimeCommand::BackgroundOutput,
                args: vec![task_id.to_owned()],
            });
        }
    }
}
