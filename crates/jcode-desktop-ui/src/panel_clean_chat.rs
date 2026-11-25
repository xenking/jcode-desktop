use super::*;

pub(super) fn conversation_item(item: &Item) -> bool {
    match item {
        Item::User(_) => classify::is_pinnable_prompt(item),
        Item::Assistant(text) | Item::AssistantOutput { text, .. } => !text.trim().is_empty(),
        Item::Image(image) => match &image.source {
            jcode_sdk::RenderedImageSource::UserInput => true,
            jcode_sdk::RenderedImageSource::Other { role } => role == "assistant",
            jcode_sdk::RenderedImageSource::ToolResult { .. } => false,
        },
        Item::Applet(_) => true,
        Item::Tool { name, done, .. } => name == "ask" && *done,
        Item::AgentMessage { message, .. } => classify::is_external(message),
        Item::SideQuestion(_) => true,
        Item::Reasoning(_)
        | Item::BackgroundTask { .. }
        | Item::Todos(_)
        | Item::ResponseStats(_)
        | Item::Error(_)
        | Item::RuntimeError { .. }
        | Item::Stopped(_)
        | Item::ProviderGuardrailRetry { .. }
        | Item::Compaction(_)
        | Item::Memory(_)
        | Item::CacheMiss(_) => false,
        Item::SwarmWorkerNotice(_) => true,
    }
}

pub(super) fn active_error(items: &[Item]) -> Option<usize> {
    for (index, item) in items.iter().enumerate().rev() {
        match item {
            Item::Error(_) | Item::RuntimeError { .. } => return Some(index),
            Item::Stopped(_) => return Some(index),
            Item::User(_) if conversation_item(item) => break,
            item if item
                .assistant_text()
                .is_some_and(|text| !markdown::public_transcript(text).trim().is_empty()) =>
            {
                break;
            }
            _ => {}
        }
    }
    None
}

pub(super) fn message_heading(item: &Item, previous: Option<&Item>) -> Option<gpui::AnyElement> {
    let (label, selector, separator, emphasized) = match item {
        Item::User(_) => ("You", "clean-chat-user", previous.is_some(), true),
        Item::Tool {
            name,
            done: true,
            output,
            error,
            ..
        } if name == "ask" && classify::answered(output, error.as_deref()) => {
            ("You", "clean-chat-user", previous.is_some(), true)
        }
        Item::AssistantOutput {
            phase, stream_id, ..
        } => {
            if let Some(Item::AssistantOutput {
                phase: before,
                stream_id: Some(prior),
                ..
            }) = previous
                && before == phase
                && stream_id.as_ref() == Some(prior)
            {
                return None;
            }
            match phase {
                AssistantOutputPhase::Final => ("Final", "clean-chat-final", true, true),
                AssistantOutputPhase::Update => ("Update", "clean-chat-update", false, false),
                AssistantOutputPhase::Pending => {
                    ("Responding", "clean-chat-responding", false, false)
                }
            }
        }
        _ => return None,
    };
    let theme = Theme::global();
    Some(
        div()
            .debug_selector(move || selector.into())
            .flex()
            .flex_col()
            .gap_2()
            .pb_1()
            .when(separator, |el| {
                el.child(div().w_full().h(px(1.)).bg(theme.PANEL_BORDER))
            })
            .child(
                div()
                    .px_1()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(if emphasized {
                        theme.TEXT
                    } else {
                        theme.TEXT_DIM
                    })
                    .child(label),
            )
            .into_any_element(),
    )
}

