use super::*;

const INDENT: &[u8] = include_bytes!("../../../assets/icons/queue-indent.svg");
const TRASH: &[u8] = include_bytes!("../../../assets/icons/trash.svg");

/// The queue card reads as part of the chat column, not as a panel of its own.
/// It tints the dock veil barely enough to group the rows; the transcript
/// behind it stays visible. Tests pin the surface as non-opaque.
pub(super) fn queue_card_surface(theme: &Theme) -> gpui::Rgba {
    let mut surface = theme.QUOTE_BG;
    surface.a = surface.a.min(0.06);
    surface
}

impl Panel {
    pub(in crate::panel) fn handle_queue_card_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
                return false;
            };
            let key = event.keystroke.key.as_str();
            let alt = event.keystroke.modifiers.alt;
            match (key, alt) {
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
                ("enter", false) => {
                    self.interrupt_all_queued_prompts(cx);
                    self.prompt_queue.card_menu = None;
                    self.focus_input(window, cx);
                }
                ("escape", false) => {
                    self.prompt_queue.card_menu = None;
                    self.focus_input(window, cx);
                }
                _ => return false,
            }
            cx.notify();
            return true;
        }
        let Some(index) = self
            .prompt_queue
            .card_menu
            .as_deref()
            .and_then(|key| key.strip_prefix("local-"))
            .and_then(|index| index.parse::<usize>().ok())
        else {
            return false;
        };
        if index >= self.prompt_queue.prompts.len() {
            self.prompt_queue.card_menu = None;
            return false;
        }
        let key = event.keystroke.key.as_str();
        let alt = event.keystroke.modifiers.alt;
        match (key, alt) {
            ("up", true) if index > 0 => {
                self.prompt_queue.prompts.swap(index, index - 1);
                self.prompt_queue.card_menu = Some(format!("local-{}", index - 1));
            }
            ("down", true) if index + 1 < self.prompt_queue.prompts.len() => {
                self.prompt_queue.prompts.swap(index, index + 1);
                self.prompt_queue.card_menu = Some(format!("local-{}", index + 1));
            }
            ("up" | "down", true) => {}
            ("m", false) => {
                self.prompt_queue.prompts[index].after_yield =
                    !self.prompt_queue.prompts[index].after_yield
            }
            ("enter", false) if self.queue_connection_ready() => {
                self.prompt_queue.card_menu = None;
                self.send_queued_prompt_now(index, cx);
                self.focus_input(window, cx);
            }
            // Esc only dismisses what the card opened: it closes the row
            // actions and gives focus back to the composer. Stopping a turn
            // is a separate action, so Esc never pauses the queue by itself.
            ("escape", false) => {
                self.prompt_queue.card_menu = None;
                self.prompt_queue.card_expanded = None;
                self.focus_input(window, cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    pub(super) fn render_queue_card(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let queue = &self.prompt_queue;
        if queue.prompts.is_empty()
            && queue.steered.is_empty()
            && queue.remote.is_empty()
            && queue.pending_remote.is_empty()
        {
            return None;
        }
        let theme = Theme::global();
        Some(
            div()
                .id("queue-card")
                .debug_selector(|| "queue-card".into())
                // Same width and edges as the composer below it: the card is a
                // continuation of the chat column, not a floating panel.
                .w_full()
                .min_w_0()
                .flex_none()
                .flex()
                .flex_col()
                .rounded_lg()
                .bg(queue_card_surface(theme))
                .pb(px(2.))
                .text_size(px(12.))
                .text_color(theme.TEXT)
                .child(self.queue_card_header(cx))
                .child(
                    div()
                        .id("prompt-queue-items")
                        .debug_selector(|| "prompt-queue-items".into())
                        .flex()
                        .flex_col()
                        .gap_1()
                        .max_h(px(144.))
                        .overflow_y_scroll()
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .when(
                            !queue.remote.is_empty() || !queue.pending_remote.is_empty(),
                            |el| {
                                el.child(
                                    div()
                                        .debug_selector(|| "daemon-prompt-queue".into())
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .children(queue.remote.iter().enumerate().map(
                                            |(index, prompt)| {
                                                self.card_remote_row(prompt, Some(index), cx)
                                            },
                                        ))
                                        .children(
                                            queue.pending_remote.iter().map(|prompt| {
                                                self.card_remote_row(prompt, None, cx)
                                            }),
                                        ),
                                )
                            },
                        )
                        .when(!queue.steered.is_empty(), |el| {
                            el.child(
                                div()
                                    .debug_selector(|| "steered-prompts".into())
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .children(
                                        queue
                                            .steered
                                            .iter()
                                            .enumerate()
                                            .map(|(i, prompt)| self.card_legacy_row(i, prompt, cx)),
                                    ),
                            )
                        })
                        .when(!queue.prompts.is_empty(), |el| {
                            el.child(
                                div()
                                    .debug_selector(|| "prompt-queue".into())
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .children(
                                        queue
                                            .prompts
                                            .iter()
                                            .enumerate()
                                            .map(|(i, prompt)| self.card_local_row(i, prompt, cx)),
                                    ),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    fn queue_card_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let waiting = self.queue_waiting_for_connection();
        let paused = self.prompt_queue.paused && !self.prompt_queue.prompts.is_empty();
        let trailing = if paused {
            queue_pill(
                "queue-resume",
                "Queue paused · Resume",
                cx.listener(|this, _, _, cx| {
                    this.prompt_queue.paused = false;
                    this.send_queued_prompts(cx);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element()
        } else if waiting {
            // A reconnect is transient. Say what is happening in neutral ink
            // instead of leaving a permanent failure in red.
            div()
                .id("queue-waiting")
                .debug_selector(|| "queue-waiting".into())
                .flex_none()
                .h(px(20.))
                .px_2()
                .flex()
                .items_center()
                .rounded_full()
                .text_color(theme.TEXT_DIM)
                .child("Waiting for connection…")
                .into_any_element()
        } else {
            queue_pill(
                "queue-send-all-now",
                "Send all now",
                cx.listener(|this, _, _, cx| {
                    this.interrupt_all_queued_prompts(cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(26.))
            .px_2()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.TEXT_DIM)
                    .child("Queued messages"),
            )
            .child(trailing)
            .into_any_element()
    }

    fn card_local_row(
        &self,
        index: usize,
        prompt: &QueuedPrompt,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let key = format!("local-{index}");
        let expanded = self.prompt_queue.card_expanded.as_deref() == Some(&key);
        let selected = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let preview = self.render_queued_content(&prompt.content, &prompt.images, cx);
        let full = prompt.content.clone();
        let key_text = key.clone();
        let key_menu = key.clone();
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
                    .child(
                        div()
                            .id(("local-queue-mode", index))
                            .debug_selector(move || format!("queue-mode-button-{index}"))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(20.))
                            .rounded_full()
                            .px_2()
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
                            .child(
                                gpui::svg()
                                    .data(INDENT)
                                    .size(px(12.))
                                    .text_color(theme.TEXT_DIM),
                            )
                            .child(if prompt.after_yield {
                                "After yield"
                            } else {
                                "Steer"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(prompt) = this.prompt_queue.prompts.get_mut(index) {
                                    prompt.after_yield = !prompt.after_yield;
                                }
                                this.prompt_queue.card_menu = Some(key_menu.clone());
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .child(queue_row_action(
                        SharedString::from(format!("recall-queued-prompt-{index}")),
                        "Edit",
                        group.clone(),
                        selected,
                        cx.listener(move |this, _, window, cx| {
                            this.prompt_queue.card_menu = None;
                            this.recall_queued_prompt(index, window, cx);
                            cx.stop_propagation();
                        }),
                    ))
                    .when(self.queue_connection_ready(), |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("send-queued-prompt-{index}")),
                            "Send now",
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, _, cx| {
                                this.prompt_queue.card_menu = None;
                                this.send_queued_prompt_now(index, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .child(queue_row_action(
                        SharedString::from(format!("remove-queued-prompt-{index}")),
                        "Remove",
                        group.clone(),
                        selected,
                        cx.listener(move |this, _, _, cx| {
                            if index < this.prompt_queue.prompts.len() {
                                this.prompt_queue.prompts.remove(index);
                            }
                            this.prompt_queue.card_menu = None;
                            this.prompt_queue.card_expanded = None;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )),
            )
            .when(expanded, |el| {
                el.child(div().px_4().pb_2().text_color(theme.TEXT).child(full))
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
            .child(div().text_color(theme.TEXT_DIM).child("Steer"))
            .when(!self.activity_active(), |row| {
                row.child(menu_action(
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
        let key = format!("remote-{id}");
        let expanded = self.prompt_queue.card_expanded.as_deref() == Some(&key);
        let selected = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let key_text = key.clone();
        let mode_id = id.clone();
        let retry_id = id.clone();
        let retry = prompt.clone();
        let delete_prompt = prompt.clone();
        let mutations = self.prompt_queue.remote_mutations_supported
            && !self.prompt_queue.remote_removing.contains(&id);
        let editing_supported = self.prompt_queue.remote_editing_supported;
        let full = prompt.content.clone();
        let failed = self.prompt_queue.remote_failures.contains_key(&id);
        let next = match prompt.mode {
            jcode_sdk::api::QueuedPromptMode::Steer => jcode_sdk::api::QueuedPromptMode::AfterYield,
            jcode_sdk::api::QueuedPromptMode::AfterYield => jcode_sdk::api::QueuedPromptMode::Steer,
        };
        let group: SharedString = format!("queue-row-remote-{id}").into();
        let row_index = index.unwrap_or(0);
        div()
            .id(SharedString::from(format!("daemon-queued-{id}")))
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
                    // The row names its delivery mode and switches it in
                    // place; the daemon owns the batch itself.
                    .child(
                        div()
                            .id(SharedString::from(format!("queue-mode-{id}")))
                            .debug_selector(move || format!("remote-queue-mode-{row_index}"))
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .h(px(20.))
                            .rounded_full()
                            .px_2()
                            .text_size(px(11.))
                            .text_color(theme.TEXT_DIM)
                            .when(!pending, |el| {
                                el.cursor_pointer().hover(|el| el.bg(theme.TOOL_BG))
                            })
                            .child(
                                gpui::svg()
                                    .data(INDENT)
                                    .size(px(12.))
                                    .text_color(theme.TEXT_DIM),
                            )
                            .child(match prompt.mode {
                                jcode_sdk::api::QueuedPromptMode::Steer => "Steer",
                                jcode_sdk::api::QueuedPromptMode::AfterYield => "After yield",
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if pending {
                                    return;
                                }
                                this.bridge.send(Command::SetQueuedPromptMode {
                                    session_id: this.session_id.clone(),
                                    client_id: mode_id.clone(),
                                    mode: next,
                                });
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
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
                                        this.interrupt_all_queued_prompts(cx);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(mutations && editing_supported && !pending, |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("remote-queue-edit-{row_index}")),
                            "Edit",
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, window, cx| {
                                this.prompt_queue.card_menu = None;
                                this.begin_queued_prompt_edit(index.unwrap_or(0), window, cx);
                                this.focus_input(window, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .when(!pending && self.queue_connection_ready(), |el| {
                        el.child(queue_row_action(
                            SharedString::from(format!("queue-row-send-now-{row_index}")),
                            "Send now",
                            group.clone(),
                            selected,
                            cx.listener(move |this, _, _, cx| {
                                this.prompt_queue.card_menu = None;
                                this.interrupt_all_queued_prompts(cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .when(!pending && mutations, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("queue-delete-{id}")))
                                .debug_selector(|| "remote-queue-delete".into())
                                .flex_none()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.QUOTE_BG))
                                .child(
                                    gpui::svg()
                                        .data(TRASH)
                                        .size(px(12.))
                                        .text_color(theme.TEXT_DIM),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_remote_card_prompt(
                                        delete_prompt.clone(),
                                        false,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                })),
                        )
                    }),
            )
            .when(expanded, |el| {
                el.child(div().px_4().pb_2().text_color(theme.TEXT).child(full))
            })
            .when_some(
                self.prompt_queue.remote_failures.get(&id).cloned(),
                |el, reason| el.child(div().px_2().text_color(theme.ERROR).child(reason)),
            )
            .into_any_element()
    }
}

/// A compact pill action that sits on its queue row. It is always part of the
/// row (no overflow menu); hovering the row or selecting it brightens it.
fn queue_row_action(
    id: SharedString,
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
        .h(px(20.))
        .px_2()
        .flex()
        .items_center()
        .rounded_full()
        .text_size(px(11.))
        .text_color(theme.TEXT_DIM)
        .cursor_pointer()
        .opacity(if selected { 1.0 } else { 0.75 })
        .group_hover(group, |el| el.opacity(1.0))
        .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
        .child(label)
        .on_click(on_click)
}

/// Header pill: a fully rounded button in the card's own ink.
fn queue_pill(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
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

fn menu_action(
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
