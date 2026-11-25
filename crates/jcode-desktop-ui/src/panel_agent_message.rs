use super::*;
use super::classify::{EXTERNAL_DM, is_external};


fn split_dm(message: &str) -> Option<(&str, &str)> {
    let (sender, body) = if let Some(envelope) = message.strip_prefix(EXTERNAL_DM) {
        envelope
            .split_once(" (swarm '")
            .and_then(|(sender, rest)| Some((sender, rest.split_once("'): ")?.1)))
            .or_else(|| envelope.split_once(": "))?
    } else {
        message.strip_prefix("DM from ")?.split_once(": ")?
    };
    (!sender.trim().is_empty() && !sender.contains(['\r', '\n'])).then_some((sender, body))
}

pub(super) fn dm_body(message: &str) -> &str {
    split_dm(message).map_or(message, |(_, body)| body)
}

pub(super) fn history_user_item(
    content: String,
    has_attachments: bool,
    timestamp_ms: Option<i64>,
) -> Item {
    if !has_attachments && let Some((sender, _)) = split_dm(&content) {
        return Item::AgentMessage {
            // Old history stores a display name, not a signed session ID.
            from_session: String::new(),
            from_name: Some(sender.to_owned()),
            message: content,
            timestamp_ms,
        };
    }
    Item::User(content)
}

pub(super) fn screenshot_fixture() -> Vec<Item> {
    vec![
        Item::User("Coordinate the Desktop review and report when it is ready.".into()),
        history_user_item("DM from hog: The unit tests passed.".into(), false, None),
        history_user_item(
            "Cross-swarm DM from turtle (swarm 'Desktop: QA'): Please check both transcript modes before shipping.\n\nOpen a narrow panel and confirm the incoming request is readable without a click. Then send me the verification results.".into(),
            false,
            None,
        ),
        Item::Tool {
            call_id: "fixture-external-dm".into(),
            name: "swarm".into(),
            input: serde_json::json!({"action":"dm", "to_session":"turtle", "message":"I’ll check both modes and send the results."}).to_string(),
            output: "External DM sent to turtle: I’ll check both modes and send the results.".into(),
            done: true,
            error: None,
        },
        Item::Assistant("I’ll start by checking clean chat in a narrow panel.".into()),
    ]
}

