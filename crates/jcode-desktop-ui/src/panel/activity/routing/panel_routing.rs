//! Account routing in a session panel: the account chip next to the other
//! status pills, the spending-limit gate card, and `/acct`.
//!
//! The daemon decides which account serves a turn. This module only shows the
//! decision, answers a gate with the keys and rows every client shares, and
//! sends routing intents typed as `/acct`.
use super::*;
use crate::input::popup::Popup;
use crate::routing::RoutingChip;
use crate::routing::gate::GateCard;
use crate::routing::popover::{RoutingPopover, Tone};
use jcode_sdk::RoutingIntentAction;
use jcode_session_types::routing::{RoutingGateChoice, RoutingGateOption};
use jcode_session_types::routing_acct::parse_acct;
use jcode_session_types::routing_gate::GateKey;

impl Panel {
    /// Record the daemon's decision for the chip. A move to another account is
    /// announced once as a notice row, with its reason, and keeps the chip up
    /// until the popover has been looked at.
    pub(super) fn routing_decision(
        &mut self,
        account: &str,
        reason: &str,
        warnings: &[String],
        cx: &mut Context<Self>,
    ) {
        let Some(chip) = RoutingChip::new(account.to_owned(), reason.to_owned(), warnings.to_vec())
        else {
            return;
        };
        let mut chip = self.with_account_icons(chip, cx);
        let previous = self.routing_chip.as_ref();
        let left = previous
            .filter(|previous| previous.account != chip.account)
            .map(|previous| (previous.account.clone(), previous.account_text()));
        chip.moved_from = match &left {
            Some((account, _)) => Some(account.clone()),
            None => previous.and_then(|previous| previous.moved_from.clone()),
        };
        if let Some((_, previous)) = left {
            self.items.push(Item::SwarmWorkerNotice(format!(
                "Account: {previous} → {} ({})",
                chip.account_text(),
                chip.reason_text()
            )));
        }
        let account = chip.account.clone();
        self.input
            .update(cx, |input, cx| input.set_current_account(Some(account), cx));
        if self.routing_chip.as_ref() != Some(&chip) {
            self.routing_chip = Some(chip);
            cx.notify();
        }
    }

    /// The popover has been looked at: a past move no longer needs the chip.
    pub(crate) fn routing_move_seen(&mut self) {
        if let Some(chip) = self.routing_chip.as_mut() {
            chip.moved_from = None;
        }
    }

    /// The icon of the account the session runs on, for the composer's model
    /// chip: the routing chip's account, else the OAuth login the runtime
    /// reports. `None` unless the provider has several accounts (or the user
    /// set an icon), since the daemon resolves icons only then.
    pub(crate) fn serving_account_icon(&self, cx: &App) -> Option<String> {
        if let Some(chip) = self.routing_chip.clone() {
            return self.with_account_icons(chip, cx).icon;
        }
        let label = self.account_label.as_deref()?;
        self.input
            .read(cx)
            .login_icon(self.provider.as_deref(), label)
    }

    /// The chip with the icons the routes carry, which the daemon resolves only
    /// where an account has to be told from its siblings.
    fn with_account_icons(&self, chip: RoutingChip, cx: &App) -> RoutingChip {
        let input = self.input.read(cx);
        chip.with_icons(|account| input.route_icon(account))
    }

    pub(super) fn open_routing_gate(
        &mut self,
        gate_id: &str,
        reason: &str,
        options: &[RoutingGateOption],
        cx: &mut Context<Self>,
    ) {
        if self
            .routing_gate
            .as_ref()
            .is_some_and(|gate| gate.view().gate_id == gate_id)
        {
            return;
        }
        self.routing_gate = Some(GateCard::new(
            gate_id.to_owned(),
            reason.to_owned(),
            options.to_vec(),
        ));
        self.transcript_measurements.invalidate();
        cx.notify();
    }

    /// The daemon closed a gate, answered here or elsewhere, or abandoned it.
    pub(super) fn close_routing_gate(&mut self, gate_id: &str, cx: &mut Context<Self>) {
        if self
            .routing_gate
            .as_ref()
            .is_some_and(|gate| gate.view().gate_id == gate_id)
        {
            self.dismiss_routing_gate(cx);
        }
    }