pub(super) fn screenshot_fixture() -> Vec<Item> {
    let reasoning = jcode_render_core::reasoning_line_markup("## Respecting the todo lane");
    let history = serde_json::from_value(serde_json::json!([
        {"role": "user", "content": "What did the earlier check cover?"},
        {"role": "assistant", "content": "Archived note. The previous check covered the original baseline only."},
        {"role": "user", "content": "Check the import again and give me the results."},
        {"role": "assistant", "content": format!("{reasoning}The import is running. I will compare the row counts before and after the retry, then check that the output is consistent.")},
        {"role": "tool", "content": "Internal tool output", "tool": {"id": "audit", "name": "read", "input": {}}},
        {"role": "assistant", "response_stats": {"output_tokens": 64}, "content": "The import passed. Every source record appears once, and the second run produced the same output.\n\nNo records were dropped. The exported report matches the source totals."}
    ])).expect("valid clean chat history fixture");
    crate::panel::history_items::build(history, vec![]).0
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

impl Panel {
    pub(super) fn clean_chat_with_display(&self, display: &crate::config::DisplayConfig) -> bool {
        self.clean_chat_override
            .unwrap_or(display.clean_chat_default)
    }

    pub(super) fn clean_chat_enabled(&self) -> bool {
        self.clean_chat_with_display(&crate::config::display())
    }

    pub(super) fn shows_transcript_activity(&self) -> bool {
        self.activity_active() && self.ask_card.is_none()
    }

    pub(super) fn set_clean_chat(&mut self, override_mode: Option<bool>, cx: &mut Context<Self>) {
        self.clean_chat_override = override_mode;
        cx.notify();
    }

    pub(super) fn toggle_clean_chat(&mut self, cx: &mut Context<Self>) {
        self.set_clean_chat(Some(!self.clean_chat_enabled()), cx);
    }

    pub(super) fn render_clean_chat_toggle(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let enabled = self.clean_chat_enabled();
        let (tooltip, toggled, color) = if enabled {
            (
                "Clean chat: On (Ctrl+Shift+J)",
                gpui::Toggled::True,
                theme.ACCENT,
            )
        } else {
            (
                "Clean chat: Off (Ctrl+Shift+J)",
                gpui::Toggled::False,
                theme.composer_secondary(),
            )
        };
        const ID: &str = "clean-chat-toggle";
        crate::keyboard::button(
            div()
                .id(ID)
                .debug_selector(|| ID.into())
                .relative()
                .flex_none()
                .size(px(chrome::TAB_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if enabled { theme.ACCENT } else { gpui::rgba(0) })
                .bg(if enabled {
                    theme.ACCENT_DIM
                } else {
                    theme.chip_background()
                })
                .hover(|el| el.bg(theme.chip_active_background()))
                .cursor_pointer()
                .role(gpui::Role::Switch)
                .aria_label("Clean chat")
                .aria_description("Show only the conversation")
                .aria_keyshortcuts("Control+Shift+J")
                .aria_toggled(toggled)
                .children(crate::fixture_control_targets::marker(
                    || {
                        if enabled {
                            "clean-chat-on"
                        } else {
                            "clean-chat-off"
                        }
                        .into()
                    },
                    || Some(self.session_id.clone()),
                ))
                .child(
                    gpui::svg()
                        .data(include_bytes!("../../../assets/icons/comment.svg") as &'static [u8])
                        .text_color(color)
                        .size(px(chrome::BAR_ICON)),
                ),
        )
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(cx.listener(|panel, _, _, cx| {
            panel.toggle_clean_chat(cx);
            cx.stop_propagation();
        }))
        .tooltip(move |_, cx| cx.new(|_| meter::MeterTooltip(tooltip.into())).into())
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
                    el.child(self.render_item(
                        index,
                        &self.items[index],
                        &crate::config::display(),
                        false,
                        px(1.),
                        window,
                        cx,
                    ))
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn clean_chat_shows_work_and_confirmed_interruption_without_trace(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-status".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel.items.push(Item::User("Run a check".into()));
            panel
        });
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::SessionStatus {
                    session_id: "clean-status".into(),
                    status: "thinking".into(),
                    pending_soft_interrupts: None,
                },
                cx,
            );
            panel.apply(
                &ApiEvent::ToolStart {
                    session_id: "clean-status".into(),
                    call_id: "check".into(),
                    name: "read".into(),
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("transcript-activity").is_some());
        assert!(vcx.debug_bounds("tool-header").is_none());
        assert!(vcx.debug_bounds("prompt-number-0-1").is_some());
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::SessionStatus {
                    session_id: "clean-status".into(),
                    status: "cancelled".into(),
                    pending_soft_interrupts: None,
                },
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("transcript-activity").is_none());
        assert!(vcx.debug_bounds("response-stop-notice").is_some());
        assert!(vcx.debug_bounds("tool-header").is_none());
    }

    #[gpui::test]
    fn clean_chat_numbers_prompts_and_decisions_together(cx: &mut gpui::TestAppContext) {
        let (_, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new("clean-numbers".into(), None, None, crate::harness::spawn_inert(), cx);
            panel.set_clean_chat(Some(true), cx);
            *panel.items = vec![
                Item::User("First".into()),
                Item::Tool { call_id: "ask-1".into(), name: "ask".into(),
                    input: r#"{"questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}"#.into(),
                    output: "User answers:\n1. Which? -> B".into(), done: true, error: None },
                Item::User("Next".into()),
            ];
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-number-0-1").is_some());
        assert!(vcx.debug_bounds("decision-number-2").is_some());
        assert!(vcx.debug_bounds("prompt-number-2-3").is_some());
        assert!(vcx.debug_bounds("tool-header").is_none());
    }

    #[gpui::test]
    fn clean_chat_keeps_failure_and_force_stop_action_visible(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-stop".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel.items.push(Item::User("Run".into()));
            panel.items.push(Item::Error("System error".into()));
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("recovery-error").is_some());
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::User("Retry".into()));
            let mut notice = turn::StopNotice::cancel_requested();
            notice.title = "Stop not confirmed".into();
            panel.items.push(Item::Stopped(notice));
            cx.notify();
        });
        vcx.run_until_parked();
        let button = vcx
            .debug_bounds("force-stop-button")
            .expect("force stop is available");
        vcx.simulate_click(button.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("response-stop-notice").is_some());
        panel.read_with(vcx, |panel, _| assert!(matches!(panel.items.last(), Some(Item::Stopped(notice)) if notice.title == "Force stop requested")));
    }

    fn text(id: &str, text: &str) -> ApiEvent {
        ApiEvent::TextDelta {
            session_id: "clean-phases".into(),
            text: text.into(),
            message_id: Some(id.into()),
        }
    }

    fn text_done(id: &str) -> ApiEvent {
        ApiEvent::TextDone {
            session_id: "clean-phases".into(),
            message_id: Some(id.into()),
        }
    }

    fn turn_done() -> ApiEvent {
        ApiEvent::TurnDone {
            session_id: "clean-phases".into(),
            pending_soft_interrupts: None,
        }
    }

    #[gpui::test]
    fn clean_chat_paints_stream_and_final_without_toggling(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-phases".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel.items.push(Item::User("Running".into()));
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("assistant-response").is_none());
        panel.update(vcx, |panel, cx| {
            panel.apply(&text("reply", "Live answer"), cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("assistant-response").is_some());
        panel.update(vcx, |panel, cx| {
            panel.apply(&text_done("reply"), cx);
            panel.apply(&turn_done(), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("clean-chat-final").is_some());
    }

    #[gpui::test]
    fn clean_chat_final_requires_a_completed_turn_even_across_reload(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-phases".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.set_clean_chat(Some(true), cx);
            panel
                .items
                .push(Item::User("Compare chronological results".into()));
            for event in [
                text("commentary", "Final is a word, not a phase."),
                text_done("commentary"),
                ApiEvent::ReasoningDelta {
                    session_id: "clean-phases".into(),
                    text: "## Respecting the todo lane\nPrivate reasoning".into(),
                },
                text("report", "Uncorrected report"),
            ] {
                panel.apply(&event, cx);
            }
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("clean-chat-user").is_some());
        assert!(vcx.debug_bounds("clean-chat-update").is_some());
        assert!(vcx.debug_bounds("clean-chat-responding").is_some());
        assert!(vcx.debug_bounds("clean-chat-final").is_none());
        assert!(vcx.debug_bounds("reasoning-inline").is_none());
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::TextReplace {
                    session_id: "clean-phases".into(),
                    text: "Corrected report.".into(),
                    message_id: Some("report".into()),
                },
                cx,
            );
            panel.apply(&text_done("report"), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("clean-chat-final").is_none());

        let snapshot = panel.read_with(vcx, |panel, cx| {
            serde_json::to_vec(&panel.snapshot_for_reload(cx)).unwrap()
        });
        let (restored, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "clean-phases".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            panel.restore_snapshot(serde_json::from_slice(&snapshot).unwrap(), cx);
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("clean-chat-final").is_none());
        restored.update(vcx, |panel, cx| panel.apply(&turn_done(), cx));
        vcx.run_until_parked();
        let update = vcx.debug_bounds("clean-chat-update").unwrap();
        let final_report = vcx.debug_bounds("clean-chat-final").unwrap();
        assert!(update.bottom() < final_report.top());
        assert!(vcx.debug_bounds("reasoning-inline").is_none());
        restored.read_with(vcx, |panel, _| {
            assert!(panel.items.iter().any(|item| matches!(item, Item::AssistantOutput { text, phase: AssistantOutputPhase::Final, .. } if text == "Corrected report.")));
        });
    }

    #[gpui::test]
    fn clean_chat_does_not_promote_tool_prefaces_retracted_or_stopped_text(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new("clean-phases".into(), None, None, crate::harness::spawn_inert(), cx);
            panel.set_clean_chat(Some(true), cx);
            panel.items.push(Item::User("Check tool output".into()));
            for value in [
                serde_json::json!({"ev":"text_delta", "message_id":"tool-preface", "text":"Started checking."}),
                serde_json::json!({"ev":"text_done", "message_id":"tool-preface"}),
                serde_json::json!({"ev":"tool_start", "call_id":"check", "name":"read"}),
                serde_json::json!({"ev":"tool_done", "call_id":"check", "name":"read", "output":"private"}),
                serde_json::json!({"ev":"text_delta", "message_id":"retracted", "text":"Retracted answer"}),
                serde_json::json!({"ev":"text_done", "message_id":"retracted"}),
                serde_json::json!({"ev":"text_replace", "message_id":"retracted", "text":""}),
                serde_json::json!({"ev":"turn_done"}),
                serde_json::json!({"ev":"text_delta", "message_id":"stopped", "text":"Partial answer"}),
                serde_json::json!({"ev":"text_done", "message_id":"stopped"}),
                serde_json::json!({"ev":"session_status", "status":"cancelled"}),
                serde_json::json!({"ev":"turn_done"}),
            ] {
                let mut value = value;
                value["session_id"] = "clean-phases".into();
                panel.apply(&serde_json::from_value(value).unwrap(), cx);
            }
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("clean-chat-update").is_some());
        assert!(vcx.debug_bounds("clean-chat-final").is_none());
        assert!(vcx.debug_bounds("tool-header").is_none());
    }

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
                window.focus(&input.editor_focus(), cx);
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
