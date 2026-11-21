use super::*;
use gpui::Pixels;
use jcode_sdk::api::{RewindAnchor, RewindMessageKey};

pub(super) struct Selection {
    selected: usize,
    reveal: bool,
    inset: Pixels,
    older_target: Option<Target>,
    original_scroll: gpui::ListOffset,
    followed_tail: bool,
}

#[derive(Clone, Copy)]
enum Target {
    Message,
    Prompt,
}

impl Target {
    fn matches(self, item: &Item) -> bool {
        prompt::is_pinnable_prompt(item)
            || matches!((self, item), (Self::Message, Item::Assistant(text)) if !text.trim().is_empty())
    }
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
        } else if let Some(selected) = self
            .items
            .iter()
            .rposition(|item| Target::Message.matches(item))
        {
            self.rewind_selection = Some(Selection {
                selected,
                reveal: true,
                inset: px(0.),
                older_target: None,
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
            selection.older_target = None;
            self.cancel_transcript_momentum();
            cx.notify();
        }
    }

    pub(super) fn shift_rewind_selection(&mut self, items: usize, rows: usize) {
        if let Some(selection) = &mut self.rewind_selection {
            selection.selected += items;
            selection.original_scroll.item_ix += rows;
            if let Some(target) = selection.older_target {
                if let Some(next) = self.items[..items]
                    .iter()
                    .rposition(|item| target.matches(item))
                {
                    selection.selected = next;
                    selection.reveal = true;
                    selection.older_target = None;
                } else {
                    self.request_older_history();
                }
            }
        }
    }

    fn remeasure_rewind_edges(&self) {
        let count = self.transcript_list.item_count();
        if count > 0 {
            self.transcript_list.remeasure_items(0..1);
        }
        if count > 1 {
            self.transcript_list.remeasure_items(count - 1..count);
        }
    }

    pub(super) fn reveal_rewind_selection(
        &mut self,
        rows: &[TranscriptRenderRow],
        dock_pad: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pixels {
        let Some(selection) = &mut self.rewind_selection else {
            return px(0.);
        };
        // Room to center the first and last messages without overscrolling the list.
        let inset = window.viewport_size().height / 2.;
        let changed = selection.inset != inset;
        if changed {
            if selection.inset == px(0.) {
                window.focus(&self.focus_handle, cx);
            }
            selection.inset = inset;
            selection.reveal = true;
        }
        if !selection.reveal {
            return inset;
        }
        let Some(item_ix) = rows.iter().position(|row| row.index == selection.selected) else {
            return inset;
        };
        if changed {
            self.remeasure_rewind_edges();
        }
        let viewport = self.transcript_list.viewport_bounds();
        if !changed && let Some(bounds) = self.transcript_list.bounds_for_item(item_ix) {
            let leading = if item_ix == 0 { inset } else { px(0.) };
            let trailing = if item_ix + 1 == self.transcript_list.item_count() {
                inset + px(dock_pad)
            } else {
                px(0.)
            };
            let visible_height = (viewport.size.height - px(dock_pad)).max(px(1.));
            let height = bounds.size.height - leading - trailing;
            let desired_top = viewport.top() + ((visible_height - height) / 2.).max(px(0.));
            self.transcript_list
                .scroll_by(bounds.top() + leading - desired_top);
            if let Some(selection) = &mut self.rewind_selection {
                selection.reveal = false;
            }
        } else {
            // Offscreen rows may not have been measured by the virtual list yet.
            self.transcript_list.scroll_to(gpui::ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            window.request_animation_frame();
        }
        inset
    }

    pub(super) fn finish_rewind(
        &mut self,
        confirm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = self.rewind_selection.take() else {
            return false;
        };
        self.remeasure_rewind_edges();
        if confirm {
            self.confirm_rewind(selection.selected);
        } else {
            self.stick_to_bottom = selection.followed_tail;
            self.transcript_list.scroll_to(selection.original_scroll);
        }
        self.focus_input(window, cx);
        cx.notify();
        true
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
                self.finish_rewind(event.keystroke.key == "enter", window, cx);
            }
            key @ ("up" | "down" | "left" | "right") => {
                let target = if matches!(key, "left" | "right") {
                    Target::Prompt
                } else {
                    Target::Message
                };
                let older = matches!(key, "up" | "left");
                let selected = selected.min(self.items.len());
                let next = if older {
                    self.items[..selected]
                        .iter()
                        .rposition(|item| target.matches(item))
                } else {
                    self.items
                        .iter()
                        .enumerate()
                        .skip(selected + 1)
                        .find_map(|(index, item)| target.matches(item).then_some(index))
                };
                if let Some(index) = next {
                    self.choose_rewind_row(index, cx);
                } else if let Some(selection) = &mut self.rewind_selection {
                    selection.older_target = older.then_some(target);
                    if older {
                        self.request_older_history();
                    }
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
        if !Target::Message.matches(item) {
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
