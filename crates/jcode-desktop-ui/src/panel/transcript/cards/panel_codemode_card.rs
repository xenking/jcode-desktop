use super::*;
use super::classify::{is_codemode, wrapper_failed};
use jcode_sdk::api::{CodemodeCall, CodemodeCallStatus, CodemodeSummary};


pub(super) fn history_summaries(
    messages: &[jcode_sdk::HistoryMessage],
) -> impl Iterator<Item = (String, Box<CodemodeSummary>)> + '_ {
    messages.iter().filter_map(|message| {
        let tool = message.tool.as_ref()?;
        if message.role != "tool" || !is_codemode(&tool.name) {
            return None;
        }
        Some((tool.id.clone(), tool.codemode.as_ref()?.clone()))
    })
}

pub(super) fn single_swarm(summary: &CodemodeSummary) -> Option<&CodemodeCall> {
    let [call] = summary.calls.as_slice() else {
        return None;
    };
    (summary.calls_omitted == 0
        && classify::is_swarm(&call.tool)
        && call.action.is_some()
        && call.status != CodemodeCallStatus::Interrupted)
        .then_some(call)
}


fn source(input: &str) -> Option<String> {
    let args: serde_json::Value = serde_json::from_str(input).ok()?;
    ["code", "script", "source", "js"]
        .into_iter()
        .find_map(|key| args.get(key)?.as_str().map(str::to_owned))
}

fn separate_error<'a>(body: &str, error: Option<&'a str>) -> Option<&'a str> {
    let body_diagnostic = body.strip_prefix("error: ").unwrap_or(body).trim();
    error.filter(|error| error.trim() != body_diagnostic)
}

fn report_body(output: &str, body_bytes: Option<usize>) -> &str {
    body_bytes
        .and_then(|end| output.get(..end))
        .unwrap_or(output)
}

impl Panel {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_codemode_detail(
        &self,
        index: usize,
        call_id: &str,
        input: &str,
        output: &str,
        error: Option<&str>,
        summary: Option<&CodemodeSummary>,
        single: Option<&CodemodeCall>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let failed = wrapper_failed(summary, error, output);
        let report = report_body(
            output,
            summary.and_then(|summary| summary.report_body_bytes),
        );
        let body = match report.strip_prefix("result:\n") {
            Some(result) if result.trim_end_matches('\n').is_empty() => "\"\"",
            Some(result) => result,
            None => report,
        };
        let error = separate_error(report, error);
        let heading = if failed && error.is_none() {
            "Codemode error"
        } else {
            "Result"
        };
        div()
            .debug_selector(|| "codemode-detail".into())
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .when_some(single, |el, call| {
                el.child(self.render_codemode_call(index, 0, call, false, window, cx))
            })
            .when_some(error, |el, error| {
                el.child(
                    self.codemode_section(
                        "codemode-error",
                        format!("codemode-error-{index}"),
                        "Codemode error",
                        error,
                        window,
                        cx,
                    )
                    .text_color(theme.ERROR),
                )
            })
            .when(!body.is_empty(), |el| {
                el.child(
                    self.codemode_section(
                        if failed && error.is_none() {
                            "codemode-error"
                        } else {
                            "codemode-result"
                        },
                        format!("codemode-result-{index}"),
                        heading,
                        body,
                        window,
                        cx,
                    )
                    .when(failed && error.is_none(), |el| el.text_color(theme.ERROR)),
                )
            })
            .when_some(summary.filter(|_| single.is_none()), |el, summary| {
                el.child(self.render_codemode_calls(index, call_id, summary, window, cx))
            })
            .children(
                self.render_codemode_source(index, input, window, cx)
                    .or_else(|| {
                        let arguments = tool_output::ToolDetail::arguments(input);
                        (!arguments.is_empty()).then(|| {
                            tool_output::selectable_arguments(
                                arguments,
                                index,
                                self.transcript_selection.clone(),
                                window,
                                cx,
                            )
                            .into_any_element()
                        })
                    }),
            )
            .into_any_element()
    }