    /// A finished turn cannot still be waiting on a gate.
    pub(super) fn dismiss_routing_gate(&mut self, cx: &mut Context<Self>) {
        if self.routing_gate.take().is_some() {
            self.transcript_measurements.invalidate();
            cx.notify();
        }
    }

    pub(super) fn answer_routing_gate(
        &mut self,
        choice: RoutingGateChoice,
        cx: &mut Context<Self>,
    ) {
        static NEXT_ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let Some(gate) = self.routing_gate.as_mut() else {
            return;
        };
        gate.mark_submitting(NEXT_ATTEMPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.bridge.send(Command::RoutingGateAnswer {
            session_id: self.session_id.clone(),
            gate_id: gate.view().gate_id.to_owned(),
            choice,
        });
        cx.notify();
    }

    /// The daemon's verdict on an answer sent from this panel.
    pub(crate) fn routing_gate_answer_settled(
        &mut self,
        gate_id: &str,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(gate) = self
            .routing_gate
            .as_mut()
            .filter(|gate| gate.view().gate_id == gate_id)
        else {
            return;
        };
        match result {
            Ok(()) => self.dismiss_routing_gate(cx),
            Err(error) => {
                gate.answer_failed(error);
                cx.notify();
            }
        }
    }

    /// Whether the gate card should take the keyboard: from the composer, or
    /// when this is the focused panel and nothing in the window has focus.
    pub(super) fn routing_gate_wants_focus(&self, window: &Window, cx: &App) -> bool {
        self.routing_gate.is_some()
            && (self.input.read(cx).is_focused(window)
                || (self.surface_focused && window.focused(cx).is_none()))
    }

    pub(super) fn handle_routing_gate_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.routing_gate.is_none() {
            return false;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return false;
        }
        // Up, Down, Enter and Escape arrive as `menu` actions.
        let key = match event.keystroke.key.as_str() {
            "tab" => GateKey::Tab,
            digit => match digit.parse::<u8>() {
                Ok(digit) if digit < 10 && digit.to_string() == event.keystroke.key => {
                    GateKey::Digit(digit)
                }
                _ => return false,
            },
        };
        self.press_routing_gate(key, window, cx)
    }

    /// Feed one key to the routing gate while this panel (not the composer or
    /// a card editor) holds the keyboard. False when the gate is not showing
    /// or does not own the keyboard.
    pub(super) fn press_routing_gate(
        &mut self,
        key: GateKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.routing_gate.is_none() {
            return false;
        }
        if !self.focus_handle.is_focused(window)
            && !self
                .transcript_selection
                .read(cx)
                .focus_handle()
                .is_focused(window)
        {
            return false;
        }
        let Some(gate) = self.routing_gate.as_mut() else {
            return false;
        };
        if gate.view().submitting {
            return true;
        }
        match gate.press(key) {
            Some(choice) => self.answer_routing_gate(choice, cx),
            None => cx.notify(),
        }
        true
    }

    /// `/acct`: steer routing for this session. Returns false for any other input.
    /// Every answer, errors included, shows in the account popover.
    pub(super) fn handle_acct_command(&mut self, trimmed: &str, cx: &mut Context<Self>) -> bool {
        let action = match parse_acct(trimmed) {
            Ok(Some(action)) => action,
            Ok(None) => return false,
            Err(usage) => {
                self.routing_intent_reply(Err(usage), cx);
                return true;
            }
        };
        self.send_routing_intent(action, cx);
        true
    }

    fn send_routing_intent(&mut self, action: RoutingIntentAction, cx: &mut Context<Self>) {
        if self.is_pending_session() {
            self.routing_intent_reply(
                Err("Account routing needs a started session. Send a message first.".into()),
                cx,
            );
        } else {
            self.bridge.send(Command::RoutingIntent {
                session_id: self.session_id.clone(),
                action,
            });
            cx.notify();
        }
    }

