use super::*;

pub(super) fn split_dm(message: &str) -> Option<(&str, &str)> {
    let envelope = message
        .strip_prefix("DM from ")
        .or_else(|| message.strip_prefix("Cross-swarm DM from "))?;
    let (sender, body) = envelope.split_once(": ")?;
    (!sender.trim().is_empty() && !sender.contains(['\r', '\n'])).then_some((sender, body))
}

pub(super) fn dm_body(message: &str) -> &str {
    split_dm(message).map_or(message, |(_, body)| body)
}

pub(super) fn history_user_item(content: String, has_attachments: bool) -> Item {
    if !has_attachments && let Some((sender, _)) = split_dm(&content) {
        return Item::AgentMessage {
            // Old history stores a display name, not a signed session ID.
            from_session: String::new(),
            from_name: Some(sender.to_owned()),
            message: content,
        };
    }
    Item::User(content)
}

impl Panel {
    pub(super) fn render_agent_message(
        &self,
        index: usize,
        from_session: &str,
        from_name: Option<&str>,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let sender = from_name
            .filter(|name| !name.is_empty())
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
        let expanded = self.expanded_regions.contains(&key);
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
        div()
            .debug_selector(|| "agent-message".into())
            .flex()
            .flex_col()
            .flex_none()
            .min_w_0()
            .child(
                crate::keyboard::tracked_button(div().id(("agent-message-summary", index)), &focus)
                    .debug_selector(|| "agent-message-summary".into())
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w_0()
                    .px_1()
                    .py(px(2.0))
                    .line_height(px(18.0))
                    .text_size(px(12.0))
                    .cursor_pointer()
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
                    ))
                    .child(
                        gpui::svg()
                            .data(swarm_card::action_icon("dm"))
                            .size(px(14.0))
                            .flex_none()
                            .text_color(theme.TEXT_FAINT),
                    )
                    .child({
                        let name = div()
                            .id(("agent-message-sender", index))
                            .debug_selector(|| "agent-message-sender".into())
                            .relative()
                            .min_w_0()
                            .max_w(px(160.0))
                            .truncate()
                            .text_color(theme.TEXT_DIM)
                            .font_weight(FontWeight::MEDIUM)
                            .child(sender);
                        if let Some(target) = target {
                            crate::keyboard::button(name)
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
                            name
                        }
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.TOOL_TEXT)
                            .child(summary),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(if expanded { "▾" } else { "▸" }),
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
        if matches!(self.items.get(index), Some(Item::AgentMessage { .. }))
            && self
                .expanded_regions
                .insert(expandable::key::details(index))
        {
            self.transcript_measurements.dirty = true;
        }
    }
}
