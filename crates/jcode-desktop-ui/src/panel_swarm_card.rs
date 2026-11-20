//! Compact, expandable transcript rows for swarm calls.
use super::*;
use crate::swarm_view::friendly_name;
use jcode_desktop_harness::swarm_usage::WorkerUsage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Call {
    pub action: String,
    pub session_ids: Vec<String>,
    target: String,
    text: String,
}

fn short_name(id: &str) -> &str {
    friendly_name(id).unwrap_or_else(|| {
        if id.starts_with("session_") {
            "worker"
        } else {
            id
        }
    })
}

fn friendly_preview(text: &str) -> String {
    let mut rest = text;
    let mut result = String::new();
    while let Some(start) = rest.find("session_") {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        result.push_str(short_name(&rest[..end]));
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}

fn row_preview(text: &str) -> String {
    friendly_preview(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn parse(name: &str, input: &str, output: &str) -> Option<Call> {
    if name.trim_start_matches("functions.") != "swarm" {
        return None;
    }
    let input: serde_json::Value = serde_json::from_str(input).unwrap_or_default();
    let action = input
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("swarm")
        .to_owned();
    let get = |key: &str| {
        input
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let mut ids = Vec::new();
    for id in input
        .get("session_ids")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .chain(
            [get("target_session"), get("to_session"), get("session_id")]
                .into_iter()
                .flatten(),
        )
    {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_owned());
        }
    }
    if action == "spawn" {
        for word in output.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
            if friendly_name(word).is_some() && !ids.iter().any(|id| id == word) {
                ids.push(word.to_owned());
            }
        }
    }
    let target = if action == "spawn" {
        let label = get("label")
            .map(str::to_owned)
            .or_else(|| ids.first().map(|s| short_name(s).to_owned()))
            .unwrap_or_default();
        if let Some(id) = ids.first() {
            format!("{} {label}", jcode_core::id::session_icon(short_name(id)))
        } else {
            label
        }
    } else if !ids.is_empty() {
        ids.iter()
            .map(|s| {
                let name = short_name(s);
                format!("{} {}", jcode_core::id::session_icon(name), name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        get("channel")
            .or_else(|| get("to_swarm"))
            .unwrap_or("all")
            .to_owned()
    };
    let text = get("message")
        .or_else(|| get("prompt"))
        .unwrap_or("")
        .to_owned();
    Some(Call {
        action,
        session_ids: ids,
        target,
        text,
    })
}

fn action_icon(action: &str) -> &'static [u8] {
    match action {
        "message" | "dm" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 4h12v8H2z M2 4l6 5 6-5" fill="none" stroke="currentColor" stroke-width="1.4"/></svg>"#,
        "spawn" | "resume" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 2v12M2 8h12" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>"#,
        "stop" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="3" y="3" width="10" height="10" rx="1" fill="currentColor"/></svg>"#,
        "await_members" => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3 2h10M3 14h10M4 2c0 4 4 4 4 6s-4 2-4 6M12 2c0 4-4 4-4 6s4 2 4 6" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>"#,
        _ => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 4h2M6 4h8M2 8h2M6 8h8M2 12h2M6 12h8" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>"#,
    }
}

impl Panel {
    pub(crate) fn set_swarm_usage(
        &mut self,
        usage: HashMap<String, WorkerUsage>,
        cx: &mut Context<Self>,
    ) {
        if self.swarm_usage != usage {
            self.swarm_usage = usage;
            self.transcript_measurements.dirty = true;
            cx.notify();
        }
    }
    pub(crate) fn swarm_card_worker_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for item in self.items.iter() {
            if let Item::Tool {
                name,
                input,
                output,
                ..
            } = item
                && let Some(call) = parse(name, input, output)
            {
                for id in call.session_ids {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        ids
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_swarm_card(
        &self,
        index: usize,
        call_id: &str,
        call: &Call,
        name: &str,
        input: &str,
        done: bool,
        output: &str,
        error: Option<&str>,
        offset: f32,
        opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let expanded = self.expanded_regions.contains(call_id);
        let key = call_id.to_owned();
        let key_for_enter = key.clone();
        let focus = self
            .swarm_row_focus
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let click_focus = focus.clone();
        let preview = row_preview(&call.text);
        let mut body = tool_detail(name, input, output);
        if let Some(error) = error {
            body.push_str("\n\nError\n");
            body.push_str(&strip_ansi(error));
        }
        let (token_label, token_color) = tool_output_token_badge(output);
        let height = if self.expandable_viewport_height > px(0.) {
            (f32::from(self.expandable_viewport_height) * 0.6).clamp(1., expandable::MAX_HEIGHT)
        } else {
            expandable::MAX_HEIGHT
        };
        div()
            .id(("tool", index))
            .debug_selector(|| "tool-swarm".into())
            .flex_none()
            .flex()
            .flex_col()
            .ml(px(offset))
            .opacity(opacity)
            .child(
                div()
                    .id(("swarm-card-row", index))
                    .debug_selector(|| "tool-swarm-header".into())
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w_0()
                    .px_1()
                    .py(px(2.0))
                    .cursor_pointer()
                    .track_focus(&focus)
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" && !event.is_held {
                            expandable::flip(&mut this.expanded_regions, &key_for_enter);
                            this.transcript_measurements.dirty = true;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&click_focus, cx);
                        expandable::flip(&mut this.expanded_regions, &key);
                        this.transcript_measurements.dirty = true;
                        cx.notify();
                    }))
                    .child(crate::tool_icon::render_badge(
                        "swarm",
                        done,
                        error.is_some(),
                    ))
                    .child(
                        gpui::svg()
                            .data(action_icon(&call.action))
                            .size(px(14.0))
                            .text_color(theme.TEXT_FAINT),
                    )
                    .child(
                        div()
                            .max_w(px(260.0))
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(theme.TEXT_DIM)
                            .child(call.target.clone()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "tool-swarm-preview".into())
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(theme.TOOL_TEXT)
                            .child(if preview.is_empty() {
                                call.action.clone()
                            } else {
                                format!("· {preview}")
                            }),
                    )
                    .children(self.render_tool_clock(index, call_id, done || error.is_some(), cx))
                    .child(
                        div()
                            .flex_none()
                            .ml_auto()
                            .px_2()
                            .py(px(2.))
                            .rounded_full()
                            .bg(theme.INLINE_CODE_BG)
                            .text_size(px(10.))
                            .text_color(token_color)
                            .child(token_label),
                    ),
            )
            .when(expanded, |el| {
                let scroll = self
                    .swarm_scrolls
                    .borrow_mut()
                    .entry(call_id.to_owned())
                    .or_default()
                    .clone();
                el.child(
                    div()
                        .debug_selector(|| "tool-swarm-result-body".into())
                        .ml(px(24.0))
                        .relative()
                        .child(
                            div()
                                .id(format!("swarm-body-{index}"))
                                .debug_selector(move || format!("swarm-body-{index}"))
                                .w_full()
                                .min_w_0()
                                .max_h(px(height))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .mb_2()
                                                .text_color(theme.TEXT_FAINT)
                                                .child(self.tool_timing_detail(call_id)),
                                        )
                                        .text_size(px(12.0))
                                        .text_color(theme.TOOL_TEXT)
                                        .child(text_selection::plain(
                                            self.transcript_selection.clone(),
                                            format!("tool-detail-{index}"),
                                            body,
                                            window,
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .debug_selector(|| "swarm-body-end".into())
                                                .child(""),
                                        ),
                                )
                                .map(|body| crate::transcript_scroll::tracked_vertical(body, &scroll)),
                        )
                        .child(crate::scrollbar::vertical_with_track(
                            &scroll,
                            "swarm-scrollbar",
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stored_swarm_result_restores_compact_row() {
        let history: Vec<jcode_sdk::HistoryMessage> = serde_json::from_value(serde_json::json!([
            {"role":"tool","content":"Direct message sent", "tool": {
                "id":"call-1", "name":"functions.swarm",
                "input":{"action":"message","to_session":"session_fox_123","message":"hello"}
            }}
        ]))
        .unwrap();
        let (items, _) = super::Panel::history_items(history, vec![]);
        assert!(items.iter().any(
            |item| matches!(item, super::Item::Tool { name, input, output, .. }
            if super::parse(name, input, output).is_some())
        ));
    }
    use super::*;
    const FOX: &str = "session_fox_1794718668160_87674d9271df588d";

    #[gpui::test]
    fn preview_uses_available_width_after_resize(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "width-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        let text = "a long message with enough words to fill the transcript ".repeat(20);
        assert_eq!(row_preview(&text), text.trim_end());
        assert!(row_preview(&text).len() > 90);
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::Tool {
                call_id: "wide".into(),
                name: "swarm".into(),
                input: serde_json::json!({"action":"spawn", "label":"worker", "prompt":text})
                    .to_string(),
                output: "ok".into(),
                done: true,
                error: None,
            });
            cx.notify();
        });
        let window = vcx.window_handle();
        vcx.simulate_window_resize(window, gpui::size(px(1400.), px(800.)));
        vcx.run_until_parked();
        let row = vcx.debug_bounds("tool-swarm-header").unwrap();
        let preview = vcx.debug_bounds("tool-swarm-preview").unwrap();
        assert!(
            preview.size.width > (row.right() - preview.left()) * 0.8,
            "{preview:?} in {row:?}"
        );
        vcx.simulate_window_resize(window, gpui::size(px(800.), px(800.)));
        vcx.run_until_parked();
        let narrower = vcx.debug_bounds("tool-swarm-preview").unwrap();
        assert!(narrower.size.width < preview.size.width);
    }

    #[test]
    fn compact_call_uses_message_and_friendly_target() {
        let call = parse("swarm", &format!(r#"{{"action":"message","to_session":"{FOX}","message":"Hello worker\nSecond line","intent":"hidden"}}"#), "").unwrap();
        assert_eq!(call.target, "🦊 fox");
        assert_eq!(call.text, "Hello worker\nSecond line");
        assert_eq!(
            friendly_preview(&format!("see {FOX}, please")),
            "see fox, please"
        );
        let spawn = parse(
            "swarm",
            r#"{"action":"spawn","label":"parser","prompt":"do work"}"#,
            "",
        )
        .unwrap();
        assert_eq!(
            (spawn.target.as_str(), spawn.text.as_str()),
            ("parser", "do work")
        );
    }

    #[gpui::test]
    fn swarm_message_is_one_row_and_expands_on_click(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("lead", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::Tool { call_id: "compact-dm".into(), name: "swarm".into(),
                input: format!(r#"{{"action":"message","to_session":"{FOX}","message":"Hello worker\nSecond line","intent":"hidden intent"}}"#),
                output: format!("Direct message sent to {FOX}: Hello worker\nSecond line"), done: true, error: None });
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-card-worker-0").is_none());
        assert!(vcx.debug_bounds("tool-swarm-result-body").is_none());
        let row = vcx.debug_bounds("tool-swarm-header").unwrap();
        vcx.simulate_click(row.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-swarm-result-body").is_some());
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-swarm-result-body").is_none());
    }

    #[gpui::test]
    fn long_swarm_message_scrolls_to_last_line(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("lead", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        let message = format!(
            "{}last line of the full message",
            "long message line\n".repeat(240)
        );
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::Tool {
                call_id: "long-dm".into(),
                name: "swarm".into(),
                input: serde_json::json!({"action": "message", "message": message}).to_string(),
                output: String::new(),
                done: true,
                error: None,
            });
            cx.notify();
        });
        vcx.run_until_parked();
        let row = vcx.debug_bounds("tool-swarm-header").unwrap();
        vcx.simulate_click(row.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let viewport = vcx.debug_bounds("swarm-body-0").unwrap();
        for _ in 0..100 {
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100.))),
                modifiers: Default::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
        }
        let end = vcx
            .debug_bounds("swarm-body-end")
            .expect("full message end rendered");
        assert!(
            end.bottom() <= viewport.bottom() && end.top() >= viewport.top(),
            "last line must be reachable"
        );
    }
}
