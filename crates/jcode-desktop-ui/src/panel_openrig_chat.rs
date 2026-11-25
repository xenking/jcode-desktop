use super::State;
use crate::{
    input::PromptInput,
    theme::{Palette, Theme},
    ui_actions::OpenFleetSession,
};
use gpui::{Context, Entity, FontWeight, ScrollHandle, SharedString, Window, div, prelude::*, px};
use jcode_desktop_openrig::{
    SectionKind,
    chat::{Access, ChatHost, ChatSeat, Recipient, Role, roster},
};

#[derive(Default)]
pub(super) struct ChatView {
    selected: Option<(String, Role)>,
    scroll: ScrollHandle,
    open_error: Option<String>,
    relay: Option<Relay>,
}

struct Relay {
    host: String,
    role: Role,
    recipient: Recipient,
    input: Entity<PromptInput>,
    phase: RelayPhase,
}

enum RelayPhase {
    Editing,
    Sending,
    Finished(Result<String, String>),
}

impl ChatView {
    pub(super) fn composer_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.relay
            .as_ref()
            .is_some_and(|relay| relay.input.read(cx).focus_handle.is_focused(window))
    }

    fn relay_sending(&self) -> bool {
        self.relay
            .as_ref()
            .is_some_and(|relay| matches!(relay.phase, RelayPhase::Sending))
    }

    fn start_relay(&mut self, relay: Relay) {
        self.relay = Some(relay);
    }

    fn dismiss_relay(&mut self) {
        self.relay = None;
    }

    fn clear_error(&mut self) {
        self.open_error = None;
    }

    fn fail_to_open(&mut self) {
        self.open_error = Some("No SSH destination could be resolved for this seat. Configure its host in Desktop's SSH machines, then try again.".into());
    }

    /// Mark the seat selected and scroll its host group into view.
    fn select(&mut self, selected: (String, Role), group: usize) {
        self.selected = Some(selected);
        self.scroll.scroll_to_item(group + 1);
    }

    fn finish_relay(&mut self, result: Result<String, String>) -> bool {
        let Some(relay) = self.relay.as_mut() else {
            return false;
        };
        relay.phase = RelayPhase::Finished(result);
        true
    }
}

impl State {
    fn visible_hosts(&self) -> impl Iterator<Item = ChatHost<'_>> {
        let filter = self.filter.to_lowercase();
        roster(&self.snapshot.fleet).filter(move |host| {
            host.name.to_lowercase().contains(&filter)
                || host.id.to_lowercase().contains(&filter)
                || host.seats.iter().any(|seat| matches_filter(seat, &filter))
        })
    }
}

fn matches_filter(seat: &ChatSeat<'_>, filter: &str) -> bool {
    seat.role.label().to_lowercase().contains(filter)
}

fn seats_matching<'a>(host: ChatHost<'a>, filter: &str) -> impl Iterator<Item = ChatSeat<'a>> {
    let filter = filter.to_lowercase();
    let all =
        host.name.to_lowercase().contains(&filter) || host.id.to_lowercase().contains(&filter);
    host.seats
        .into_iter()
        .filter(move |seat| all || matches_filter(seat, &filter))
}

fn open_action(access: &Access<'_>) -> Option<OpenFleetSession> {
    match access {
        Access::Native { target, session_id } => Some(OpenFleetSession {
            target: (*target).clone(),
            native_id: (*session_id).into(),
        }),
        Access::Unavailable(_) | Access::Relay { .. } => None,
    }
}

fn text(theme: &Palette, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w_0()
        .text_size(px(12.))
        .text_color(theme.TEXT_DIM)
        .child(label.into())
}

fn pill(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    theme: &Palette,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .rounded_full()
        .px_3()
        .py_1()
        .bg(if primary {
            theme.ACCENT_DIM
        } else {
            theme.PANEL_BG
        })
        .hover(|style| style.bg(theme.HEADER_BG))
        .cursor_pointer()
        .text_size(px(12.))
        .child(label)
}

