use super::*;
use jcode_sdk::api::QueuedPromptMode;

const INDENT: &[u8] = include_bytes!("../../../assets/icons/queue-indent.svg");
const SEND: &[u8] = include_bytes!("../../../assets/icons/arrow-up.svg");
const PENCIL: &[u8] = include_bytes!("../../../assets/icons/pencil.svg");
const TRASH: &[u8] = include_bytes!("../../../assets/icons/trash.svg");


/// The prompt row whose action menu is open.
enum CardRow {
    Remote { id: String, index: usize },
    Local { index: usize },
}

impl Panel {
    /// The open row, or `None` when no menu is open or its prompt is gone, in
    /// which case the stale menu is closed.
    fn queue_card_row(&mut self) -> Option<CardRow> {
        if let Some(id) = self
            .prompt_queue
            .card_menu
            .as_deref()
            .and_then(|key| key.strip_prefix("remote-"))
            .map(str::to_owned)
        {
            let Some(index) = self
                .prompt_queue
                .remote
                .iter()
                .position(|prompt| prompt.client_id == id)
            else {
                self.prompt_queue.card_menu = None;
                return None;
            };
            return Some(CardRow::Remote { id, index });
        }
        let index = self
            .prompt_queue
            .card_menu
            .as_deref()
            .and_then(|key| key.strip_prefix("local-"))
            .and_then(|index| index.parse::<usize>().ok())?;
        if index >= self.prompt_queue.prompts.len() {
            self.prompt_queue.card_menu = None;
            return None;
        }
        Some(CardRow::Local { index })
    }