    fn render_codemode_calls(
        &self,
        index: usize,
        call_id: &str,
        summary: &CodemodeSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let count = summary.calls.len().saturating_add(summary.calls_omitted);
        if count == 0 {
            return div()
                .debug_selector(|| "codemode-no-calls".into())
                .text_color(theme.TEXT_FAINT)
                .child("No tools were called")
                .into_any_element();
        }
        let failures = summary
            .calls
            .iter()
            .filter(|call| call.status != CodemodeCallStatus::Success)
            .count();
        let key = format!("codemode-calls:{call_id}");
        let expanded = self.expanded_regions.contains(&key);
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap_1()
            .child(
                crate::keyboard::button(div().id(("codemode-calls", index)))
                    .debug_selector(|| "codemode-calls-toggle".into())
                    .aria_expanded(expanded)
                    .cursor_pointer()
                    .self_start()
                    .rounded_full()
                    .px_2()
                    .py_1()
                    .bg(theme.INLINE_CODE_BG)
                    .hover(|el| el.bg(theme.TOOL_BORDER))
                    .text_color(theme.TEXT_DIM)
                    .child(format!(
                        "{} Calls · {count}",
                        if expanded { "▾" } else { "▸" }
                    ))
                    .when(failures != 0, |el| {
                        el.child(
                            div()
                                .ml_1()
                                .text_color(theme.ERROR)
                                .child(format!(" · {failures} with issues")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_line(&key, cx);
                    })),
            )
            .when(expanded, |el| {
                el.child(
                    div()
                        .debug_selector(|| "codemode-calls-list".into())
                        .flex()
                        .flex_col()
                        .gap_2()
                        .pl_2()
                        .children(summary.calls.iter().enumerate().map(|(ordinal, call)| {
                            self.render_codemode_call(index, ordinal, call, true, window, cx)
                        }))
                        .when(summary.calls_omitted > 0, |el| {
                            el.child(
                                div().text_color(theme.TEXT_FAINT).child(format!(
                                    "{} more calls not shown",
                                    summary.calls_omitted
                                )),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn render_codemode_call(
        &self,
        index: usize,
        ordinal: usize,
        call: &CodemodeCall,
        show_preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = Theme::global();
        let (status, color) = match call.status {
            CodemodeCallStatus::Success => ("Done", theme.TEXT_FAINT),
            CodemodeCallStatus::Error => ("Failed", theme.ERROR),
            CodemodeCallStatus::Interrupted => ("Interrupted", theme.WARN),
        };
        let label = match (&call.action, &call.target) {
            (Some(action), Some(target)) => format!("{} {action} · {target}", call.tool),
            (Some(action), None) => format!("{} {action}", call.tool),
            _ => call.tool.clone(),
        };
        div()
            .debug_selector(move || format!("codemode-call-{}", ordinal + 1))
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!(
                        "codemode-call-{index}-{ordinal}"
                    )))
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(format!("#{}", ordinal + 1)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(div().flex_none().text_color(color).child(status))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(format!("{} ms", call.duration_ms)),
                    ),
            )
            .when_some(call.diagnostic.as_deref(), |el, diagnostic| {
                el.child(
                    div()
                        .debug_selector(|| "codemode-call-diagnostic".into())
                        .pl_4()
                        .text_color(color)
                        .child(text_selection::plain(
                            self.transcript_selection.clone(),
                            format!("codemode-call-diagnostic-{index}-{ordinal}"),
                            strip_ansi(diagnostic),
                            window,
                            cx,
                        )),
                )
            })
            .when_some(
                call.preview.as_deref().filter(|_| show_preview),
                |el, preview| {
                    el.child(
                        div()
                            .pl_4()
                            .text_color(theme.TEXT_DIM)
                            .child(text_selection::plain(
                                self.transcript_selection.clone(),
                                format!("codemode-call-preview-{index}-{ordinal}"),
                                strip_ansi(preview),
                                window,
                                cx,
                            )),
                    )
                },
            )
    }

    fn codemode_section(
        &self,
        selector: &'static str,
        key: String,
        title: &'static str,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .debug_selector(move || selector.into())
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .font_family(Theme::global().FONT_AI)
                    .text_color(Theme::global().TEXT_DIM)
                    .child(title),
            )
            .child(text_selection::plain(
                self.transcript_selection.clone(),
                key,
                strip_ansi(body),
                window,
                cx,
            ))
    }

    fn render_codemode_source(
        &self,
        index: usize,
        input: &str,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let code = source(input)?;
        Some(
            div()
                .id(("codemode-source", index))
                .debug_selector(|| "codemode-source".into())
                .min_w_0()
                .child(
                    div()
                        .font_family(Theme::global().FONT_AI)
                        .text_color(Theme::global().TEXT_DIM)
                        .child("Source"),
                )
                .child(crate::code_block::code_block_with_selection(
                    "typescript",
                    &code,
                    window,
                    Some((
                        &self.transcript_selection,
                        format!("codemode-source-{index}").into(),
                        cx,
                    )),
                ))
                .into_any_element(),
        )
    }
}

pub(super) fn screenshot_history() -> Vec<jcode_sdk::HistoryMessage> {
    let single = "result:\nChecks scheduled.\n";
    let multi = "result:\n2 tasks remain.\n";
    serde_json::from_value(serde_json::json!([
        {
            "role": "tool", "content": "Direct message sent to camel",
            "tool": {"id": "fixture-swarm-direct", "name": "swarm", "duration_ms": 62,
                "input": {"action": "message", "to_session": "camel", "message": "Please check the native control flow before merging."}}
        },
        {
            "role": "user", "timestamp_ms": 1760000000000i64,
            "content": "DM from camel: The native control flow passes. The remaining check is the compact layout."
        },
        {
            "role": "tool", "content": format!("{single}\ncalls:\n  #1 swarm ok 39ms 20B\n\n[1 tool call, 0.1s]"),
            "tool": {"id": "fixture-codemode-single", "name": "codemode", "duration_ms": 104,
                "input": {"code": "const send = tools['swa' + 'rm'];\nawait send({ action: 'message', to_session: 'camel', message: 'Check the narrow layout.' });\nreturn 'Checks scheduled.';", "intent": "Ask camel for the narrow layout check"},
                "report_body_bytes": single.len(), "codemode_status": "success", "codemode_calls": [
                    {"tool": "swarm", "action": "message", "target": "camel", "direction": "outbound", "status": "success", "duration_ms": 39, "preview": "Check the narrow layout."}
                ]}
        },
        {
            "role": "tool", "content": format!("{multi}\ncalls:\n  #1 todo ok 24ms 60000B\n  #2 swarm error 88ms 0B\n\n[2 tool calls, 0.3s]"),
            "tool": {"id": "fixture-codemode-multi", "name": "codemode", "duration_ms": 302,
                "input": {"code": "const tasks = await tools.todo({});\nawait tools.swarm({ action: 'summary', to_session: 'camel' }).catch(() => null);\nreturn '2 tasks remain.';", "intent": "Summarize the remaining checks"},
                "report_body_bytes": multi.len(), "codemode_status": "success", "codemode_calls": [
                    {"tool": "todo", "status": "success", "duration_ms": 24},
                    {"tool": "swarm", "action": "summary", "target": "camel", "status": "error", "duration_ms": 88, "diagnostic": "Resource not found"}
                ]}
        }
    ])).expect("valid tool cards fixture")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_sdk::api::CodemodeStatus;

    fn test_panel(cx: &mut gpui::TestAppContext) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
        cx.add_window_view(|_, cx| {
            Panel::new(
                "tool-cards-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        })
    }

    fn single_history() -> jcode_sdk::HistoryMessage {
        screenshot_history().remove(2)
    }

    #[test]
    fn duplicate_error_is_suppressed_but_a_separate_result_with_the_same_text_is_not() {
        let error = Some("checks scheduled");
        assert_eq!(separate_error("error: checks scheduled\n", error), None);
        assert_eq!(separate_error("checks scheduled", error), None);
        assert_eq!(separate_error("result:\nchecks scheduled\n", error), error);
        assert_eq!(separate_error("result:\nother result\n", error), error);
    }

    #[test]
    fn output_boundary_preserves_literal_footer_and_legacy_or_invalid_offsets() {
        let user_body = "result:\nГотово 🐪\n\ncalls:\n  this is part of the result\n";
        let output = format!("{user_body}\ncalls:\n  #1 swarm ok 3ms 5B\n\n[1 tool call, 0.0s]");
        assert_eq!(report_body(&output, Some(user_body.len())), user_body);
        for boundary in [None, Some("result:\n".len() + 1), Some(output.len() + 1)] {
            assert_eq!(report_body(&output, boundary), output);
        }
    }

    #[test]
    fn screenshot_payload_uses_wire_preview_and_small_todo_return() {
        let entries = screenshot_history();
        let single = entries[2].tool.as_ref().unwrap().codemode.as_ref().unwrap();
        let swarm = single_swarm(single).unwrap();
        assert_eq!(swarm.preview.as_deref(), Some("Check the narrow layout."));
        assert_eq!(
            report_body(&entries[2].content, single.report_body_bytes),
            "result:\nChecks scheduled.\n"
        );
        let multi = entries[3].tool.as_ref().unwrap().codemode.as_ref().unwrap();
        assert_eq!(multi.calls[0].tool, "todo");
        assert_eq!(
            multi.calls[1].diagnostic.as_deref(),
            Some("Resource not found")
        );
        assert_eq!(
            report_body(&entries[3].content, multi.report_body_bytes),
            "result:\n2 tasks remain.\n"
        );
    }

    #[test]
    fn live_tool_done_and_history_share_exact_typed_summary() {
        let entries = screenshot_history();
        for message in &entries[2..] {
            let tool = message.tool.as_ref().unwrap();
            let history_json = serde_json::to_value(tool).unwrap();
            let live: ApiEvent = serde_json::from_value(serde_json::json!({
                "ev": "tool_done", "session_id": "tool-cards-test", "call_id": tool.id,
                "name": tool.name, "output": message.content,
                "codemode_status": history_json["codemode_status"],
                "codemode_calls": history_json["codemode_calls"],
                "report_body_bytes": history_json["report_body_bytes"],
            }))
            .unwrap();
            let ApiEvent::ToolDone {
                codemode, output, ..
            } = live
            else {
                panic!("expected a tool_done event");
            };
            assert_eq!(codemode, tool.codemode);
            assert_eq!(output, message.content);
        }
    }

    #[gpui::test]
    fn codemode_single_swarm_keeps_provenance_and_reports_wrapper_failure(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = test_panel(cx);
        let history = single_history();
        let tool = history.tool.unwrap();
        for event in [
            ApiEvent::ToolStart {
                session_id: "tool-cards-test".into(),
                call_id: tool.id.clone(),
                name: tool.name.clone(),
            },
            ApiEvent::ToolInputDelta {
                session_id: "tool-cards-test".into(),
                call_id: tool.id.clone(),
                delta: tool.input.to_string(),
            },
        ] {
            panel.update(vcx, |panel, cx| panel.apply(&event, cx));
        }
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-header").is_some());
        assert!(vcx.debug_bounds("swarm-codemode-provenance").is_none());
        let mut summary = tool.codemode.unwrap();
        summary.status = CodemodeStatus::Error;
        let body = "error: wrapper aborted after delivery\n";
        let output = format!("{body}\n[1 tool call, 0.1s]");
        summary.report_body_bytes = Some(body.len());
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::ToolDone {
                    session_id: "tool-cards-test".into(),
                    call_id: tool.id.clone(),
                    name: tool.name.clone(),
                    output: output.clone(),
                    error: None,
                    codemode: Some(summary.clone()),
                    ask: None,
                },
                cx,
            )
        });
        vcx.run_until_parked();
        let header = vcx.debug_bounds("tool-swarm-header").unwrap();
        assert!(vcx.debug_bounds("tool-card-tokens").is_some());
        assert!(vcx.debug_bounds("tool-icon-failed").is_some());
        for width in [480., 1250.] {
            vcx.simulate_window_resize(vcx.window_handle(), gpui::size(px(width), px(720.)));
            vcx.run_until_parked();
            let header = vcx.debug_bounds("tool-swarm-header").unwrap();
            let provenance = vcx.debug_bounds("swarm-codemode-provenance").unwrap();
            let preview = vcx.debug_bounds("tool-swarm-preview").unwrap();
            let tokens = vcx.debug_bounds("tool-card-tokens").unwrap();
            assert!(preview.size.width > px(0.));
            assert!(preview.right() <= provenance.left() && tokens.right() <= header.right());
        }
        assert_eq!(
            vcx.debug_bounds("tool-swarm-header").unwrap().size.height,
            header.size.height
        );
        let center = vcx.debug_bounds("tool-swarm-header").unwrap().center();
        vcx.simulate_click(center, gpui::Modifiers::none());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("codemode-error").is_some());
        assert!(vcx.debug_bounds("codemode-source").is_some());
        assert!(vcx.debug_bounds("codemode-call-1").is_some());
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("codemode-source").is_none());

        let mut saved = single_history();
        saved.content = output;
        let stored_tool = saved.tool.as_mut().unwrap();
        stored_tool.id = "stored-wrapper".into();
        stored_tool.codemode = Some(summary);
        let saved = serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
        let (panel, vcx) = test_panel(cx);
        panel.update(vcx, |panel, cx| panel.load_history(vec![saved], vec![], cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-codemode-provenance").is_some());
        assert!(vcx.debug_bounds("tool-icon-failed").is_some());
    }

    #[gpui::test]
    fn codemode_replay_lists_calls_in_dispatch_order_without_replacing_the_result(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = test_panel(cx);
        let history = screenshot_history().pop().unwrap();
        panel.update(vcx, |panel, cx| {
            panel.load_history(vec![history], vec![], cx);
            panel.toggle_line("fixture-codemode-multi", cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-inline").is_some());
        assert!(vcx.debug_bounds("codemode-calls-list").is_none());
        let result = vcx.debug_bounds("codemode-result").unwrap();
        let toggle = vcx.debug_bounds("codemode-calls-toggle").unwrap();
        vcx.simulate_click(toggle.center(), gpui::Modifiers::none());
        vcx.run_until_parked();
        let first = vcx.debug_bounds("codemode-call-1").unwrap();
        let second = vcx.debug_bounds("codemode-call-2").unwrap();
        let diagnostic = vcx.debug_bounds("codemode-call-diagnostic").unwrap();
        let code = vcx.debug_bounds("codemode-source").unwrap();
        assert!(result.bottom() <= first.top());
        assert!(first.bottom() <= second.top() && second.bottom() <= code.top());
        assert!(diagnostic.top() >= second.top() && diagnostic.bottom() <= second.bottom());
        assert!(vcx.debug_bounds("tool-output-size").is_some());
    }

    #[gpui::test]
    fn codemode_noop_partial_and_legacy_do_not_infer_swarm_from_the_script(
        cx: &mut gpui::TestAppContext,
    ) {
        for mode in ["noop", "partial", "interrupted", "legacy"] {
            let (panel, vcx) = test_panel(cx);
            let mut history = single_history();
            let tool = history.tool.as_mut().unwrap();
            match mode {
                "noop" => tool.codemode.as_mut().unwrap().calls.clear(),
                "partial" => tool.codemode.as_mut().unwrap().calls_omitted = 1,
                "interrupted" => {
                    tool.codemode.as_mut().unwrap().calls[0].status =
                        CodemodeCallStatus::Interrupted
                }
                "legacy" => tool.codemode = None,
                _ => unreachable!(),
            }
            let call_id = tool.id.clone();
            panel.update(vcx, |panel, cx| {
                panel.load_history(vec![history], vec![], cx);
                panel.toggle_line(&call_id, cx);
            });
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("swarm-codemode-provenance").is_none(),
                "{mode}"
            );
            assert!(vcx.debug_bounds("tool-inline").is_some(), "{mode}");
            assert!(vcx.debug_bounds("codemode-result").is_some(), "{mode}");
            assert!(vcx.debug_bounds("codemode-source").is_some(), "{mode}");
            assert_eq!(
                vcx.debug_bounds("codemode-no-calls").is_some(),
                mode == "noop"
            );
        }
    }
}
