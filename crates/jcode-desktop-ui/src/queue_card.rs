use super::*;

const INDENT: &[u8] = include_bytes!("../../../assets/icons/queue-indent.svg");
const TRASH: &[u8] = include_bytes!("../../../assets/icons/trash.svg");
const OVERFLOW: &[u8] = include_bytes!("../../../assets/icons/overflow.svg");

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
                    self.bridge.send(Command::InterruptQueuedPrompt {
                        session_id: self.session_id.clone(),
                        client_id: id,
                    });
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
            ("enter", false)
                if !self.is_pending_session() && !self.prompt_queue.waiting_for_connection =>
            {
                self.prompt_queue.card_menu = None;
                self.send_queued_prompt_now(index, cx);
                self.focus_input(window, cx);
            }
            ("escape", false) => {
                self.prompt_queue.card_menu = None;
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
                .w_full()
                .min_w_0()
                .flex_none()
                .flex()
                .flex_col()
                .rounded_t_lg()
                .border_1()
                .border_b_0()
                .border_color(theme.INPUT_BORDER)
                .bg(theme.INLINE_CODE_BG)
                .pb(px(4.))
                .pt_1()
                .px_1()
                .text_size(px(12.))
                .text_color(theme.TEXT)
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
                .when(queue.paused && !queue.prompts.is_empty(), |el| {
                    el.child(
                        div()
                            .id("resume-prompt-queue")
                            .rounded_full()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.QUOTE_BG))
                            .child("Resume queue")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prompt_queue.paused = false;
                                this.send_queued_prompts(cx);
                                cx.notify();
                            })),
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
        let key = format!("local-{index}");
        let expanded = self.prompt_queue.card_expanded.as_deref() == Some(&key);
        let menu = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let preview = self.render_queued_content(&prompt.content, &prompt.images, cx);
        let full = prompt.content.clone();
        let key_text = key.clone();
        let key_menu = key.clone();
        let theme = Theme::global();
        div()
            .id(("queued-prompt", index))
            .debug_selector(move || format!("queued-prompt-{index}"))
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .rounded_lg()
            .hover(|el| el.bg(theme.QUOTE_BG))
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
                            .on_click(cx.listener(move |this, _, _, cx| {
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
                            .rounded_full()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
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
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .child(queue_action(
                        "remove-queued-prompt",
                        index,
                        TRASH,
                        cx.listener(move |this, _, _, cx| {
                            if index < this.prompt_queue.prompts.len() {
                                this.prompt_queue.prompts.remove(index);
                            }
                            this.prompt_queue.card_menu = None;
                            this.prompt_queue.card_expanded = None;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ))
                    .child(queue_action(
                        "queue-overflow",
                        index,
                        OVERFLOW,
                        cx.listener(move |this, _, window, cx| {
                            this.prompt_queue.card_menu =
                                if this.prompt_queue.card_menu.as_deref() == Some(&key_menu) {
                                    None
                                } else {
                                    Some(key_menu.clone())
                                };
                            if this.prompt_queue.card_menu.is_some() {
                                window.focus(&this.focus_handle, cx);
                            } else {
                                this.focus_input(window, cx);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )),
            )
            .when(expanded, |el| {
                el.child(div().px_4().pb_2().text_color(theme.TEXT).child(full))
            })
            .when(menu, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .px_2()
                        .pb_2()
                        .child(menu_action(
                            "recall-queued-prompt",
                            index,
                            "Edit",
                            cx.listener(move |this, _, window, cx| {
                                this.prompt_queue.card_menu = None;
                                this.recall_queued_prompt(index, window, cx);
                                cx.stop_propagation();
                            }),
                        ))
                        .when(
                            !self.prompt_queue.waiting_for_connection && !self.is_pending_session(),
                            |el| {
                                el.child(menu_action(
                                    "send-queued-prompt",
                                    index,
                                    "Send now",
                                    cx.listener(move |this, _, _, cx| {
                                        this.prompt_queue.card_menu = None;
                                        this.send_queued_prompt_now(index, cx);
                                        cx.stop_propagation();
                                    }),
                                ))
                            },
                        )
                        .when(index > 0, |el| {
                            el.child(menu_action(
                                "queue-move-up",
                                index,
                                "Move up",
                                cx.listener(move |this, _, _, cx| {
                                    if index < this.prompt_queue.prompts.len() && index > 0 {
                                        this.prompt_queue.prompts.swap(index, index - 1);
                                    }
                                    this.prompt_queue.card_menu = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ))
                        })
                        .when(index + 1 < self.prompt_queue.prompts.len(), |el| {
                            el.child(menu_action(
                                "queue-move-down",
                                index,
                                "Move down",
                                cx.listener(move |this, _, _, cx| {
                                    if index + 1 < this.prompt_queue.prompts.len() {
                                        this.prompt_queue.prompts.swap(index, index + 1);
                                    }
                                    this.prompt_queue.card_menu = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ))
                        }),
                )
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
        let menu = self.prompt_queue.card_menu.as_deref() == Some(&key);
        let key_text = key.clone();
        let key_menu = key.clone();
        let mode_id = id.clone();
        let interrupt_id = id.clone();
        let retry_id = id.clone();
        let retry = prompt.clone();
        let delete_prompt = prompt.clone();
        let edit_prompt = prompt.clone();
        let mutations = self.prompt_queue.remote_mutations_supported
            && !self.prompt_queue.remote_removing.contains(&id);
        let full = prompt.content.clone();
        let failed = self.prompt_queue.remote_failures.contains_key(&id);
        let next = match prompt.mode {
            jcode_sdk::api::QueuedPromptMode::Steer => jcode_sdk::api::QueuedPromptMode::AfterYield,
            jcode_sdk::api::QueuedPromptMode::AfterYield => jcode_sdk::api::QueuedPromptMode::Steer,
        };
        div()
            .id(SharedString::from(format!("daemon-queued-{id}")))
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .rounded_lg()
            .hover(|el| el.bg(theme.QUOTE_BG))
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
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .cursor_pointer()
                            .child(self.render_queued_content(&prompt.content, &prompt.images, cx))
                            .on_click(cx.listener(move |this, _, _, cx| {
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
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("queue-mode-{id}")))
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .rounded_full()
                            .px_2()
                            .py_1()
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
                            .child(if prompt.mode == jcode_sdk::api::QueuedPromptMode::Steer {
                                "Steer"
                            } else {
                                "After yield"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !pending {
                                    this.bridge.send(Command::SetQueuedPromptMode {
                                        session_id: this.session_id.clone(),
                                        client_id: mode_id.clone(),
                                        mode: next,
                                    });
                                }
                                cx.stop_propagation();
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
                                .child(div().size(px(6.)).rounded_full().bg(theme.ERROR))
                                .child("Retry")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prompt_queue.remote_failures.remove(&retry_id);
                                    this.bridge.send(Command::QueuePrompt {
                                        session_id: this.session_id.clone(),
                                        prompt: retry.clone(),
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!pending && mutations, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("queue-delete-{id}")))
                                .debug_selector(|| "remote-queue-delete".into())
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.QUOTE_BG))
                                .child(
                                    gpui::svg()
                                        .data(TRASH)
                                        .size(px(13.))
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
                    })
                    .when(!pending, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(format!("queue-overflow-{id}")))
                                .debug_selector(move || {
                                    format!("remote-queue-overflow-{}", index.unwrap_or(0))
                                })
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.TOOL_BG))
                                .child(
                                    gpui::svg()
                                        .data(OVERFLOW)
                                        .size(px(13.))
                                        .text_color(theme.TEXT_DIM),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.prompt_queue.card_menu = if this
                                        .prompt_queue
                                        .card_menu
                                        .as_deref()
                                        == Some(&key_menu)
                                    {
                                        None
                                    } else {
                                        Some(key_menu.clone())
                                    };
                                    if this.prompt_queue.card_menu.is_some() {
                                        window.focus(&this.focus_handle, cx);
                                    } else {
                                        this.focus_input(window, cx);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(expanded, |el| {
                el.child(div().px_4().pb_2().text_color(theme.TEXT).child(full))
            })
            .when(menu && !pending, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .px_2()
                        .pb_2()
                        .when(mutations, |el| {
                            el.child(menu_action(
                                "remote-queue-edit",
                                index.unwrap_or(0),
                                "Edit",
                                cx.listener(move |this, _, window, cx| {
                                    this.remove_remote_card_prompt(edit_prompt.clone(), true, cx);
                                    this.focus_input(window, cx);
                                    cx.stop_propagation();
                                }),
                            ))
                        })
                        .when(mutations && index.is_some_and(|i| i > 0), |el| {
                            el.child(menu_action(
                                "remote-queue-up",
                                index.unwrap_or(0),
                                "Move up",
                                cx.listener(move |this, _, _, cx| {
                                    this.reorder_remote_card_prompt(index.unwrap_or(0), true);
                                    this.prompt_queue.card_menu = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ))
                        })
                        .when(
                            mutations
                                && index.is_some_and(|i| i + 1 < self.prompt_queue.remote.len()),
                            |el| {
                                el.child(menu_action(
                                    "remote-queue-down",
                                    index.unwrap_or(0),
                                    "Move down",
                                    cx.listener(move |this, _, _, cx| {
                                        this.reorder_remote_card_prompt(index.unwrap_or(0), false);
                                        this.prompt_queue.card_menu = None;
                                        cx.stop_propagation();
                                        cx.notify();
                                    }),
                                ))
                            },
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("queue-interrupt-{id}")))
                                .debug_selector(|| "queue-send-control".into())
                                .rounded_full()
                                .px_2()
                                .py_1()
                                .cursor_pointer()
                                .text_color(theme.TEXT_DIM)
                                .hover(|el| el.bg(theme.TOOL_BG).text_color(theme.TEXT))
                                .child("Send now / interrupt")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prompt_queue.card_menu = None;
                                    this.bridge.send(Command::InterruptQueuedPrompt {
                                        session_id: this.session_id.clone(),
                                        client_id: interrupt_id.clone(),
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        ),
                )
            })
            .into_any_element()
    }
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