    /// Alt+Up and Alt+Down reorder, M toggles the delivery mode. Escape and
    /// Enter arrive as `menu` actions.
    pub(in crate::panel) fn handle_queue_card_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(row) = self.queue_card_row() else {
            return false;
        };
        let key = event.keystroke.key.as_str();
        let alt = event.keystroke.modifiers.alt;
        match row {
            CardRow::Remote { id, index } => match (key, alt) {
                ("up", true) => self.reorder_remote_card_prompt(index, true),
                ("down", true) => self.reorder_remote_card_prompt(index, false),
                ("m", false) => {
                    let mode = match self.prompt_queue.remote[index].mode {
                        jcode_sdk::api::QueuedPromptMode::Steer => {
                            jcode_sdk::api::QueuedPromptMode::AfterYield
                        }
                        jcode_sdk::api::QueuedPromptMode::AfterYield => {
                            jcode_sdk::api::QueuedPromptMode::Steer
                        }
                    };
                    self.bridge.send(Command::SetQueuedPromptMode {
                        session_id: self.session_id.clone(),
                        client_id: id,
                        mode,
                    });
                }
                _ => return false,
            },
            CardRow::Local { index } => match (key, alt) {
                ("up", true) if index > 0 => {
                    self.prompt_queue.prompts.swap(index, index - 1);
                    self.local_queue_swapped(index, index - 1);
                    self.prompt_queue.card_menu = Some(format!("local-{}", index - 1));
                }
                ("down", true) if index + 1 < self.prompt_queue.prompts.len() => {
                    self.prompt_queue.prompts.swap(index, index + 1);
                    self.local_queue_swapped(index, index + 1);
                    self.prompt_queue.card_menu = Some(format!("local-{}", index + 1));
                }
                ("up" | "down", true) => {}
                ("m", false) => {
                    self.prompt_queue.prompts[index].after_yield =
                        !self.prompt_queue.prompts[index].after_yield
                }
                _ => return false,
            },
        }
        cx.notify();
        true
    }

    /// Enter sends the open row now.
    pub(in crate::panel) fn confirm_queue_card(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.queue_card_row() {
            Some(CardRow::Remote { id, index }) => {
                self.send_remote_queue_row(&id, self.prompt_queue.remote[index].mode, cx);
                self.prompt_queue.card_menu = None;
            }
            Some(CardRow::Local { index }) if self.queue_connection_ready() => {
                self.prompt_queue.card_menu = None;
                self.send_local_queue_row(index, cx);
            }
            _ => return false,
        }
        self.focus_input(window, cx);
        cx.notify();
        true
    }

    /// Esc only dismisses what the card opened: it closes the row actions and
    /// gives focus back to the composer. Stopping a turn is a separate action,
    /// so Esc never pauses the queue by itself.
    pub(in crate::panel) fn cancel_queue_card(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.queue_card_row() {
            Some(CardRow::Remote { .. }) => {}
            Some(CardRow::Local { .. }) => self.prompt_queue.card_expanded = None,
            None => return false,
        }
        self.prompt_queue.card_menu = None;
        self.focus_input(window, cx);
        cx.notify();
        true
    }

    pub(super) fn render_queue_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let queue = &self.prompt_queue;
        let steered = self.queue_section(QueuedPromptMode::Steer, cx);
        let after_yield = self.queue_section(QueuedPromptMode::AfterYield, cx);
        let split = steered.is_some() && after_yield.is_some();
        if steered.is_none() && after_yield.is_none() {
            return None;
        }
        let theme = Theme::global();
        Some(
            div()
                .id("queue-card")
                .debug_selector(|| "queue-card".into())
                .w_full()
                .min_w_0()
                .flex_none()
                .flex()
                .flex_col()
                .rounded_lg()
                .bg(Kit::surface(theme))
                .pb(px(2.))
                .text_size(px(12.))
                .text_color(theme.TEXT)
                .child(
                    div()
                        .id("prompt-queue-items")
                        .debug_selector(|| "prompt-queue-items".into())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .max_h(px(if queue.queued_edit.is_some() {
                            360.
                        } else if split {
                            192.
                        } else {
                            170.
                        }))
                        .overflow_y_scroll()
                        .track_scroll(&self.queue_items_scroll)
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .children(steered)
                        .children(after_yield),
                )
                .into_any_element(),
        )
    }

    fn queue_section(
        &self,
        mode: QueuedPromptMode,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let queue = &self.prompt_queue;
        let after_yield = mode == QueuedPromptMode::AfterYield;
        let (section_id, button_id, title) = if after_yield {
            (
                "queue-section-after-yield",
                "queue-send-all-after-yield",
                "After yield",
            )
        } else {
            (
                "queue-section-steer",
                "queue-send-all-steer",
                "Steered messages",
            )
        };
        let remote = queue
            .remote
            .iter()
            .enumerate()
            .filter(|(_, prompt)| prompt.mode == mode);
        let pending = queue
            .pending_remote
            .iter()
            .filter(|prompt| prompt.mode == mode);
        let local = queue
            .prompts
            .iter()
            .enumerate()
            .filter(|(_, prompt)| prompt.after_yield == after_yield);
        let orphan_edit = queue.queued_edit.as_ref().filter(|edit| {
            edit.after_yield == after_yield
                && matches!(&edit.target, QueueEditTarget::Remote(id)
                if !queue.remote.iter().any(|prompt| &prompt.client_id == id))
        });
        let legacy = !after_yield && !queue.steered.is_empty();
        let has_local = local.clone().next().is_some();
        let has_pending = pending.clone().next().is_some();
        if remote.clone().next().is_none()
            && !has_pending
            && !has_local
            && !legacy
            && orphan_edit.is_none()
        {
            return None;
        }

        let theme = Theme::global();
        let waiting = self.queue_waiting_for_connection();
        let can_send = !waiting
            && (has_pending
                || remote
                    .clone()
                    .any(|(_, prompt)| self.queued_remote_sendable(prompt))
                || local
                    .clone()
                    .any(|(i, _)| self.local_edit_index() != Some(i)));
        let paused = queue.paused && has_local;
        Some(
            div()
                .id(section_id)
                .debug_selector(move || section_id.into())
                .flex()
                .flex_col()
                .flex_none()
                .gap_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(26.))
                        .px_2()
                        .gap_2()
                        .text_size(px(11.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.TEXT_DIM)
                                .child(title),
                        )
                        .when(paused, |el| {
                            el.child(Kit::pill(
                                "queue-resume",
                                "Queue paused · Resume",
                                cx.listener(|this, _, _, cx| {
                                    this.prompt_queue.paused = false;
                                    this.send_queued_prompts(cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ))
                        })
                        .when(waiting && !paused, |el| {
                            el.child(
                                div()
                                    .debug_selector(|| "queue-waiting".into())
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.TEXT_DIM)
                                    .child("Waiting for connection…"),
                            )
                        })
                        .when(can_send, |el| {
                            el.child(
                                Kit::pill(
                                    button_id,
                                    "Send all now",
                                    cx.listener(move |this, _, _, cx| {
                                        this.interrupt_queue_section(mode, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .flex_row_reverse()
                                .gap_1()
                                .bg(theme.TOOL_BG)
                                .border_1()
                                .border_color(theme.INPUT_BORDER)
                                .text_color(theme.TEXT)
                                .child(Kit::icon(SEND, 12.).text_color(theme.TEXT)),
                            )
                        }),
                )
                .when_some(orphan_edit, |el, edit| {
                    el.child(self.queue_edit_row(
                        edit,
                        "queued-prompt-edit-orphan".into(),
                        "queue-edit-orphan".into(),
                        cx,
                    ))
                })
                .children(
                    remote.map(|(index, prompt)| self.card_remote_row(prompt, Some(index), cx)),
                )
                .children(pending.map(|prompt| self.card_remote_row(prompt, None, cx)))
                .when(legacy, |el| {
                    el.children(
                        queue
                            .steered
                            .iter()
                            .enumerate()
                            .map(|(i, prompt)| self.card_legacy_row(i, prompt, cx)),
                    )
                })
                .when(has_local, |el| {
                    el.child(
                        div()
                            .debug_selector(|| "prompt-queue".into())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(local.map(|(i, prompt)| self.card_local_row(i, prompt, cx))),
                    )
                })
                .into_any_element(),
        )
    }

    fn card_local_row(
        &self,
        index: usize,
        prompt: &QueuedPrompt,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if let Some(edit) = self
            .prompt_queue
            .queued_edit
            .as_ref()
            .filter(|edit| edit.target == QueueEditTarget::Local(index))
        {
            return self.queue_edit_row(
                edit,
                SharedString::from(format!("queued-prompt-edit-{index}")),
                format!("queued-prompt-{index}"),
                cx,
            );
        }
        let key = format!("local-{index}");
        let expanded = self.prompt_queue.card_expanded.as_deref() == Some(&key);
        let selected = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let preview = self.render_queued_content(&prompt.content, &prompt.images, cx);
        let key_text = key.clone();
        let send_label = Kit::send_label(prompt.after_yield);
        let theme = Theme::global();
        let group: SharedString = format!("queue-row-local-{index}").into();
        div()
            .id(("queued-prompt", index))
            .debug_selector(move || format!("queued-prompt-{index}"))
            .group(group.clone())
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .rounded_lg()
            .when(selected, |el| el.bg(theme.QUOTE_BG))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .h(px(30.))
                    .px_2()
                    .child(
                        div()
                            .debug_selector(move || format!("queue-row-icon-{index}"))
                            .flex_none()
                            .child(
                                gpui::svg()
                                    .data(INDENT)
                                    .size(px(14.))
                                    .text_color(theme.TEXT_DIM),
                            ),
                    )
                    .child(
                        div()
                            .id(("queue-row-text", index))
                            .debug_selector(move || format!("queue-row-text-{index}"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .cursor_pointer()
                            .child(preview)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.prompt_queue.card_expanded = if this
                                    .prompt_queue
                                    .card_expanded
                                    .as_deref()
                                    == Some(&key_text)
                                {
                                    None
                                } else {
                                    Some(key_text.clone())
                                };
                                this.prompt_queue.card_menu = Some(key_text.clone());
                                window.focus(&this.focus_handle, cx);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .when(self.queue_connection_ready(), |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("send-queued-prompt-{index}")),
                            SEND,
                            send_label,
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, _, cx| {
                                this.prompt_queue.card_menu = None;
                                this.send_local_queue_row(index, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .child(queue_row_action(
                        SharedString::from(format!("recall-queued-prompt-{index}")),
                        PENCIL,
                        "Edit",
                        group.clone(),
                        selected,
                        cx.listener(move |this, _, window, cx| {
                            this.prompt_queue.card_menu = None;
                            this.edit_local_queued_prompt(index, window, cx);
                            cx.stop_propagation();
                        }),
                    ))
                    .child(queue_row_action(
                        SharedString::from(format!("remove-queued-prompt-{index}")),
                        TRASH,
                        "Delete",
                        group.clone(),
                        selected,
                        cx.listener(move |this, _, _, cx| {
                            if index < this.prompt_queue.prompts.len() {
                                this.prompt_queue.prompts.remove(index);
                                if this.local_edit_index() == Some(index) {
                                    this.prompt_queue.queued_edit = None;
                                    this.send_queued_prompts(cx);
                                } else {
                                    this.local_queue_removed(index);
                                }
                            }
                            this.prompt_queue.card_menu = None;
                            this.prompt_queue.card_expanded = None;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )),
            )
            .when(expanded, |el| {
                el.child(self.render_expanded_queued_content(&prompt.content, &prompt.images, cx))
            })
            .into_any_element()
    }

    fn card_legacy_row(
        &self,
        index: usize,
        prompt: &QueuedPrompt,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        div()
            .id(("steered-prompt", index))
            .debug_selector(move || format!("steered-prompt-{index}"))
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .h(px(30.))
            .px_2()
            .rounded_lg()
            .hover(|el| el.bg(theme.QUOTE_BG))
            .child(
                gpui::svg()
                    .data(INDENT)
                    .size(px(14.))
                    .text_color(theme.TEXT_DIM),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.render_queued_content(&prompt.content, &prompt.images, cx)),
            )
            .when(!self.activity_active(), |row| {
                row.child(Kit::action(
                    "resend-steered",
                    index,
                    "Send again",
                    cx.listener(move |this, _, _, cx| {
                        if index < this.prompt_queue.steered.len() {
                            let prompt = this.prompt_queue.steered.remove(index);
                            this.submit_or_queue(prompt.content, prompt.images, false, cx);
                        }
                        cx.stop_propagation();
                    }),
                ))
            })
            .into_any_element()
    }

    fn card_remote_row(
        &self,
        prompt: &jcode_sdk::api::QueuedPrompt,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let pending = index.is_none();
        let theme = Theme::global();
        let id = prompt.client_id.clone();
        let row_index = index.unwrap_or(0);
        if !pending
            && let Some(edit) = self
                .prompt_queue
                .queued_edit
                .as_ref()
                .filter(|edit| matches!(&edit.target, QueueEditTarget::Remote(edit_id) if *edit_id == id))
        {
            return self.queue_edit_row(
                edit,
                SharedString::from(format!("daemon-queued-edit-{id}")),
                format!("remote-queue-row-{row_index}"),
                cx,
            );
        }
        let key = format!("remote-{id}");
        let expanded = self.prompt_queue.card_expanded.as_deref() == Some(&key);
        let selected = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let key_text = key.clone();
        let mode = prompt.mode;
        let send_id = id.clone();
        let retry_id = id.clone();
        let retry = prompt.clone();
        let delete_prompt = prompt.clone();
        let mutations = self.prompt_queue.remote_mutations_supported
            && !self.prompt_queue.remote_removing.contains(&id);
        let editing_supported = self.prompt_queue.remote_editing_supported;
        let failed = self.prompt_queue.remote_failures.contains_key(&id);
        let can_send = !pending && self.queue_connection_ready() && self.queued_remote_sendable(prompt);
        let group: SharedString = format!("queue-row-remote-{id}").into();
        div()
            .id(SharedString::from(format!("daemon-queued-{id}")))
            .when(!pending, |el| {
                el.debug_selector(move || format!("remote-queue-row-{row_index}"))
            })
            .group(group.clone())
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .rounded_lg()
            .when(selected, |el| el.bg(theme.QUOTE_BG))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .h(px(30.))
                    .px_2()
                    .child(
                        gpui::svg()
                            .data(INDENT)
                            .size(px(14.))
                            .text_color(theme.TEXT_DIM),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("queue-text-{id}")))
                            .debug_selector(move || format!("queue-row-text-remote-{row_index}"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .cursor_pointer()
                            .child(self.render_queued_content(&prompt.content, &prompt.images, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.prompt_queue.card_expanded = if this
                                    .prompt_queue
                                    .card_expanded
                                    .as_deref()
                                    == Some(&key_text)
                                {
                                    None
                                } else {
                                    Some(key_text.clone())
                                };
                                this.prompt_queue.card_menu = Some(key_text.clone());
                                window.focus(&this.focus_handle, cx);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .when(prompt.editing, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("queue-editing-{id}")))
                                .debug_selector(|| "remote-queue-editing".into())
                                .flex_none()
                                .rounded_full()
                                .px_2()
                                .py_1()
                                .text_color(theme.TEXT_DIM)
                                .child("editing"),
                        )
                    })
                    .when(failed, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("queue-retry-{id}")))
                                .debug_selector(|| "queue-send-control".into())
                                .flex()
                                .items_center()
                                .gap_1()
                                .rounded_full()
                                .px_2()
                                .cursor_pointer()
                                .text_color(theme.ERROR)
                                .child("Retry")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prompt_queue.remote_failures.remove(&retry_id);
                                    if pending {
                                        this.bridge.send(Command::QueuePrompt {
                                            session_id: this.session_id.clone(),
                                            prompt: retry.clone(),
                                        });
                                    } else {
                                        this.send_remote_queue_row(&retry_id, mode, cx);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(can_send, |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("queue-row-send-now-{row_index}")),
                            SEND,
                            Kit::send_label(mode == QueuedPromptMode::AfterYield),
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, _, cx| {
                                this.prompt_queue.card_menu = None;
                                this.send_remote_queue_row(&send_id, mode, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .when(mutations && editing_supported && !pending && !prompt.editing, |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("remote-queue-edit-{row_index}")),
                            PENCIL,
                            "Edit",
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, window, cx| {
                                this.prompt_queue.card_menu = None;
                                this.begin_queued_prompt_edit(row_index, window, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .when(!pending && mutations, |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("remote-queue-delete-{row_index}")),
                            TRASH,
                            "Delete",
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, _, cx| {
                                this.remove_remote_card_prompt(delete_prompt.clone(), false, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    }),
            )
            .when(expanded, |el| {
                el.child(self.render_expanded_queued_content(&prompt.content, &prompt.images, cx))
            })
            .when_some(
                self.prompt_queue.remote_failures.get(&id).cloned(),
                |el, reason| el.child(div().px_2().text_color(theme.ERROR).child(reason)),
            )
            .into_any_element()
    }
}

impl Panel {
    /// A queued message open for editing, drawn in its own row: the full
    /// message in an editor with its attachments as chips, then Save and
    /// Cancel. The composer below is not involved.
    fn queue_edit_row(
        &self,
        edit: &QueuedPromptEditState,
        id: SharedString,
        selector: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let input = edit.input.clone();
        let drop_input = input.clone();
        let (note, save_label) = match edit.phase {
            QueueEditPhase::Editing => (None, "Save"),
            QueueEditPhase::Saving => (None, "Saving\u{2026}"),
            QueueEditPhase::Recovered(note) => (Some(note), "Queue as new"),
        };
        let saving = edit.phase == QueueEditPhase::Saving;
        let can_requeue = !matches!(edit.phase, QueueEditPhase::Recovered(_))
            || (self.queue_connection_ready() && self.prompt_queue.remote_supported);
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .min_w_0()
            .px_1()
            .py_1()
            .rounded_xl()
            .bg(theme.QUOTE_BG)
            .anchor_scroll(Some(self.queue_edit_anchor.clone()))
            // Files dropped on the row attach to this message, not the composer.
            .drag_over::<gpui::ExternalPaths>(|style, _, _, _| style.bg(Theme::global().SELECTION))
            .on_drop(move |paths: &gpui::ExternalPaths, window, cx| {
                let paths = paths.paths().to_vec();
                let input = drop_input.clone();
                window.defer(cx, move |window, cx| {
                    input.update(cx, |input, cx| input.drop_files(&paths, window, cx));
                });
            })
            .child(
                div()
                    .debug_selector(|| "queue-edit-input".into())
                    .w_full()
                    .min_w_0()
                    .child(input),
            )
            .when_some(note, |el, note| {
                el.child(
                    div()
                        .debug_selector(|| "queue-edit-note".into())
                        .px_3()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(note),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .px_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_FAINT)
                            .child("Enter to save \u{b7} Esc to cancel"),
                    )
                    .children(
                        [
                            (false, "queue-edit-mode-steer", "Steer"),
                            (true, "queue-edit-mode-after-yield", "After yield"),
                        ]
                        .map(|(after_yield, selector, label)| {
                            Kit::pill(
                                selector,
                                label,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(edit) = this.prompt_queue.queued_edit.as_mut()
                                        && edit.phase != QueueEditPhase::Saving
                                    {
                                        edit.after_yield = after_yield;
                                        cx.notify();
                                    }
                                    cx.stop_propagation();
                                }),
                            )
                            .bg(if edit.after_yield == after_yield {
                                theme.TOOL_BG
                            } else {
                                theme.QUOTE_BG
                            })
                            .when(edit.after_yield == after_yield, |el| {
                                el.text_color(theme.TEXT)
                            })
                            .when(saving, |el| el.opacity(0.6))
                        }),
                    )
                    .child(Kit::pill(
                        "queue-edit-cancel",
                        "Cancel",
                        cx.listener(|this, _, window, cx| {
                            this.cancel_queued_prompt_edit(window, cx);
                            cx.stop_propagation();
                        }),
                    ))
                    .child(
                        div()
                            .id("queue-edit-save")
                            .debug_selector(|| "queue-edit-save".into())
                            .flex_none()
                            .h(px(22.))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded_full()
                            .text_size(px(11.))
                            .bg(theme.ACCENT)
                            .text_color(theme.BG)
                            .when(saving || !can_requeue, |el| el.opacity(0.6))
                            .when(!saving && can_requeue, |el| el.cursor_pointer())
                            .child(save_label)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_queued_prompt_edit(window, cx);
                                cx.stop_propagation();
                            })),
                    ),
            )
            .into_any_element()
    }
}



struct QueueActionTooltip(&'static str);

impl gpui::Render for QueueActionTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .px_2()
            .py_1()
            .rounded_full()
            .bg(theme.TOOL_BG)
            .text_size(px(11.))
            .text_color(theme.TEXT)
            .child(self.0)
    }
}

/// The small widgets of the queue card.
pub(super) struct Kit;

impl Kit {
    /// The queue card reads as part of the chat column, not as a panel of its own.
    /// It tints the dock veil barely enough to group the rows; the transcript
    /// behind it stays visible. Tests pin the surface as non-opaque.
    pub(super) fn surface(theme: &Palette) -> gpui::Rgba {
        let mut surface = theme.QUOTE_BG;
        surface.a = surface.a.min(0.06);
        surface
    }

    pub(super) fn send_label(after_yield: bool) -> &'static str {
        if after_yield {
            "Move to Steered messages"
        } else {
            "Interrupt now"
        }
    }

    pub(super) fn icon(icon: &'static [u8], size: f32) -> gpui::Svg {
        gpui::svg().data(icon).size(px(size))
    }

    /// Header pill: a fully rounded button in the card's own ink.
    pub(super) fn pill(
        id: &'static str,
        label: &'static str,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::global();
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .flex_none()
            .h(px(20.))
            .px_2()
            .flex()
            .items_center()
            .rounded_full()
            .text_size(px(11.))
            .text_color(theme.TEXT_DIM)
            .cursor_pointer()
            .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
            .child(label)
            .on_click(on_click)
    }

    pub(super) fn action(
        id: &'static str,
        index: usize,
        label: &'static str,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let theme = Theme::global();
        div()
            .id((id, index))
            .debug_selector(move || format!("{id}-{index}"))
            .rounded_full()
            .px_2()
            .py_1()
            .cursor_pointer()
            .text_color(theme.TEXT_DIM)
            .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
            .child(label)
            .on_click(on_click)
    }
}

fn queue_row_action(
    id: SharedString,
    icon: &'static [u8],
    label: &'static str,
    group: SharedString,
    selected: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let theme = Theme::global();
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .flex_none()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .opacity(if selected { 1.0 } else { 0.85 })
        .group_hover(group, |el| el.opacity(1.0))
        .hover(|el| el.bg(theme.TOOL_BG))
        .tooltip(move |_, cx| cx.new(|_| QueueActionTooltip(label)).into())
        .child(Kit::icon(icon, 13.).text_color(theme.TEXT_DIM))
        .on_click(on_click)
}


