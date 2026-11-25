//! Account routing in a session panel: the account chip next to the other
//! status pills, the spending-limit gate card, and `/acct`.
//!
//! The daemon decides which account serves a turn. This module only shows the
//! decision, answers a gate with the keys and rows every client shares, and
//! sends routing intents typed as `/acct`.
use super::*;
use crate::routing::{GateCard, RoutingChip};
use jcode_session_types::routing::{RoutingGateChoice, RoutingGateOption};
use jcode_session_types::routing_acct::parse_acct;
use jcode_session_types::routing_gate::GateKey;

impl Panel {
    /// Record the daemon's decision for the chip. A move to another account is
    /// announced once in the transcript, with its reason.
    pub(super) fn routing_decision(
        &mut self,
        account: &str,
        reason: &str,
        warnings: &[String],
        pending: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(chip) = RoutingChip::new(
            account.to_owned(),
            reason.to_owned(),
            warnings.to_vec(),
            pending.map(str::to_owned),
        ) else {
            return;
        };
        let moved = self
            .routing_chip
            .as_ref()
            .is_some_and(|previous| previous.account != chip.account);
        if moved {
            self.items.push(Item::Assistant(format!(
                "Routing: now on {} ({})",
                chip.account,
                if chip.reason.is_empty() {
                    "no reason given"
                } else {
                    &chip.reason
                }
            )));
        }
        if self.routing_chip.as_ref() != Some(&chip) {
            self.routing_chip = Some(chip);
            cx.notify();
        }
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
            .is_some_and(|gate| gate.gate_id() == gate_id)
        {
            return;
        }
        self.routing_gate = Some(GateCard::new(
            gate_id.to_owned(),
            reason.to_owned(),
            options.to_vec(),
        ));
        self.transcript_measurements.dirty = true;
        cx.notify();
    }

    /// The daemon closed a gate, answered here or elsewhere, or abandoned it.
    pub(super) fn close_routing_gate(&mut self, gate_id: &str, cx: &mut Context<Self>) {
        if self
            .routing_gate
            .as_ref()
            .is_some_and(|gate| gate.gate_id() == gate_id)
        {
            self.dismiss_routing_gate(cx);
        }
    }

    /// A finished turn cannot still be waiting on a gate.
    pub(super) fn dismiss_routing_gate(&mut self, cx: &mut Context<Self>) {
        if self.routing_gate.take().is_some() {
            self.transcript_measurements.dirty = true;
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
            gate_id: gate.gate_id().to_owned(),
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
            .filter(|gate| gate.gate_id() == gate_id)
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
            && (self.input.read(cx).focus_handle.is_focused(window)
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
        if gate.submitting() {
            return true;
        }
        match gate.press(key) {
            Some(choice) => self.answer_routing_gate(choice, cx),
            None => cx.notify(),
        }
        true
    }

    /// `/acct`: steer routing for this session. Returns false for any other input.
    pub(super) fn handle_acct_command(&mut self, trimmed: &str, cx: &mut Context<Self>) -> bool {
        let action = match parse_acct(trimmed) {
            Ok(Some(action)) => action,
            Ok(None) => return false,
            Err(usage) => {
                self.items.push(Item::Error(usage));
                cx.notify();
                return true;
            }
        };
        if self.is_pending_session() {
            self.items.push(Item::Error(
                "Account routing needs a started session. Send a message first.".into(),
            ));
        } else {
            self.bridge.send(Command::RoutingIntent {
                session_id: self.session_id.clone(),
                action,
            });
        }
        cx.notify();
        true
    }

    /// Print the daemon's answer to a routing intent.
    pub(crate) fn routing_intent_reply(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        self.items.push(match result {
            Ok(text) => Item::Assistant(format!("```text\n{}\n```", text.trim_end())),
            Err(error) => Item::Error(error),
        });
        self.stick_to_bottom = true;
        self.transcript_measurements.dirty = true;
        cx.notify();
    }

    /// The account chip, or nothing before the first decision.
    pub(super) fn render_routing_chip(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let chip = self.routing_chip.as_ref()?;
        let theme = Theme::global();
        let rows = chip.tooltip();
        let marker = chip.warning_marker();
        let pending = chip.pending.clone();
        Some(
            super::composer::composer_pill("composer-routing-chip")
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
                .tooltip(move |_, cx| {
                    cx.new(|_| super::usage::RowsTooltip {
                        title: "Account routing".into(),
                        rows: rows.clone(),
                    })
                    .into()
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.handle_acct_command("/acct status", cx);
                }))
                .child(
                    div()
                        .debug_selector(|| "composer-routing-account".into())
                        .min_w_0()
                        .truncate()
                        .child(chip.account.clone()),
                )
                .children(marker.map(|marker| {
                    div()
                        .debug_selector(|| "composer-routing-warning".into())
                        .flex_none()
                        .text_color(theme.WARN)
                        .child(marker)
                }))
                .children(pending.map(|pending| {
                    div()
                        .debug_selector(|| "composer-routing-pending".into())
                        .min_w_0()
                        .truncate()
                        .text_color(theme.ACCENT)
                        .child(format!("→ {pending}"))
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
        let submitting = gate.submitting();
        let mut rows = div()
            .debug_selector(|| "routing-gate-rows".into())
            .flex()
            .flex_col()
            .gap_1();
        for (index, text) in gate.rows().into_iter().enumerate() {
            let selected = gate.cursor() == index;
            let cancel = index + 1 == gate.row_count();
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
                        .child(gate.reason().to_owned()),
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