    /// Show the daemon's answer to a routing intent in the account popover.
    /// Routing text is never a transcript item.
    pub(crate) fn routing_intent_reply(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        self.swarm_list_open = false;
        self.routing_popover.get_or_insert_default().show(result);
        cx.notify();
    }

    /// The chip click: open the popover, or close it.
    fn toggle_routing_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.routing_popover.is_some() {
            self.close_routing_popover(window, cx);
            return;
        }
        if self.picker_search.take().is_some() {
            self.model_picker_open = false;
        }
        self.swarm_list_open = false;
        self.composer_popover_trigger = Some(self.routing_trigger_focus.clone());
        self.routing_popover = Some(RoutingPopover::default());
        window.focus(&self.routing_popover_focus, cx);
        cx.notify();
    }

    pub(super) fn close_routing_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_composer_popover(&self.routing_popover_focus.clone(), false, window, cx);
        window.focus(&self.routing_trigger_focus, cx);
    }

    /// The popover above the composer: a definite width so a long row wraps
    /// onto more lines instead of being cut, and a scroll when it outgrows the window.
    pub(super) fn render_routing_popover(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let popover = self.routing_popover.as_ref()?;
        let chip = self
            .routing_chip
            .clone()
            .map(|chip| self.with_account_icons(chip, cx));
        let rows = popover.rows(chip.as_ref());
        let theme = Theme::global();
        let owner = cx.weak_entity();
        let focus = self.routing_popover_focus.clone();
        Some(
            gpui::deferred(
                Popup::new(
                    div()
                        .id("routing-popover")
                        .debug_selector(|| "routing-popover".into())
                        .track_focus(&self.routing_popover_focus)
                        .key_context(crate::menu::CONTEXT)
                        .w(px(460.))
                        .max_w_full()
                        .max_h_full()
                        .px_3()
                        .py_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_xl()
                        .bg(theme.HEADER_BG)
                        .border_1()
                        .border_color(theme.PANEL_BORDER_FOCUS)
                        .shadow_lg()
                        .occlude()
                        .text_size(px(11.))
                        .child(
                            div()
                                .id("routing-popover-rows")
                                .debug_selector(|| "routing-popover-rows".into())
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .overflow_y_scroll()
                                .children(rows.into_iter().enumerate().map(|(index, row)| {
                                    div()
                                        .debug_selector(move || {
                                            format!("routing-popover-row-{index}")
                                        })
                                        .flex_none()
                                        .min_w_0()
                                        .line_height(px(16.))
                                        .whitespace_normal()
                                        .font_family(theme.FONT_MONO)
                                        .text_color(match row.tone {
                                            Tone::Plain => theme.TEXT_DIM,
                                            Tone::Warn => theme.WARN,
                                        })
                                        .child(row.text)
                                })),
                        )
                        .child(
                            div()
                                .id("routing-popover-manage")
                                .debug_selector(|| "routing-popover-manage".into())
                                .flex_none()
                                .line_height(px(16.))
                                .text_color(theme.ACCENT)
                                .cursor_pointer()
                                .hover(|el| el.underline())
                                .child("Manage accounts")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.close_routing_popover(window, cx);
                                    window.dispatch_action(
                                        Box::new(crate::ui_actions::OpenAccounts {
                                            source: cx.entity_id(),
                                            login_command: None,
                                        }),
                                        cx,
                                    );
                                })),
                        ),
                    px(5.),
                )
                .at_composer()
                .dismiss_outside(Some(std::rc::Rc::new(move |window, cx| {
                    let _ = owner.update(cx, |panel, cx| {
                        panel.dismiss_composer_popover(&focus, true, window, cx);
                    });
                }))),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The account chip: only while the daemon moved the session or warns about
    /// its account. The model chip already carries the account's icon.
    pub(super) fn render_routing_chip(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let chip =
            self.with_account_icons(self.routing_chip.clone().filter(RoutingChip::is_alert)?, cx);
        let theme = Theme::global();
        let marker = chip.warning_marker();
        let account = chip.account_text();
        Some(
            crate::keyboard::popover_trigger(
                super::chrome::composer_pill("composer-routing-chip"),
                &self.routing_trigger_focus,
            )
            .relative()
            .children(crate::fixture_control_targets::marker(
                || "routing-chip".into(),
                || Some(self.session_id.clone()),
            ))
            .flex_shrink_1()
            .min_w_0()
            .max_w(px(220.))
            .overflow_hidden()
            .px_2()
            .gap_1()
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.toggle_routing_popover(window, cx);
            }))
            .child(
                div()
                    .debug_selector(|| "composer-routing-account".into())
                    .min_w_0()
                    .truncate()
                    .child(account),
            )
            .children(marker.map(|marker| {
                div()
                    .debug_selector(|| "composer-routing-warning".into())
                    .flex_none()
                    .text_color(theme.WARN)
                    .child(marker)
            }))
            .into_any_element(),
        )
    }

    /// The gate card: the reason, one pill row per way out, and Cancel.
    pub(super) fn render_routing_gate_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let gate = self.routing_gate.as_ref()?;
        let theme = Theme::global();
        let focused = self.focus_handle.is_focused(window);
        let view = gate.view();
        let submitting = view.submitting;
        let mut rows = div()
            .debug_selector(|| "routing-gate-rows".into())
            .flex()
            .flex_col()
            .gap_1();
        let row_count = view.rows.len();
        for (index, text) in view.rows.into_iter().enumerate() {
            let selected = view.cursor == index;
            let cancel = index + 1 == row_count;
            rows = rows.child(
                crate::keyboard::button(div().id(("routing-gate-row", index)))
                    .debug_selector(move || format!("routing-gate-row-{index}"))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        move || format!("routing-gate-row-{index}"),
                        || Some(self.session_id.clone()),
                    ))
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_full()
                    .border_1()
                    .border_color(if selected && focused {
                        theme.ACCENT
                    } else {
                        theme.wash(crate::theme::Ink::Accent, 0.0)
                    })
                    .when(selected, |el| el.bg(theme.ACCENT_DIM))
                    .when(!submitting, |el| {
                        el.cursor_pointer().hover(|el| el.bg(theme.ACCENT_DIM))
                    })
                    .text_color(if submitting {
                        theme.TEXT_DIM
                    } else {
                        theme.TEXT
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(px(14.))
                            .text_color(theme.TEXT_DIM)
                            .child((index + 1).to_string()),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("routing-gate-row-{index}-text"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(text),
                    )
                    .when(cancel, |el| el.text_color(theme.TEXT_DIM))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.focus_handle, cx);
                        let choice = this
                            .routing_gate
                            .as_mut()
                            .and_then(|gate| gate.choose_row(index));
                        match choice {
                            Some(choice) => this.answer_routing_gate(choice, cx),
                            None => cx.notify(),
                        }
                    })),
            );
        }
        Some(
            div()
                .id("routing-gate-card")
                .debug_selector(|| "routing-gate-card".into())
                .occlude()
                .flex_none()
                .flex()
                .flex_col()
                .gap_2()
                .w_full()
                .min_w_0()
                .p_2()
                .rounded_lg()
                .border_1()
                .border_color(theme.INPUT_BORDER)
                .bg(theme.INPUT_BG)
                .text_size(px(13.))
                .text_color(theme.TEXT)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_size(px(11.))
                        .text_color(theme.TEXT_DIM)
                        .child(
                            div()
                                .debug_selector(|| "routing-gate-title".into())
                                .child("Routing · limit reached"),
                        )
                        .child(div().truncate().child(if submitting {
                            "Sending…"
                        } else {
                            "↑↓ move · Enter choose · Esc cancel"
                        })),
                )
                .child(
                    div()
                        .debug_selector(|| "routing-gate-reason".into())
                        .min_w_0()
                        .child(gate.view().reason.to_owned()),
                )
                .child(rows)
                .children(gate.error.as_ref().map(|error| {
                    div()
                        .debug_selector(|| "routing-gate-error".into())
                        .text_size(px(11.))
                        .text_color(theme.WARN)
                        .child(error.clone())
                }))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "panel_routing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_routing_popover_tests.rs"]
mod popover_tests;
