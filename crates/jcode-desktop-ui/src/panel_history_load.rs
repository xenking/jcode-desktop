//! Prefetching a conversation's history and adopting the authoritative reply
//! over the provisional prefix.
use super::*;

impl Panel {
    /// Paint the locally persisted transcript while the runtime attaches.
    /// Attaching restores the agent before `get_history` can answer, which
    /// takes hundreds of milliseconds. The stored record renders in a few, so
    /// the reader sees the conversation at once and the authoritative reply
    /// swaps in underneath (usually as a no-op when nothing changed).
    pub fn prefetch_persisted_history(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || crate::harness::screenshot_mode() {
            return;
        }
        let id = self.session_id.clone();
        if !crate::persisted_history::is_local_session_id(&id)
            || !jcode_base::session::session_path(&id).is_ok_and(|path| path.exists())
        {
            return;
        }
        self.prefetch_history_with(move || crate::persisted_history::load(&id), cx);
    }

    pub(crate) fn prefetch_history_with(
        &mut self,
        loader: impl FnOnce() -> Result<crate::persisted_history::History, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if !self.can_show_provisional() {
            return;
        }
        self.awaiting_persisted = true;
        let started = Instant::now();
        // Parse, render, and decode images off the UI thread.
        let request = cx.background_executor().spawn(async move {
            loader().map(|(messages, images)| {
                let times = history_tool_times(&messages);
                let codemode: Vec<_> = codemode_card::history_summaries(&messages).collect();
                (crate::panel::history_items::build(messages, images), times, codemode)
            })
        });
        self.provisional_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |panel, cx| {
                panel.provisional_task = None;
                if std::mem::take(&mut panel.awaiting_persisted) {
                    cx.notify();
                }
                let Ok(((items, user_times), tool_times, codemode)) = result else {
                    return;
                };
                if items.is_empty() || !panel.can_show_provisional() {
                    return;
                }
                eprintln!(
                    "jcode desktop: provisional transcript items={} in {:?}",
                    items.len(),
                    started.elapsed()
                );
                panel.provisional_items = items.len();
                panel.user_times = user_times;
                panel.codemode_summaries.extend(codemode);
                for (id, (start, finish, duration)) in tool_times {
                    panel.tool_wall_times.insert(id.clone(), (start, finish));
                    if let Some(duration) = duration {
                        panel.tool_durations.insert(id, duration);
                    }
                }
                *panel.items = items;
                panel.transcript_measurements.invalidate();
                if panel.startup_layout.as_ref().is_some_and(|l| !l.committed) {
                    panel.startup_layout = None;
                }
                if panel.pending_history_scroll.is_none() && panel.stick_to_bottom {
                    panel.transcript_list.scroll_to_end();
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn restore_tool_times(&mut self, messages: &[jcode_sdk::HistoryMessage]) {
        for (id, (start, finish, duration)) in history_tool_times(messages) {
            self.tool_wall_times.insert(id.clone(), (start, finish));
            if let Some(duration) = duration {
                self.tool_durations.insert(id, duration);
            }
        }
    }

    /// Only an untouched, not-yet-hydrated transcript may borrow the disk copy.
    pub(crate) fn can_show_provisional(&self) -> bool {
        !self.history_loaded
            && !self.restored_transcript
            && !self.transcript_only
            && !self.is_side_document()
            && self.provisional_items == 0
            && self.items.is_empty()
            && self.pending_users.is_empty()
            && self.streaming_text.is_empty()
            && self.streaming_reasoning.is_empty()
    }

    /// Retire the provisional prefix before the authoritative history lands.
    /// Returns true when the reply matched it exactly and nothing else is
    /// needed, which keeps measurements and scroll position untouched.
    pub(crate) fn adopt_authoritative_over_provisional(
        &mut self,
        items: &[Item],
        user_times: &HashMap<usize, i64>,
    ) -> bool {
        self.provisional_task = None;
        self.awaiting_persisted = false;
        let count = std::mem::take(&mut self.provisional_items).min(self.items.len());
        if count == 0 {
            return false;
        }
        if self.items[..count] == *items {
            self.user_times
                .extend(user_times.iter().map(|(index, at)| (*index, *at)));
            return true;
        }
        self.items.drain(..count);
        self.shift_user_times(0, count);
        self.pending_users = self
            .pending_users
            .drain(..)
            .filter_map(|index| index.checked_sub(count))
            .collect();
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .filter_map(|(index, at)| Some((index.checked_sub(count)?, at)))
            .collect();
        self.clear_pinned_prompt();
        self.swarm_scrolls.borrow_mut().clear();
        false
    }

    pub(super) fn load_history_body(
        &mut self,
        messages: Vec<jcode_sdk::HistoryMessage>,
        images: Vec<jcode_sdk::RenderedImage>,
        cx: &mut Context<Self>,
    ) {
        if self.is_side_document() {
            return;
        }
        self.note_session_image_ordinal(images.iter().filter_map(|image| image.session_image.as_ref()).map(|image| image.ordinal).max(), cx);
        self.restore_tool_times(&messages);
        self.codemode_summaries
            .extend(codemode_card::history_summaries(&messages));
        if self.history_paging.is_none() {
            self.history_paging = history_paging::HistoryPaging::from_history(&messages);
            if self.history_loaded
                && let Some(paging) = &mut self.history_paging
            {
                paging.set_anchor(&self.items);
            }
        }
        self.transcript_measurements.invalidate();
        if self.restored_transcript {
            self.hydrate_restored_prefix(&messages);
            // An empty/behind reply still completes the connection handshake.
            // Keep the cache reconciliation marker independent of readiness.
            self.history_loaded = true;
            self.input
                .update(cx, |input, cx| input.set_pending_session(false, cx));
        }
        if self.history_loaded {
            self.schedule_ask_recovery(cx);
            // History is persisted message state, not a replay of the live turn.
            // Wait for authoritative idle before reconciling it with a saved
            // suffix. Buffered deltas must not be appended to a history prefix.
            self.defer_reconnect_history(&messages);
            self.reconcile_history_images(&messages, images);
            self.send_queued_prompts(cx);
            cx.notify();
            return;
        }
        self.history_loaded = true;
        if !self.is_pending_session() {
            self.input
                .update(cx, |input, cx| input.set_pending_session(false, cx));
        }
        // An established session can paint an empty placeholder before its
        // history arrives. That must not adopt a new conversation's layout.
        if !messages.is_empty()
            && self.pending_users.is_empty()
            && self
                .startup_layout
                .as_ref()
                .is_some_and(|layout| !layout.committed)
        {
            self.startup_layout = None;
        }
        let (mut items, history_times) = crate::panel::history_items::build(messages, images);
        if let Some(paging) = &mut self.history_paging {
            paging.set_anchor(&items);
        }
        if self.adopt_authoritative_over_provisional(&items, &history_times) {
            self.send_queued_prompts(cx);
            cx.notify();
            return;
        }
        // History goes first; anything echoed locally before it arrived is
        // appended, minus the duplicate the server already knows about.
        let mut existing = std::mem::take(&mut self.items);
        existing.retain(|item| match item {
            Item::User(text) => !matches!(items.last(), Some(Item::User(last)) if last == text),
            _ => true,
        });
        let history_len = items.len();
        self.shift_user_times(history_len, 0);
        self.user_times.extend(history_times);
        self.pending_users = self
            .pending_users
            .drain(..)
            .map(|index| index + history_len)
            .collect();
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(index, at)| (index + history_len, at))
            .collect();
        items.append(&mut existing);
        self.image_pane_selected = None;
        self.clear_pinned_prompt();
        *self.items = items;
        self.schedule_ask_recovery(cx);
        if self.pending_history_scroll.is_none() && self.stick_to_bottom {
            self.transcript_list.scroll_to_end();
        }
        self.send_queued_prompts(cx);
        cx.notify();
    }

    pub(super) fn recover(&mut self, response: &str) {
        self.transcript_measurements.invalidate();
        if self.streaming_text == response {
            return;
        }
        if !self.streaming_text.is_empty() && response.starts_with(&self.streaming_text) {
            self.streaming_text = response.to_string();
            return;
        }

        let latest = self
            .items
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, item)| !matches!(item, Item::User(_)))
            .find(|(_, item)| item.assistant_text().is_some_and(|text| !text.trim().is_empty()));
        if let Some((index, item)) = latest {
            if item.assistant_text() == Some(response) {
                return;
            }
            if let Item::Assistant(existing) | Item::AssistantOutput {
                text: existing, stream_id: None, ..
            } = &mut self.items[index]
                && response.starts_with(existing.as_str())
            {
                *existing = response.to_string();
                return;
            }
        }

        self.flush_reasoning();
        self.flush_streaming();
        self.items.push(Item::Assistant(response.to_string()));
    }
}