impl State {
    pub(super) fn configure_chat_capture(&mut self) {
        if std::env::var_os("JCODE_DESKTOP_SCREENSHOT_OPENRIG_CHAT").is_some() {
            if super::live_verification() {
                self.snapshot =
                    jcode_desktop_openrig::fetch_snapshot(&super::destination_base_url());
            } else if crate::harness::controlled_runtime_socket().is_some()
                && let Ok(session) = std::env::var("JCODE_DESKTOP_CONTROL_OPENRIG_SEAT_SESSION")
                && let Some(seat) = self
                    .snapshot
                    .fleet
                    .hosts
                    .iter_mut()
                    .take(1)
                    .flat_map(|host| host.rigs.iter_mut().flatten())
                    .flat_map(|rig| rig.pods.iter_mut().flatten())
                    .flat_map(|pod| &mut pod.seats)
                    .find(|seat| seat.logical_id == "operator.agent")
            {
                seat.jcode_session_id = Some(session);
            }
            self.select_kind(SectionKind::Chat);
            self.events.clear();
        }
    }
}

impl State {
    fn compose_chat_relay(
        &mut self,
        host: String,
        role: Role,
        recipient: Recipient,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chat.relay_sending() {
            return;
        }
        let weak = cx.weak_entity();
        let input = cx.new(|cx| {
            PromptInput::new(cx, "Message to the OpenRig seat", |_, _, _, _| {})
                .without_command_completion()
                .with_on_overlay_cancel(move |app| {
                    let _ = weak.update(app, |this, cx| {
                        this.chat.dismiss_relay();
                        cx.notify();
                    });
                    true
                })
        });
        window.focus(&input.read(cx).focus_handle.clone(), cx);
        self.chat.start_relay(Relay {
            host,
            role,
            recipient,
            input,
            phase: RelayPhase::Editing,
        });
        cx.notify();
    }

