use super::*;

pub(super) fn conversation_item(item: &Item) -> bool {
    match item {
        Item::User(_) => prompt::is_pinnable_prompt(item),
        // History marks some final rows with response stats. Live text and
        // older history have no final marker, and progress is public too.
        Item::Assistant(text) => !text.trim().is_empty(),
        Item::Image(image) => match &image.source {
            jcode_sdk::RenderedImageSource::UserInput => true,
            jcode_sdk::RenderedImageSource::Other { role } => role == "assistant",
            jcode_sdk::RenderedImageSource::ToolResult { .. } => false,
        },
        Item::AgentMessage { .. }
        | Item::Reasoning(_)
        | Item::Tool { .. }
        | Item::BackgroundTask { .. }
        | Item::Todos(_)
        | Item::ResponseStats(_)
        | Item::Error(_)
        | Item::RuntimeError { .. }
        | Item::Stopped(_)
        | Item::ProviderGuardrailRetry { .. }
        | Item::Compaction(_)
        | Item::Memory(_)
        | Item::CacheMiss(_)
        | Item::Applet(_) => false,
    }
}

pub(super) fn active_error(items: &[Item]) -> Option<usize> {
    for (index, item) in items.iter().enumerate().rev() {
        match item {
            Item::Error(_) | Item::RuntimeError { .. } => return Some(index),
            Item::Stopped(notice) => {
                return (notice.shows_error() || notice.provisional).then_some(index);
            }
            Item::User(_) | Item::Assistant(_) if conversation_item(item) => break,
            _ => {}
        }
    }
    None
}

pub(super) fn remap_anchor(
    old: &[TranscriptRenderRow],
    new: &[TranscriptRenderRow],
    offset: gpui::ListOffset,
) -> Option<gpui::ListOffset> {
    let positions: HashMap<usize, usize> = new
        .iter()
        .enumerate()
        .map(|(position, row)| (row.index, position))
        .collect();
    let start = offset.item_ix.min(old.len());
    let matched = (start..old.len())
        .chain((0..start).rev())
        .find_map(|index| Some((index, *positions.get(&old.get(index)?.index)?)))?;
    Some(gpui::ListOffset {
        item_ix: matched.1,
        offset_in_item: if matched.0 == offset.item_ix {
            offset.offset_in_item
        } else {
            px(0.)
        },
    })
}

pub(super) struct PendingApproval {
    pub tool_name: String,
    pub description: String,
}

struct Tooltip(&'static str);

impl gpui::Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .rounded_md()
            .px_2()
            .py_1()
            .bg(Theme::global().TOOL_BG)
            .text_color(Theme::global().TEXT)
            .text_size(px(11.))
            .child(self.0)
    }
}

impl Panel {
    pub(super) fn clean_chat_with_display(&self, display: &crate::config::DisplayConfig) -> bool {
        self.clean_chat_override
            .unwrap_or(display.clean_chat_default)
    }

    pub(super) fn clean_chat_enabled(&self) -> bool {
        self.clean_chat_with_display(&crate::config::display())
    }

    pub(super) fn shows_transcript_activity(&self) -> bool {
        !self.clean_chat_enabled() && self.activity_active() && self.ask_card.is_none()
    }

    pub(super) fn set_clean_chat(&mut self, override_mode: Option<bool>, cx: &mut Context<Self>) {
        self.clean_chat_override = override_mode;
        cx.notify();
    }

    pub(super) fn toggle_clean_chat(&mut self, cx: &mut Context<Self>) {
        self.set_clean_chat(Some(!self.clean_chat_enabled()), cx);
    }

