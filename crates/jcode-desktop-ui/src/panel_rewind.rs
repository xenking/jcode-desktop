use super::*;
use jcode_sdk::api::{RewindAnchor, RewindMessageKey};

pub(super) struct Selection {
    selected: usize,
    reveal: bool,
    original_scroll: gpui::ListOffset,
    followed_tail: bool,
}

fn is_target(item: &Item) -> bool {
    prompt::is_pinnable_prompt(item)
        || matches!(item, Item::Assistant(text) if !text.trim().is_empty())
}

impl Panel {
    pub(super) fn start_rewind(&mut self, cx: &mut Context<Self>) {
        if self.is_side_document()
            || self.preview_state.is_some()
            || self.rewind_selection.is_some()
        {
            return;
        }
        if self.activity_active() {
            self.items.push(Item::Error(
                "Wait for the current turn before rewinding.".into(),
            ));
        } else if let Some(selected) = self.items.iter().rposition(is_target) {
            self.rewind_selection = Some(Selection {
                selected,
                reveal: true,
                original_scroll: self.transcript_list.logical_scroll_top(),
                followed_tail: self.stick_to_bottom,
            });
            self.stick_to_bottom = false;
            self.tail_glide.reset();
            self.cancel_transcript_momentum();
        } else {
            self.items
                .push(Item::Error("There are no messages to rewind to.".into()));
        }
        cx.notify();
    }

    fn choose_rewind_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(selection) = &mut self.rewind_selection {
            selection.selected = index;
            selection.reveal = true;
            self.cancel_transcript_momentum();
            cx.notify();
        }
    }

    pub(super) fn shift_rewind_selection(&mut self, items: usize, rows: usize) {
        if let Some(selection) = &mut self.rewind_selection {
            selection.selected += items;
            selection.original_scroll.item_ix += rows;
        }
    }

    pub(super) fn reveal_rewind_selection(
        &mut self,
        rows: &[TranscriptRenderRow],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selection) = &mut self.rewind_selection
            && std::mem::take(&mut selection.reveal)
            && let Some(item_ix) = rows.iter().position(|row| row.index == selection.selected)
        {
            window.focus(&self.focus_handle, cx);
            let viewport = self.transcript_list.viewport_bounds();
            if !self
                .transcript_list
                .bounds_for_item(item_ix)
                .is_some_and(|bounds| {
                    bounds.origin.y >= viewport.origin.y && bounds.origin.y < viewport.bottom()
                })
            {
                self.transcript_list.scroll_to(gpui::ListOffset {
                    item_ix,
                    offset_in_item: px(0.),
                });
            }
        }
    }

    pub(super) fn handle_rewind_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = &self.rewind_selection else {
            return false;
        };
        let selected = selection.selected;
        match event.keystroke.key.as_str() {
            "escape" | "enter" => {
                let selection = self.rewind_selection.take().expect("active selection");
                if event.keystroke.key == "enter" {
                    self.confirm_rewind(selected);
                } else {
                    self.stick_to_bottom = selection.followed_tail;
                    self.transcript_list.scroll_to(selection.original_scroll);
                }
                self.focus_input(window, cx);
                cx.notify();
            }
            key @ ("up" | "down" | "left" | "right") => {
                let target = |item: &Item| {
                    is_target(item)
                        && (!matches!(key, "left" | "right") || matches!(item, Item::User(_)))
                };
                let selected = selected.min(self.items.len());
                let next = if matches!(key, "up" | "left") {
                    self.items[..selected].iter().rposition(target)
                } else {
                    self.items
                        .iter()
                        .enumerate()
                        .skip(selected + 1)
                        .find_map(|(index, item)| target(item).then_some(index))
                };
                if let Some(index) = next {
                    self.choose_rewind_row(index, cx);
                } else if matches!(key, "up" | "left") {
                    self.request_older_history();
                }
            }
            _ => {}
        }
        true
    }

    fn confirm_rewind(&mut self, selected: usize) {
        fn key(item: &Item) -> Option<RewindMessageKey> {
            match item {
                Item::User(text) => RewindMessageKey::new("user", text),
                Item::Assistant(text) => RewindMessageKey::new("assistant", text),
                _ => None,
            }
        }
        let Some(target) = self.items.get(selected).and_then(key) else {
            return;
        };
        let target = RewindAnchor {
            selected: target,
            before: self.items[..selected].iter().rev().find_map(key),
            after: self.items[selected + 1..].iter().find_map(key),
        };
        self.run_session_operation(SessionOperation::RewindTranscript(target), "");
    }

    pub(crate) fn replace_rewound_history(
        &mut self,
        page: crate::harness::BoundedHistoryPage,
        bounded: bool,
        cx: &mut Context<Self>,
    ) {
        self.rewind_selection = None;
        self.cancel_transcript_momentum();
        self.history_loaded = false;
        self.history_paging = None;
        self.restored_transcript = false;
        self.pending_history_scroll = None;
        // Prompts sent while the rewind reply was in flight still belong to the
        // new suffix. The harness publishes the rewind snapshot before their acks.
        let retained_from = self
            .pending_users
            .front()
            .copied()
            .unwrap_or(self.items.len());
        self.items.drain(..retained_from);
        self.user_times = self
            .user_times
            .drain()
            .filter_map(|(index, time)| Some((index.checked_sub(retained_from)?, time)))
            .collect();
        for index in &mut self.pending_users {
            *index -= retained_from;
        }
        self.expanded_prompts.clear();
        self.accepted_users.clear();
        self.response_stats = response_stats::Tracker::default();
        self.stick_to_bottom = true;
        if bounded {
            self.load_bounded_history(page, cx);
        } else {
            self.load_history(page.messages, page.images, cx);
        }
    }

    pub(super) fn render_rewind_row(
        &self,
        index: usize,
        item: &Item,
        row: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(selection) = &self.rewind_selection else {
            return row.into_any_element();
        };
        if !is_target(item) {
            return row.into_any_element();
        }
        row.id(("rewind-row", index))
            .relative()
            .cursor_pointer()
            .capture_any_mouse_down(cx.listener(
                move |panel, event: &gpui::MouseDownEvent, window, cx| {
                    if event.button == gpui::MouseButton::Left {
                        panel.choose_rewind_row(index, cx);
                        window.focus(&panel.focus_handle, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .when(selection.selected == index, |el| {
                el.child(
                    div()
                        .debug_selector(move || format!("rewind-selected-{index}"))
                        .absolute()
                        .inset_0()
                        .rounded_lg()
                        .border_1()
                        .border_color(Theme::global().ACCENT),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_rewind_hint(&self) -> Option<gpui::AnyElement> {
        self.rewind_selection.as_ref()?;
        Some(
            div()
                .debug_selector(|| "rewind-keys".into())
                .flex_none()
                .min_w_0()
                .px_3()
                .py_1()
                .text_size(px(12.))
                .text_color(Theme::global().TEXT)
                .bg(Theme::global().ACCENT_DIM)
                .truncate()
                .child("Rewind   ↑↓ Messages   ←→ Your prompts   Enter Confirm   Esc Cancel")
                .into_any_element(),
        )
    }
}