impl Panel {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_agent_message(
        &self,
        index: usize,
        from_session: &str,
        from_name: Option<&str>,
        message: &str,
        timestamp_ms: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let icon_name = crate::swarm_view::friendly_name(from_session);
        let sender = from_name
            .filter(|name| !name.is_empty())
            .or(icon_name)
            .map_or_else(|| short_id(from_session), str::to_owned);
        let target = if !from_session.is_empty() {
            Some(from_session.to_owned())
        } else {
            let mut matches = self
                .swarm_workers
                .iter()
                .filter(|worker| worker.name == sender || worker.session_id == sender);
            matches
                .next()
                .filter(|_| matches.next().is_none())
                .map(|worker| worker.session_id.clone())
        };
        let body = dm_body(message);
        let first_line = body.lines().next().unwrap_or_default().trim();
        let key = expandable::key::details(index);
        let external = is_external(message);
        let expanded = self.expanded_regions.contains(&key) ^ external;
        let focus = self
            .swarm_row_focus
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let click_focus = focus.clone();
        let keyboard_key = key.clone();
        let summary = if !expanded && body.lines().skip(1).any(|line| !line.trim().is_empty()) {
            format!("{first_line}…")
        } else {
            first_line.to_owned()
        };
        let peer = swarm_card::row_peer(format!(
            "{}{} {sender}",
            if external { "From " } else { "" },
            jcode_core::id::session_icon(icon_name.unwrap_or(&sender))
        ))
        .id(("agent-message-sender", index))
        .debug_selector(|| "agent-message-sender".into())
        .relative();
        let peer = if let Some(target) = target {
            crate::keyboard::button(peer)
                .hover(|el| el.text_color(theme.ACCENT))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(OpenSwarmWorker(target.clone()));
                }))
                .children(crate::fixture_control_targets::marker(
                    || format!("agent-message:sender:{index}"),
                    || Some(self.session_id.clone()),
                ))
        } else {
            peer
        };
        let row = swarm_card::Row {
            action: "dm",
            direction: Some(swarm_card::Direction::From),
            external,
            peer: peer.into_any_element(),
            message: if external && expanded { "" } else { &summary },
            clock: timestamp_ms.and_then(swarm_card::render_swarm_timestamp),
            tokens: None,
            via_codemode: false,
            done: true,
            failed: false,
        };
        div()
            .debug_selector(|| "agent-message".into())
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .child(
                row.render(
                    crate::keyboard::tracked_button(
                        div().id(("agent-message-summary", index)),
                        &focus,
                    )
                    .debug_selector(|| "agent-message-summary".into())
                    .aria_expanded(expanded)
                    .relative()
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" && !event.is_held {
                            this.toggle_line(&keyboard_key, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&click_focus, cx);
                        cx.stop_propagation();
                        this.toggle_line(&key, cx);
                    }))
                    .children(crate::fixture_control_targets::marker(
                        || format!("agent-message:summary:{index}"),
                        || Some(self.session_id.clone()),
                    )),
                    Some(expanded),
                ),
            )
            .when(expanded, |row| {
                row.child(
                    div()
                        .debug_selector(|| "agent-message-body".into())
                        .font_family(theme.FONT_AI)
                        .ml(px(24.0))
                        .min_w_0()
                        .text_color(theme.TEXT)
                        .child(markdown::render_interactive(
                            body,
                            index,
                            &self.transcript_selection,
                            window,
                            cx,
                            false,
                            self.media_preview_handler(cx),
                        )),
                )
            })
            .into_any_element()
    }

    pub(super) fn open_agent_message(&mut self, index: usize) {
        let Some(Item::AgentMessage { message, .. }) = self.items.get(index) else {
            return;
        };
        let key = expandable::key::details(index);
        let changed = if is_external(message) {
            self.expanded_regions.remove(&key)
        } else {
            self.expanded_regions.insert(key)
        };
        if changed {
            self.transcript_measurements.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn external_dm_incoming_renders_full_selectable_body_and_collapses(
        cx: &mut gpui::TestAppContext,
    ) {
        const MESSAGE: &str = "Cross-swarm DM from turtle (swarm 'Desktop: QA'): Please check the first item.\n\nPlease check the second item too.";
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "daisy".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel.apply(
                &ApiEvent::AgentMessage {
                    session_id: "daisy".into(),
                    from_session: "turtle".into(),
                    from_name: Some("turtle".into()),
                    message: MESSAGE.into(),
                },
                cx,
            );
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("external-message-marker").is_some());
        assert!(vcx.debug_bounds("swarm-action").is_none());
        let body = vcx
            .debug_bounds("agent-message-body")
            .expect("external request opens by default");
        let summary = vcx.debug_bounds("agent-message-summary").unwrap();
        assert!(
            body.size.height > summary.size.height,
            "both paragraphs should be visible"
        );
        assert_eq!(
            dm_body(MESSAGE),
            "Please check the first item.\n\nPlease check the second item too."
        );
        vcx.simulate_click(summary.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("agent-message-body").is_none());
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("agent-message-body").is_some());
        panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(false), cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("external-message-marker").is_some());
        assert!(vcx.debug_bounds("agent-message-body").is_some());
    }

    #[gpui::test]
    fn external_dm_outgoing_has_a_distinct_marker_in_both_modes(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "daisy".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel.items = screenshot_fixture()
                .into_iter()
                .filter(|item| matches!(item, Item::Tool { .. }))
                .collect::<Vec<_>>()
                .into();
            panel
        });
        for clean in [true, false] {
            panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(clean), cx));
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("external-message-marker").is_some());
            assert!(vcx.debug_bounds("tool-swarm-header").is_some());
            assert!(vcx.debug_bounds("swarm-action").is_none());
            assert!(vcx.debug_bounds("agent-message-body").is_none());
        }
    }

    #[gpui::test]
    fn own_swarm_dm_stays_compact_and_is_hidden_in_clean_chat(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "daisy".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(false), cx);
            panel.items.push(history_user_item(
                "DM from hog: First line\n\nSecond line".into(),
                false,
                None,
            ));
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-action").is_some());
        assert!(vcx.debug_bounds("external-message-marker").is_none());
        assert!(vcx.debug_bounds("agent-message-body").is_none());
        panel.update(vcx, |panel, cx| panel.set_clean_chat(Some(true), cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("agent-message-summary").is_none());
    }
}
