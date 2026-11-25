//! The transcript as rows: deriving them from items and drawing each one.
use super::*;

impl Panel {
    /// Refresh transcript applet placement from the shared runtime. Cards
    /// anchored to a tool call render in that call's row. Everything else
    /// (`end`, `after_message`, and tool anchors whose call is not in this
    /// transcript, such as batch subcalls) is pinned at the position where
    /// it first appeared so it scrolls with the conversation.
    pub(super) fn sync_applet_rows(&mut self, cx: &mut Context<Self>) {
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
    pub(super) fn prune_row_state(&self) {
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
                    live.insert(expandable::key::details(index).into());
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
    pub(super) fn transcript_derived(&mut self, cx: &mut Context<Self>) -> &TranscriptDerived {
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

    pub(super) fn transcript_render_rows(&self) -> Vec<TranscriptRenderRow> {
        self.transcript_render_rows_with_display(&crate::config::display())
    }

    pub(super) fn transcript_render_rows_with_display(
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

    /// Compact elapsed time: ticking while a tool runs, final duration after.
    pub(super) fn render_tool_clock(
        &self,
        index: usize,
        call_id: &str,
        finished: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let clock = compact_clock();
        let start = self
            .tool_wall_times
            .get(call_id)
            .and_then(|(start, _)| *start)
            .and_then(tool_clock_label);
        if finished {
            let duration = self
                .tool_durations
                .get(call_id)
                .map(|duration| format_tool_elapsed(*duration));
            let label = [start, duration]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            (!label.is_empty()).then(|| clock.child(label).into_any_element())
        } else {
            let started = *self.tool_started.get(call_id)?;
            let _ = index;
            // Ticks in its own view: refreshing the clock must not rebuild
            // the transcript around it.
            let ticker = self.running_tool_tickers.borrow_mut().get(
                &format!("clock:{call_id}"),
                crate::pulse_text::Kind::Elapsed {
                    since: started,
                    format: format_tool_elapsed,
                },
                cx,
            );
            Some(
                clock
                    .children(start.map(|start| format!("{start} · ")))
                    .child(ticker)
                    .into_any_element(),
            )
        }
    }

    pub(super) fn tool_timing_detail(&self, call_id: &str) -> String {
        let (start, finish) = self
            .tool_wall_times
            .get(call_id)
            .copied()
            .unwrap_or_default();
        let start = start
            .and_then(tool_clock_label)
            .unwrap_or_else(|| "unknown".into());
        let finish = finish
            .and_then(tool_clock_label)
            .unwrap_or_else(|| "unknown".into());
        let duration = self
            .tool_durations
            .get(call_id)
            .copied()
            .or_else(|| self.tool_started.get(call_id).map(Instant::elapsed))
            .map(format_tool_elapsed)
            .unwrap_or_else(|| "unknown".into());
        format!("Started at: {start}   Finished at: {finish}   Duration: {duration}")
    }

    pub(super) fn render_item(
        &self,
        index: usize,
        item: &Item,
        display: &crate::config::DisplayConfig,
        _show_avatar: bool,
        _response_viewport_height: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if display.screenshot_mode
            && matches!(
                item,
                Item::Error(_) | Item::RuntimeError { .. } | Item::BackgroundTask { .. }
            )
        {
            return div()
                .px_1()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(
                    if matches!(item, Item::Error(_) | Item::RuntimeError { .. }) {
                        "Error details hidden"
                    } else {
                        "Background task"
                    },
                )
                .into_any_element();
        }
        let notice_title = match item {
            Item::Stopped(notice) => Some(notice.title()),
            Item::CacheMiss(_) => Some("Cache miss"),
            _ => None,
        };
        if display.screenshot_mode
            && let Some(title) = notice_title
        {
            return div()
                .px_2()
                .py_1()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(title.to_owned())
                .into_any_element();
        }
        match item {
            Item::Applet(instance) => {
                let selection = self.applet_selection.clone();
                crate::applet_surface::card_with_tasks(
                    instance, true, &selection,
                    Some((&self.session_id, &|app| self.applet_session_tasks(app))), window, cx,
                )
                    .unwrap_or_else(|| div().into_any_element())
            }
            Item::Stopped(notice) => self.render_stop_notice(index, notice, window, cx),
            Item::SwarmWorkerNotice(text) => div()
                .debug_selector(|| "swarm-worker-notice".into())
                .min_w_0()
                .px_2()
                .py_1()
                .truncate()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(text_selection::plain(
                    self.transcript_selection.clone(),
                    format!("{index}-swarm-worker-notice"),
                    text.clone(),
                    window,
                    cx,
                ))
                .into_any_element(),
            Item::ProviderGuardrailRetry { text, .. } => div()
                .debug_selector(|| "provider-guardrail-retry".into())
                .min_w_0()
                .px_2()
                .py_1()
                .truncate()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(text_selection::plain(
                    self.transcript_selection.clone(),
                    format!("{index}-provider-guardrail-retry"),
                    text.clone(),
                    window,
                    cx,
                ))
                .into_any_element(),
            Item::Compaction(row) => self.render_compaction_divider(index, row, window, cx),
            Item::Memory(row) => self.render_memory_divider(index, row, window, cx),
            Item::CacheMiss(notice) => self.render_cache_miss_notice(index, notice, window, cx),
            Item::ResponseStats(stats) => stats.render(index).into_any_element(),
            Item::User(text) if jcode_sdk::todo::is_auto_poke_message(text) => {
                self.render_auto_poke_notice(index, text, window, cx)
            }
            Item::User(text) => self.render_user_prompt(index, text, None, window, cx),
            Item::Image(image) => {
                if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
                    return self.render_user_image_chip(index, image, cx);
                }
                if self.image_pane_open && !display.screenshot_mode {
                    return self.render_image_pane_link(index, image, cx);
                }
                let panel = cx.entity().downgrade();
                let label = if display.screenshot_mode {
                    "Image attachment".to_string()
                } else {
                    image.display_label("Image attachment")
                };
                div()
                    .id(("transcript-image", index))
                    .debug_selector(|| "transcript-image".into())
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .when_some(image.preview.clone(), |el, preview| {
                        let scroll_panel = panel.clone();
                        let gesture_panel = panel.clone();
                        el.child(
                            crate::inline_image::InlineImage::new(index, preview)
                                .metadata(label.clone())
                                .on_fit_scroll(move |event, window, cx| {
                                    let _ = scroll_panel.update(cx, |panel, cx| {
                                        if panel.transcript_list.is_scrollbar_dragging() {
                                            return;
                                        }
                                        let y = f32::from(
                                            event.delta.pixel_delta(window.line_height()).y,
                                        );
                                        if y == 0.0 {
                                            return;
                                        }
                                        if event.delta.precise() {
                                            panel.glide_transcript_input(-y, true, cx);
                                        } else {
                                            panel.glide_transcript_wheel(-y, cx);
                                        }
                                    });
                                })
                                .on_gesture(move |_, cx| {
                                    let _ = gesture_panel.update(cx, |panel, _| {
                                        panel.cancel_transcript_momentum();
                                    });
                                }),
                        )
                    })
                    .when(image.preview.is_none(), |el| {
                        let lazy_image = image.clone();
                        let lazy = image.data.starts_with("jcode-lazy:");
                        el.child(
                            div()
                                .id(("history-image-placeholder", index))
                                .debug_selector(|| "history-image-placeholder".into())
                                .text_size(px(11.0))
                                .text_color(Theme::global().TEXT_DIM)
                                .when(lazy, |el| {
                                    el.cursor_pointer().on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.open_image_preview(lazy_image.clone(), window, cx);
                                        },
                                    ))
                                })
                                .child(if lazy {
                                    format!("{label} (click to load)")
                                } else {
                                    format!("{label} (could not display {})", image.media_type)
                                }),
                        )
                    })
                    .into_any_element()
            }
            Item::Assistant(text) | Item::AssistantOutput { text, .. } => div()
                .debug_selector(|| "assistant-response".into())
                .font_family(Theme::global().FONT_AI)
                .px_1()
                .text_color(Theme::global().TEXT)
                .child(markdown::with_stream_fade(
                    if index == usize::MAX {
                        self.text_reveal.fade(&self.streaming_text)
                    } else {
                        markdown::StreamFade::default()
                    },
                    || {
                        markdown::render_interactive(
                            text,
                            index,
                            &self.transcript_selection,
                            window,
                            cx,
                            false,
                            self.media_preview_handler(cx),
                        )
                        .into_any_element()
                    },
                ))
                .into_any_element(),
            Item::AgentMessage {
                from_session,
                from_name,
                message,
                timestamp_ms,
            } => self.render_agent_message(
                index,
                from_session,
                from_name.as_deref(),
                message,
                *timestamp_ms,
                window,
                cx,
            ),
            Item::SideQuestion(question) => {
                self.render_inline_side_question(index, question, window, cx)
            }
            // Thinking is secondary transcript text, not a separate card. Keep
            // the same presentation while streaming, settled, and restored.
            // Never truncate it: there is deliberately no disclosure control.
            Item::Reasoning(text) => div()
                .debug_selector(|| "reasoning-inline".into())
                .font_family(Theme::global().FONT_AI)
                .flex_none()
                .px_1()
                .text_size(px(12.0))
                .italic()
                .text_color(Theme::global().REASONING)
                .child(markdown::with_stream_fade(
                    if index == usize::MAX - 1 {
                        self.reasoning_reveal.fade(&self.streaming_reasoning)
                    } else {
                        markdown::StreamFade::default()
                    },
                    || {
                        markdown::render_interactive(
                            text,
                            index,
                            &self.transcript_selection,
                            window,
                            cx,
                            true,
                            self.media_preview_handler(cx),
                        )
                        .into_any_element()
                    },
                ))
                .into_any_element(),
            Item::Todos(payload) => render_todo_card_with_style(
                payload,
                false,
                &format!("todo-{index}"),
                &self.transcript_selection,
                window,
                cx,
            ),
            Item::BackgroundTask {
                task_id,
                label,
                summary,
                detail,
                percent,
                done,
            } => {
                let key = expandable::key::task(task_id);
                let expanded = self.expanded_regions.contains(&key);
                let (row_summary, exit_code) = background_task::summary_parts(summary);
                // One ellipsis budget, with the title ahead of progress text.
                let mut header = label.clone();
                let summary_start = header.len();
                if !row_summary.is_empty() {
                    header.push_str(" · ");
                    header.push_str(&row_summary);
                }
                let summary_style = (summary_start < header.len()).then(|| {
                    (
                        summary_start..header.len(),
                        gpui::HighlightStyle {
                            color: Some(crate::theme::to_hsla(Theme::global().TEXT_FAINT)),
                            ..Default::default()
                        },
                    )
                });
                let header = gpui::StyledText::new(header).with_highlights(summary_style);
                let toggle_key = key.clone();
                let background_task_id = task_id.clone();
                let done_for_click = *done
                    && detail
                        .as_deref()
                        .is_some_and(|value| value.contains("_Full output:_"));
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .min_w_0()
                    .debug_selector(|| "background-task-card".into())
                    .child(
                        div()
                            .id(("background-task-toggle", index))
                            .debug_selector(|| "background-task-expand".into())
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .min_w_0()
                            .overflow_hidden()
                            .px_1()
                            .py(px(2.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                if !this.expanded_regions.contains(&toggle_key) && done_for_click {
                                    this.request_background_output(&background_task_id);
                                }
                                this.toggle_line(&toggle_key, cx);
                            }))
                            .child(crate::tool_icon::render_badge(
                                "bg",
                                *done,
                                *done
                                    && (exit_code.is_some()
                                        || summary.to_lowercase().contains("failed")),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .debug_selector(|| "background-task-title".into())
                                    .text_color(Theme::global().TOOL_TEXT)
                                    .child(header),
                            )
                            .when_some(exit_code, |el, code| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .px_2()
                                        .rounded_full()
                                        .bg(Theme::global().INLINE_CODE_BG)
                                        .text_color(Theme::global().ERROR)
                                        .child(format!("exit {code}")),
                                )
                            })
                            .when_some(
                                percent.filter(|value| !*done && value.is_finite()),
                                |el, percent| {
                                    el.child(
                                        div()
                                            .flex_none()
                                            .w(px(40.0))
                                            .debug_selector(|| "background-task-progress".into())
                                            .h(px(2.0))
                                            .rounded_full()
                                            .bg(Theme::global().TOOL_BORDER)
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(relative((percent / 100.0).clamp(0.0, 1.0)))
                                                    .rounded_full()
                                                    .bg(Theme::global().WARN),
                                            ),
                                    )
                                },
                            )
                            .children(tool_card_token_pill(detail.as_deref().unwrap_or("")))
                            .when(crate::fixture_control_targets::background_tasks(), |el| {
                                el.relative()
                                    .children(crate::fixture_control_targets::marker(
                                        || key.clone(),
                                        || Some(self.session_id.clone()),
                                    ))
                            }),
                    )
                    .when(expanded, |el| {
                        el.child(
                            div()
                                .id(("background-task-output-scroll", index))
                                .debug_selector(|| "background-task-output".into())
                                .ml(px(24.0))
                                .mt_1()
                                .p_3()
                                .rounded_lg()
                                .max_h(px(expandable::MAX_HEIGHT))
                                .bg(Theme::global().CODE_BG)
                                .font_family(Theme::global().FONT_MONO)
                                .text_size(px(11.5))
                                .text_color(Theme::global().CODE_TEXT)
                                .child(text_selection::plain(
                                    self.transcript_selection.clone(),
                                    format!("background-detail-{index}"),
                                    background_task::detail_text(
                                        task_id,
                                        summary,
                                        detail.as_deref(),
                                    ),
                                    window,
                                    cx,
                                ))
                                .map(crate::transcript_scroll::vertical),
                        )
                    })
                    .into_any_element()
            }
            Item::Tool {
                call_id,
                name,
                input,
                output,
                done,
                error,
            } => {
                if name == "ask" && *done {
                    return self.render_decision(
                        index,
                        call_id,
                        input,
                        output,
                        error.as_deref(),
                        window,
                        cx,
                    );
                }
                let (offset, opacity, animating) = self
                    .arriving_tools
                    .get(call_id)
                    .map(|started_at| {
                        crate::transition::arrival_motion(
                            *started_at,
                            Instant::now(),
                            crate::transition::policy(crate::transition::Transition::ToolArrival)
                                .duration,
                        )
                    })
                    .unwrap_or((0.0, 1.0, false));
                if animating {
                    window.request_animation_frame();
                }
                let is_codemode = classify::is_codemode(name);
                let codemode = (is_codemode && *done)
                    .then(|| self.codemode_summaries.get(call_id).map(Box::as_ref))
                    .flatten();
                let failed = error.is_some()
                    || (is_codemode && classify::wrapper_failed(codemode, None, output));
                let nested = codemode.and_then(|summary| {
                    codemode_card::single_swarm(summary).map(|call| (summary, call))
                });
                if !display.screenshot_mode {
                    if let Some(call) = nested
                        .map(|(_, call)| swarm_card::from_codemode(call, input))
                        .or_else(|| swarm_card::parse(name, input, output))
                    {
                        return self.render_swarm_card(
                            index, call_id, &call, name, input, *done, output, error.as_deref(),
                            nested, offset, opacity, window, cx,
                        );
                    }
                }
                if self.clean_chat_with_display(&display) && let Some(summary) = codemode {
                    return div()
                        .children(Self::render_external_codemode_messages(summary))
                        .into_any_element();
                }
                let expanded = self.expanded_regions.contains(call_id)
                    ^ (name == "agentgrep" && display.show_agentgrep_output);
                let detail_progress = self
                    .tool_detail_motion
                    .get(call_id)
                    .map(|motion| {
                        let mut motion = *motion;
                        motion.sample(Instant::now())
                    })
                    .unwrap_or(if expanded { 1.0 } else { 0.0 });
                let detail_visible = expanded || self.tool_detail_motion.contains_key(call_id);
                let summary = if display.screenshot_mode {
                    if *done {
                        "finished".into()
                    } else {
                        "running".into()
                    }
                } else {
                    tool_row_summary(name, input)
                };
                let toggle_key = call_id.clone();
                let bg_task_for_click = if name == "bg" && *done {
                    serde_json::from_str::<serde_json::Value>(input)
                        .ok()
                        .and_then(|v| v.get("task_id").and_then(|v| v.as_str()).map(str::to_owned))
                } else {
                    None
                };
                let keyboard_key = call_id.clone();
                let bg_task_for_keyboard = bg_task_for_click.clone();
                let row_focus = self
                    .swarm_row_focus
                    .borrow_mut()
                    .entry(call_id.clone())
                    .or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone();
                let click_focus = row_focus.clone();
                let has_detail = true;
                let (token_label, token_color) = tool_output_token_badge(output);
                let call_id = call_id.clone();
                let running_veil = (!*done && !failed).then(|| {
                    gpui::AnyView::from(self.running_tool_tickers.borrow_mut().get(
                        &format!("pulse:{call_id}"),
                        crate::tool_icon::running_veil(
                            Theme::global().panel_background(self.surface_focused),
                        ),
                        cx,
                    ))
                });
                div()
                    .id(("tool", index))
                    .debug_selector(|| "tool-inline".into())
                    .flex()
                    .ml(px(offset))
                    .opacity(opacity)
                    // Transcript rows live in a fixed-height flex column. Once
                    // it overflows, flex items shrink by default, which can
                    // squash tool rows instead of letting the column scroll.
                    .flex_none()
                    .flex_col()
                    .overflow_hidden()
                    .when_some(codemode.filter(|_| nested.is_none() && !display.screenshot_mode), |el, summary| {
                        el.children(Self::render_external_codemode_messages(summary))
                    })
                    .child(crate::tool_icon::animate_running(
                        div()
                            .id(("tool-header-click", index))
                            .debug_selector(|| "tool-header".into())
                            .cursor_pointer()
                            .track_focus(&row_focus)
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, _, cx| {
                                    if event.keystroke.key == "enter" && !event.is_held {
                                        if !this.expanded_regions.contains(&keyboard_key) && let Some(task_id) = &bg_task_for_keyboard {
                                            this.request_background_output(task_id);
                                        }
                                        this.toggle_region(&keyboard_key, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                window.focus(&click_focus, cx);
                                if !this.expanded_regions.contains(&toggle_key) && let Some(task_id) = &bg_task_for_click {
                                    this.request_background_output(task_id);
                                }
                                this.toggle_region(&toggle_key, cx);
                                cx.stop_propagation();
                            }))
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .px_1()
                            .py(px(2.0))
                            .text_size(px(14.0))
                            // Match the token pill's 14px line plus 2px padding per side.
                            .line_height(px(18.0))
                            .child(crate::tool_icon::render_badge(name, *done, failed))
                            .when(!summary.is_empty(), |el| {
                                el.child(
                                    div()
                                        .debug_selector(|| "tool-summary".into())
                                        .id(("tool-summary-text", index))
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(Theme::global().TOOL_TEXT)
                                        .child(summary),
                                )
                            })
                            .when_some(tool_output::background_output(name, output), |el, result| {
                                el.when_some(tool_bg_status(name, output), |el, status| el.child(div().flex_none().text_size(px(11.0)).text_color(Theme::global().TEXT_FAINT).child(status)))
                                  .when_some(output.lines().find_map(|line| line.strip_prefix("Duration: ")), |el, duration| el.child(div().flex_none().text_size(px(11.0)).text_color(Theme::global().TEXT_FAINT).child(duration.to_owned())))
                                  .when_some(result.exit_code.filter(|code| *code != 0), |el, code| el.child(div().debug_selector(|| "tool-bg-exit-code".into()).flex_none().px_2().rounded_full().bg(Theme::global().INLINE_CODE_BG).text_color(Theme::global().ERROR).child(format!("exit {code}"))))
                            })
                            .children(self.render_tool_clock(
                                index,
                                &call_id,
                                *done || failed,
                                cx,
                            ))
                            // The token pill shares the row's click target.
                            .when(has_detail && !display.screenshot_mode, |el| {
                                el.child(
                                    div()
                                        .id(("tool-output-toggle", index))
                                        .debug_selector(|| "tool-output-size".into())
                                        .flex_none()
                                        .ml_auto()
                                        .px_2()
                                        .py(px(2.0))
                                        .rounded_full()
                                        .bg(Theme::global().INLINE_CODE_BG)
                                        .hover(|style| style.bg(Theme::global().TOOL_BORDER))
                                        .cursor_pointer()
                                        .text_size(px(10.0))
                                        .line_height(px(14.0))
                                        .text_color(token_color)
                                        .child(token_label),
                                )
                            }),
                        *done,
                        failed,
                        running_veil,
                    ))
                    .when(detail_visible && !display.screenshot_mode && has_detail, |el| {
                        let scroll = self
                            .swarm_scrolls
                            .borrow_mut()
                            .entry(call_id.clone())
                            .or_default()
                            .clone();
                        let applet_instance = crate::applet_runtime::get(cx)
                            .host
                            .borrow()
                            .tool_card(&self.session_id, &call_id)
                            .map(|mounted| mounted.instance.id.clone());
                        let mut supplement = if name == "ask" && *done {
                            Some(
                                div()
                                    .debug_selector(|| "ask-result".into())
                                    .child(self.render_ask_answer(
                                        call_id.as_str(),
                                        ask::result_text(input, output, error.as_deref()),
                                        cx,
                                    ))
                                    .into_any_element(),
                            )
                        } else if let Some(response) = error
                            .is_none()
                            .then(|| user_response::parse(name, input))
                            .flatten()
                        {
                            Some(user_response::render(
                                &response,
                                index,
                                None,
                                &self.transcript_selection,
                                self.media_preview_handler(cx),
                                window,
                                cx,
                            ))
                        } else if let Some(instance) = applet_instance {
                            crate::applet_surface::card_with_tasks(
                                &instance,
                                true,
                                &self.applet_selection,
                                Some((&self.session_id, &|app| self.applet_session_tasks(app))),
                                window,
                                cx,
                            )
                        } else if name == "todo" && *done && error.is_none() {
                            parse_todo_tool_output(output).map(|payload| {
                                render_todo_card_with_style(
                                    &payload,
                                    false,
                                    &format!("todo-{index}"),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                )
                            })
                        } else {
                            None
                        };
                        if supplement.is_none()
                            && display.diff_display != crate::config::DiffDisplay::Off
                        {
                            supplement = self.render_edit_metadata(
                                &call_id,
                                name,
                                input,
                                output,
                                *done,
                                error.as_deref(),
                                cx,
                            );
                        }
                        if supplement.is_none() {
                            let toggle_id = call_id.clone();
                            if let Some(compose) = gmail_draft_card::parse(name, input) {
                                let compose =
                                    gmail_draft_card::merge_output(compose, output, *done);
                                supplement = Some(gmail_draft_card::render(
                                    index,
                                    &compose,
                                    &gmail_draft_card::outcome(output, *done),
                                    error.as_deref(),
                                    tool_card_token_pill(output),
                                    self.superseded_drafts().get(&call_id).copied(),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            } else if let Some(view) =
                                gmail_read_card::parse(name, input, output, *done, error.as_deref())
                            {
                                supplement = Some(gmail_read_card::render(
                                    index,
                                    &view,
                                    tool_card_token_pill(output),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            } else if let Some(view) =
                                mcp_list_card::parse(name, input, output, *done, error.as_deref())
                            {
                                supplement = Some(mcp_list_card::render(
                                    index,
                                    &view,
                                    tool_card_token_pill(output),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            }
                        }
                        let height = if self.expandable_viewport_height > px(0.) {
                            (f32::from(self.expandable_viewport_height) * 0.6)
                                .clamp(1., expandable::MAX_HEIGHT)
                        } else {
                            expandable::MAX_HEIGHT
                        };
                        el.child(
                            div()
                                .debug_selector(|| "tool-detail".into())
                                .opacity(detail_progress)
                                .relative()
                                .ml(px(24.0))
                                .mt_1()
                                .mb_1()
                                .p_3()
                                .rounded_lg()
                                .bg(Theme::global().CODE_BG)
                                .font_family(Theme::global().FONT_MONO)
                                .text_size(px(11.5))
                                .line_height(relative(1.45))
                                .text_color(Theme::global().CODE_TEXT)
                                .child(
                                    div()
                                        .id(format!("tool-detail-{index}-scroll"))
                                        .debug_selector(move || {
                                            format!("tool-detail-{index}-scroll")
                                        })
                                        .w_full()
                                        .min_w_0()
                                        .max_h(px(height))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(div().debug_selector(|| "tool-timing-detail".into()).mb_2().text_color(Theme::global().TEXT_FAINT).child(self.tool_timing_detail(&call_id)))
                                                .when_some(tool_output::bash_exit_code(name, output).filter(|code| *code != 0), |el, code| {
                                                    el.child(div().debug_selector(|| "tool-detail-exit-code".into()).self_start().mb_2().px_2().py_1().rounded_full().bg(Theme::global().INLINE_CODE_BG).text_color(if code == 0 { Theme::global().OK } else { Theme::global().ERROR }).child(format!("exit {code}")))
                                                })
                                                .when(!*done && error.is_none() && self.has_running_tool(), |el| {
                                                el.child(
                                                    div()
                                                        .id(("tool-background", index))
                                                        .debug_selector(|| "tool-background".into())
                                                        .flex_none()
                                                        .px_2()
                                                        .py(px(2.0))
                                                        .rounded_full()
                                                        .bg(Theme::global().INLINE_CODE_BG)
                                                        .hover(|style| style.bg(Theme::global().TOOL_BORDER))
                                                        .cursor_pointer()
                                                        .text_size(px(10.0))
                                                        .line_height(px(14.0))
                                                        .text_color(Theme::global().TEXT_FAINT)
                                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                                            cx.stop_propagation();
                                                            this.background_running_tool(cx);
                                                        }))
                                                        .child("Background  Alt+B"),
                                                )
                                            })
                                            .children(supplement)
                                                .child(if is_codemode && *done {
                                                    self.render_codemode_detail(
                                                        index, &call_id, input, output, error.as_deref(),
                                                        codemode, None, window, cx,
                                                    )
                                                } else if let Some((args, result)) = tool_output::bash_sections(name, input, output, error.as_deref()) {
                                                    div().flex().flex_col()
                                                        .child(div().debug_selector(|| "tool-bash-command".into()).child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .when(!result.is_empty(), |el| el.child(div().debug_selector(|| "tool-bash-output-code".into()).p_2().rounded_lg().bg(Theme::global().INLINE_CODE_BG).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-bash-output-{index}"), result, window, cx))))
                                                        .into_any_element()
                                                } else if let Some(result) = tool_output::background_output(name, output) {
                                                    div().flex().flex_col().gap_2()
                                                        .when_some(result.preview, |el, preview| el.child(div().debug_selector(|| "tool-bg-output-code".into()).p_2().rounded_lg().bg(Theme::global().INLINE_CODE_BG).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-bg-preview-{index}"), preview, window, cx))))
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else if let Some(lines) = tool_output::json_lines(name, output, &call_id, &self.expanded_regions) {
                                                    let args = tool_output::ToolDetail::arguments(input);
                                                    let mut toggle_index = 0usize;
                                                    let rows = lines.into_iter().enumerate().map(|(line_index, line)| {
                                                        let toggle = line.toggle.map(|key| {
                                                            let selector = format!("tool-json-toggle-{toggle_index}");
                                                            toggle_index += 1;
                                                            div().id(SharedString::from(format!("json-toggle-{index}-{line_index}"))).debug_selector(move || selector.clone().into())
                                                                .w(px(18.0)).flex_none().cursor_pointer().text_color(Theme::global().TEXT_FAINT)
                                                                .child(if line.collapsed { "▸" } else { "▾" })
                                                                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.toggle_line(&key, cx); })).into_any_element()
                                                        }).unwrap_or_else(|| div().w(px(18.0)).flex_none().into_any_element());
                                                        div().flex().child(toggle).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-json-{index}-{line_index}"), line.text, window, cx)).into_any_element()
                                                    }).collect::<Vec<_>>();
                                                    div().debug_selector(|| "tool-detail-json".into()).font_family(Theme::global().FONT_MONO).flex().flex_col()
                                                        .when(!args.is_empty(), |el| el.child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .children(rows)
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else if let Some(markdown) = tool_output::markdown_output(name, output) {
                                                    let args = tool_output::ToolDetail::arguments(input);
                                                    div().debug_selector(|| "tool-detail-markdown".into())
                                                        .flex().flex_col().gap_2()
                                                        .when(!args.is_empty(), |el| el.child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .child(div().font_family(Theme::global().FONT_AI).child(markdown::render(&markdown, tool_output::markdown_row(index), &self.transcript_selection, window, cx)))
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else {
                                                    div().debug_selector(|| "tool-argument-table".into()).child(div().debug_selector({ let selector = tool_output::selector(name, input, output); move || selector.into() }).child(tool_output::selectable_detail(
                                                        name, input, tool_output::ToolDetail::new(name, input, output, error.as_deref()),
                                                        format!("tool-detail-{index}"),
                                                        self.transcript_selection.clone(), window, cx,
                                                    ))).into_any_element()
                                                })
                                                .child(
                                                    div()
                                                        .debug_selector(|| "tool-detail-end".into())
                                                        .child(""),
                                                ),
                                        )
                                        .map(|body| crate::transcript_scroll::tracked_vertical(body, &scroll)),
                                )
                                .children((crate::fixture_control_targets::transcript_scroll_fixture() && call_id == "scroll-fixture").then(|| {
                                    crate::fixture_control_targets::marker(|| "scroll:tool".into(), || Some(self.session_id.clone())).unwrap()
                                }))
                                .child(crate::scrollbar::vertical_with_track(
                                    &scroll,
                                    "tool-detail-scrollbar",
                                )),
                        )
                    })
                    .into_any_element()
            }
            Item::Error(message) => self.render_recovery_error(index, message, false, window, cx),
            Item::RuntimeError { message, .. } => {
                self.render_recovery_error(index, message, true, window, cx)
            }
        }
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
    pub(super) fn request_background_output(&self, task_id: &str) {
        if !self.is_side_document() && self.preview_state.is_none() {
            self.bridge.send(Command::RuntimeCommand {
                session_id: self.session_id.clone(),
                command: jcode_sdk::api::RuntimeCommand::BackgroundOutput,
                args: vec![task_id.to_owned()],
            });
        }
    }

    pub(crate) fn toggle_line(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded_regions.remove(key) {
            self.expanded_regions.insert(key.to_owned());
        }
        self.transcript_measurements.invalidate();
        cx.notify();
    }
}