    fn send_chat_relay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(relay) = &mut self.chat.relay else {
            return;
        };
        if !matches!(relay.phase, RelayPhase::Editing) {
            return;
        }
        let message = relay.input.read(cx).content.clone();
        if message.trim().is_empty() {
            return;
        }
        if self.destination_fixture || crate::harness::screenshot_mode() {
            relay.phase =
                RelayPhase::Finished(Err("This isolated preview does not send messages.".into()));
            cx.notify();
            return;
        }
        let recipient = relay.recipient.clone();
        relay.phase = RelayPhase::Sending;
        window.focus(&self.focus, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { recipient.send(&message) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.chat.finish_relay(result) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn show_chat_open_error(&mut self, cx: &mut Context<Self>) {
        self.chat.fail_to_open();
        cx.notify();
    }

    pub(super) fn chat_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(key, "j" | "k" | "up" | "down" | "enter" | "right") {
            return false;
        }
        let rows: Vec<_> = self
            .visible_hosts()
            .enumerate()
            .flat_map(|(index, host)| {
                let host_id = host.id;
                seats_matching(host, &self.filter).map(move |seat| (index, host_id, seat))
            })
            .collect();
        let selected = rows.iter().position(|(_, id, seat)| {
            self.chat
                .selected
                .as_ref()
                .is_some_and(|(host, role)| host == id && *role == seat.role)
        });
        let next = match key {
            "k" | "up" => selected.map_or(0, |index| index.saturating_sub(1)),
            "j" | "down" => selected
                .map_or(0, |index| index + 1)
                .min(rows.len().saturating_sub(1)),
            _ => selected.unwrap_or(0),
        };
        if let Some((group, host, seat)) = rows.get(next) {
            let group = *group;
            let selected = ((*host).to_owned(), seat.role);
            let activate = matches!(key, "enter" | "right");
            let action = activate.then(|| open_action(&seat.access)).flatten();
            let relay = match &seat.access {
                Access::Relay { recipient, .. } if activate && self.chat.relay.is_none() => {
                    Some(recipient.clone())
                }
                _ => None,
            };
            self.chat.select(selected.clone(), group);
            if let Some(action) = action {
                self.chat.clear_error();
                window.dispatch_action(Box::new(action), cx);
            } else if let Some(recipient) = relay {
                self.compose_chat_relay(selected.0, selected.1, recipient, window, cx);
            }
        }
        true
    }

    pub(super) fn render_chat(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let mut hosts = self.visible_hosts().peekable();
        let empty = hosts.peek().is_none();
        div()
            .id("openrig-chat")
            .debug_selector(|| "openrig-chat".into())
            .min_w_0()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.chat.scroll)
            .p_3()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Chat"))
                    .child(text(
                        theme,
                        "Open the Advisor or Operator session for a host.",
                    ))
                    .when_some(self.chat.open_error.clone(), |header, error| {
                        header.child(text(theme, error).text_color(theme.ERROR))
                    }),
            )
            .when(empty, |list| {
                list.child(text(
                    theme,
                    if self.filter.is_empty() {
                        "No hosts loaded. Check the OpenRig connection and try again."
                    } else {
                        "No matching seats. Clear the filter to see all hosts."
                    },
                ))
            })
            .children(hosts.map(|host| {
                let host_id = host.id;
                div()
                    .id(SharedString::from(format!("openrig-chat-host:{host_id}")))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(host.name.to_owned()),
                    )
                    .children(
                        seats_matching(host, &self.filter)
                            .map(|seat| self.render_chat_seat(host_id, seat, theme, cx)),
                    )
            }))
            .into_any_element()
    }

    fn render_chat_seat(
        &self,
        host: &str,
        seat: ChatSeat<'_>,
        theme: &Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let role = seat.role.label();
        let selector = format!("openrig-chat:{host}:{role}");
        let selected = self
            .chat
            .selected
            .as_ref()
            .is_some_and(|(id, selected_role)| id == host && *selected_role == seat.role);
        let status = seat.status.unwrap_or("unknown");
        let activity = seat
            .last_activity_at
            .map(|time| time.format("%b %d, %H:%M UTC").to_string());
        let mut row = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector({
                let selector = selector.clone();
                move || selector
            })
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x_3()
                    .gap_y_1()
                    .rounded_full()
                    .bg(if selected {
                        theme.ACCENT_DIM
                    } else {
                        theme.PANEL_BG
                    })
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(role),
                    )
                    .child(
                        text(theme, status.replace('_', " ")).text_color(match status {
                            "running" => theme.OK,
                            "attention_required" => theme.WARN,
                            "failed" => theme.ERROR,
                            _ => theme.TEXT_DIM,
                        }),
                    )
                    .when_some(seat.runtime, |pill, runtime| {
                        pill.child(text(theme, runtime.label()))
                    })
                    .child(text(
                        theme,
                        activity.map_or_else(
                            || "Last activity unknown".into(),
                            |time| format!("Last active {time}"),
                        ),
                    )),
            );
        if let Some(relay) = self
            .chat
            .relay
            .as_ref()
            .filter(|relay| relay.host == host && relay.role == seat.role)
        {
            let still_current = matches!(&seat.access, Access::Relay { recipient, .. } if relay.recipient == *recipient);
            row = row.child(self.render_chat_relay(relay, still_current, theme, cx));
        } else if let Some(action) = open_action(&seat.access) {
            let button = format!("{selector}:open");
            row = row.child(
                div().pl_3().flex().child(
                    div()
                        .id(SharedString::from(button.clone()))
                        .debug_selector({
                            let button = button.clone();
                            move || button
                        })
                        .relative()
                        .children(crate::fixture_control_targets::marker(
                            || button,
                            || Some(self.session_id.clone()),
                        ))
                        .rounded_full()
                        .px_3()
                        .py_1()
                        .bg(theme.ACCENT_DIM)
                        .hover(|style| style.bg(theme.HEADER_BG))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .child("Open Jcode chat")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.chat.clear_error();
                            window.dispatch_action(Box::new(action.clone()), cx);
                        })),
                ),
            );
        } else if let Access::Relay { reason, recipient } = seat.access {
            let compose = format!("{selector}:relay");
            if self.chat.relay.is_none() {
                let host = host.to_owned();
                let role = seat.role;
                row = row.child(
                    div()
                        .pl_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(text(
                            theme,
                            match reason {
                                jcode_desktop_openrig::chat::Unavailable::MissingNativeRef => {
                                    "OpenRig has not reported a Jcode session ID."
                                }
                                _ => "This runtime has no native Jcode chat.",
                            },
                        ))
                        .child(
                            div().flex().child(
                                pill(
                                    SharedString::from(compose.clone()),
                                    "Write via OpenRig",
                                    theme,
                                    false,
                                )
                                .debug_selector(move || compose)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.compose_chat_relay(
                                            host.clone(),
                                            role,
                                            recipient.clone(),
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                            ),
                        ),
                );
            }
        } else if let Access::Unavailable(reason) = seat.access {
            let reason_selector = format!("{selector}:unavailable:{reason:?}");
            row = row.child(
                div()
                    .id(SharedString::from(reason_selector.clone()))
                    .debug_selector(move || reason_selector)
                    .px_3()
                    .child(text(theme, reason.explanation())),
            );
        }
        row.into_any_element()
    }

    fn render_chat_relay(
        &self,
        relay: &Relay,
        still_current: bool,
        theme: &Palette,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let body = div().px_3().flex().flex_col().gap_2();
        if matches!(relay.phase, RelayPhase::Sending) {
            return body.child(text(theme, "Sending through OpenRig…"));
        }
        let dismiss = pill(
            "openrig-chat-dismiss",
            if matches!(relay.phase, RelayPhase::Editing) {
                "Cancel"
            } else {
                "Done"
            },
            theme,
            false,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.chat.dismiss_relay();
            window.focus(&this.focus, cx);
            cx.notify();
        }));
        match &relay.phase {
            RelayPhase::Editing => body
                .capture_action(
                    cx.listener(|_, _: &crate::input::Submit, _, cx| cx.stop_propagation()),
                )
                .child(text(
                    theme,
                    if still_current {
                        "Send one message via OpenRig. This is not a transcript."
                    } else {
                        "This seat changed. Cancel and reopen the latest seat."
                    },
                ))
                .when(still_current, |body| body.child(relay.input.clone()))
                .child(div().flex().flex_wrap().gap_2().child(dismiss).when(
                    still_current,
                    |buttons| {
                        buttons.child(
                            pill("openrig-chat-send", "Send via OpenRig", theme, true).on_click(
                                cx.listener(|this, _, window, cx| this.send_chat_relay(window, cx)),
                            ),
                        )
                    },
                )),
            RelayPhase::Finished(result) => {
                let (summary, color) = match result {
                    Ok(summary) => (summary, theme.TEXT_DIM),
                    Err(summary) => (summary, theme.WARN),
                };
                body.child(text(theme, summary.clone()).text_color(color))
                    .child(div().flex().child(dismiss))
            }
            RelayPhase::Sending => body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::Panel;
    use jcode_desktop_openrig::fleet::{Fleet, Runtime};

    #[gpui::test]
    fn chat_narrow_split_shows_each_unavailable_reason_and_no_native_button(
        cx: &mut gpui::TestAppContext,
    ) {
        use jcode_desktop_openrig::chat::Unavailable::*;

        cx.update(crate::bind_workspace_keys);
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new_openrig(crate::harness::spawn_recording().0, cx));
        for reason in [
            Inventory,
            MissingSeat,
            AmbiguousSeat,
            Stopped,
            UnknownState,
            MissingNativeRef,
            UnsupportedRuntime,
            UnknownRuntime,
        ] {
            panel.update_in(vcx, |panel, _, cx| {
                let pane = panel.openrig.clone().unwrap();
                pane.update(cx, |state, cx| {
                    state.snapshot.fleet = Fleet::fixture();
                    let host = &mut state.snapshot.fleet.hosts[0];
                    let rigs = host.rigs.as_mut().unwrap();
                    let seats = rigs[0]
                        .pods
                        .as_mut()
                        .unwrap()
                        .iter_mut()
                        .flat_map(|pod| &mut pod.seats);
                    let seat = seats
                        .into_iter()
                        .find(|seat| seat.logical_id == "advisor.lead")
                        .unwrap();
                    match reason {
                        Inventory => rigs[0].pods = None,
                        MissingSeat => seat.logical_id = "not-advisor".into(),
                        AmbiguousSeat => rigs.push(rigs[0].clone()),
                        Stopped => seat.session_status = Some("stopped".into()),
                        UnknownState => seat.session_status = None,
                        MissingNativeRef => (),
                        UnsupportedRuntime => seat.runtime = Some(Runtime::Other("codex".into())),
                        UnknownRuntime => seat.runtime = None,
                    }
                    state.select_kind(SectionKind::Chat);
                    cx.notify();
                });
                panel.set_openrig_width(px(420.), cx);
            });
            vcx.run_until_parked();
            let selector: &'static str = Box::leak(
                format!("openrig-chat:host-fixture-local:Advisor:unavailable:{reason:?}")
                    .into_boxed_str(),
            );
            assert!(vcx.debug_bounds(selector).is_some(), "missing {reason:?}");
            assert!(
                vcx.debug_bounds("openrig-chat:host-fixture-local:Advisor:open")
                    .is_none()
            );
        }
    }
}
