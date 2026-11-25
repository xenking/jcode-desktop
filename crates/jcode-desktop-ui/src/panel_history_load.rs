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
}