    pub(super) fn render_clean_chat_toggle(
        &self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let tooltip = if enabled {
            "Show full transcript (Ctrl+Shift+J)"
        } else {
            "Show only the conversation (Ctrl+Shift+J)"
        };
        div()
            .flex_none()
            .flex()
            .justify_end()
            .px_3()
            .pt_2()
            .child(
                crate::keyboard::button(
                    div()
                        .id("clean-chat-toggle")
                        .debug_selector(|| "clean-chat-toggle".into())
                        .relative()
                        .rounded_full()
                        .px_2()
                        .py_0p5()
                        .text_size(px(11.))
                        .bg(if enabled {
                            theme.ACCENT_DIM
                        } else {
                            theme.HEADER_BG
                        })
                        .text_color(if enabled {
                            theme.ACCENT
                        } else {
                            theme.TEXT_DIM
                        })
                        .hover(|el| el.text_color(theme.TEXT))
                        .cursor_pointer()
                        .child(if enabled {
                            "Clean chat · On"
                        } else {
                            "Clean chat"
                        }),
                )
                .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(|panel, _, _, cx| {
                    panel.toggle_clean_chat(cx);
                    cx.stop_propagation();
                }))
                .tooltip(move |_, cx| cx.new(|_| Tooltip(tooltip)).into()),
            )
            .into_any_element()
    }

    pub(super) fn render_chat_blockers(
        &self,
        error: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let approval = self.pending_approval.as_ref();
        if self.transcript_only || (approval.is_none() && error.is_none()) {
            return None;
        }
        Some(
            div()
                .id("chat-blockers")
                .debug_selector(|| "chat-blockers".into())
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .pt_1()
                .max_h(window.viewport_size().height * 0.25)
                .overflow_y_scroll()
                .when_some(approval, |el, approval| {
                    let theme = Theme::global();
                    el.child(
                        div()
                            .debug_selector(|| "chat-pending-approval".into())
                            .rounded_xl()
                            .px_3()
                            .py_2()
                            .bg(theme.HEADER_BG)
                            .text_size(px(12.))
                            .child(format!("Approval needed: {}", approval.tool_name))
                            .child(
                                div()
                                    .text_color(theme.TEXT_DIM)
                                    .child(approval.description.clone()),
                            )
                            .child(
                                div().child("Review with `jcode permissions` on the runtime host."),
                            ),
                    )
                })
                .when_some(error, |el, index| {
                    el.child(self.render_item(index, &self.items[index], false, px(1.), window, cx))
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn clean_chat_keeps_anchor_draft_and_live_text_through_reload(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-anchor".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            for turn in 0..40 {
                panel.items.push(Item::User(format!("Question {turn}")));
                panel
                    .items
                    .push(Item::Reasoning(format!("Reasoning {turn}")));
                panel
                    .items
                    .push(Item::Assistant(format!("Public response {turn}")));
            }
            panel
        });
        vcx.update(|window, cx| {
            let input = panel.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.set_content("Unsent draft".into(), cx);
                input.set_caret(4, cx);
                window.focus(&input.focus_handle, cx);
            });
            panel.update(cx, |panel, cx| {
                panel.stick_to_bottom = false;
                panel.transcript_list.scroll_to(gpui::ListOffset {
                    item_ix: 41,
                    offset_in_item: px(3.),
                });
                cx.notify();
            });
        });
        vcx.run_until_parked();
        let before = vcx.debug_bounds("selectable-text-41-0").unwrap().top();
        let draft = panel.read_with(vcx, |panel, cx| panel.input.read(cx).snapshot());
        vcx.simulate_keystrokes(shortcuts::CLEAN_CHAT_CHORD);
        vcx.run_until_parked();
        let after = vcx.debug_bounds("selectable-text-41-0").unwrap().top();
        assert!(
            (after - before).abs() <= px(1.),
            "anchor moved from {before:?} to {after:?}"
        );
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(panel.input.read(cx).snapshot(), draft);
            assert_eq!(panel.clean_chat_override, Some(true));
            assert!(!panel.stick_to_bottom);
        });
        vcx.update(|window, cx| {
            assert!(
                panel
                    .read(cx)
                    .input
                    .read(cx)
                    .focus_handle
                    .is_focused(window)
            );
        });

        panel.update(vcx, |panel, cx| {
            panel.apply_event(
                &ApiEvent::ReasoningDelta {
                    session_id: "clean-anchor".into(),
                    text: "Hidden live reasoning".into(),
                },
                cx,
            );
            panel.apply_event(
                &ApiEvent::TextDelta {
                    session_id: "clean-anchor".into(),
                    text: "Public live answer".into(),
                    message_id: None,
                },
                cx,
            );
            panel.jump_to_latest(cx);
        });
        vcx.run_until_parked();
        let live = format!("selectable-text-{}-0", usize::MAX).leak();
        assert!(vcx.debug_bounds(live).is_some());
        let snapshot = panel.read_with(vcx, |panel, cx| {
            serde_json::to_vec(&panel.snapshot_for_reload(cx)).unwrap()
        });
        let (restored, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-anchor".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.restore_snapshot(serde_json::from_slice(&snapshot).unwrap(), cx);
            panel
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds(live).is_some(),
            "reloaded public stream must paint"
        );
        restored.read_with(vcx, |panel, cx| {
            assert_eq!(panel.input.read(cx).snapshot(), draft);
            assert_eq!(panel.clean_chat_override, Some(true));
            assert_eq!(
                panel.items.len(),
                121,
                "private reasoning settles into unmodified history"
            );
        });
    }

    #[gpui::test]
    fn clean_chat_leaves_active_decisions_and_recovery_outside_the_stream(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-blockers".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.items.push(Item::Error("Already handled".into()));
            panel.items.push(Item::User("Can we begin?".into()));
            panel.items.push(Item::Assistant("Ready".into()));
            panel.set_clean_chat(Some(true), cx);
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("recovery-error").is_none());
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::Error("Current failure".into()));
            let event = serde_json::from_value(serde_json::json!({
                "ev":"ask_request", "session_id":"clean-blockers", "tool_call_id":"ask-test",
                "request_id":"ask-test", "questions":[{"question":"Proceed?", "options":[
                    {"label":"Yes"},{"label":"No"}
                ]}]
            }))
            .unwrap();
            panel.apply_event(&event, cx);
            panel.apply_event(
                &ApiEvent::PermissionRequest {
                    session_id: "clean-blockers".into(),
                    request_id: "approval-test".into(),
                    tool_name: "test-tool".into(),
                    description: "Approval fixture".into(),
                },
                cx,
            );
        });
        vcx.run_until_parked();
        let blockers = vcx.debug_bounds("chat-blockers").unwrap();
        assert!(blockers.intersects(&vcx.debug_bounds("recovery-error").unwrap()));
        assert!(blockers.intersects(&vcx.debug_bounds("chat-pending-approval").unwrap()));
        assert!(vcx.debug_bounds("transcript-row-2").is_none());
        let option = vcx
            .debug_bounds("ask-option-0-1")
            .expect("active Ask remains actionable");
        vcx.simulate_click(option.center(), Default::default());
        panel.read_with(vcx, |panel, _| {
            assert!(panel.ask_card.as_ref().unwrap().chosen[0].contains(&1))
        });
    }
}
