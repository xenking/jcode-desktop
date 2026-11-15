//! Inline `swarm` tool calls as worker rows, not a tool-call size.
//!
//! The old card showed the coordinator's own call output ("~15 tok"), which
//! says nothing about the worker. A call that names workers (spawn, await,
//! stop, status, message, resume) now lists each one on a line like the CLI's
//! swarm member: animal icon, live status glyph, task, status, model,
//! provider and the tokens that worker has spent in total. Clicking a row
//! opens that worker's chat.

use super::*;
use crate::swarm_glyph::Mark;
use crate::swarm_view::{Worker, WorkerState, friendly_name};
use jcode_desktop_harness::swarm_usage::{WorkerUsage, format_tokens};

/// A parsed `swarm` call: what was asked and which workers it concerns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Call {
    pub action: String,
    pub intent: Option<String>,
    pub session_ids: Vec<String>,
}

/// One worker line, ready to render.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub session_id: String,
    /// Animal emoji from the friendly name, when known.
    pub icon: Option<&'static str>,
    pub state: WorkerState,
    /// Task label, else the friendly name.
    pub title: String,
    /// `running`, `ready`, `failed`... Empty when nothing is known.
    pub status: String,
    /// `Opus 5.5 · claude`, when the worker's session record names them.
    pub model: Option<String>,
    /// `4.5M tok`: the worker's total spend, never the call's size.
    pub tokens: Option<String>,
}

pub(super) fn parse(name: &str, input: &str, output: &str) -> Option<Call> {
    if name.trim_start_matches("functions.") != "swarm" {
        return None;
    }
    let input: serde_json::Value = serde_json::from_str(input).ok()?;
    let action = input.get("action")?.as_str()?.to_owned();
    let mut ids: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        let id = id.trim();
        if id.starts_with("session_") && !ids.iter().any(|known| known == id) {
            ids.push(id.to_owned());
        }
    };
    if let Some(list) = input.get("session_ids").and_then(|v| v.as_array()) {
        list.iter().filter_map(|v| v.as_str()).for_each(&mut push);
    }
    for key in ["target_session", "to_session", "session_id"] {
        if let Some(id) = input.get(key).and_then(|v| v.as_str()) {
            push(id);
        }
    }
    if action == "spawn" {
        // "Spawned new agent: session_fox_1794..._ab12" names the new worker.
        for word in output.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')) {
            if word.starts_with("session_") && word.matches('_').count() >= 2 {
                push(word);
            }
        }
    }
    Some(Call {
        action,
        intent: input
            .get("intent")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        session_ids: ids,
    })
}

/// `claude-oauth` -> `claude`: the route suffix is noise on a one-line row.
fn provider_label(key: &str) -> &str {
    key.strip_suffix("-oauth")
        .or_else(|| key.strip_suffix("-api"))
        .unwrap_or(key)
}

pub(super) fn rows(
    call: &Call,
    workers: &[Worker],
    usage: &HashMap<String, WorkerUsage>,
) -> Vec<Row> {
    call.session_ids
        .iter()
        .map(|id| {
            let worker = workers.iter().find(|w| &w.session_id == id);
            let name = worker
                .map(|w| w.name.clone())
                .or_else(|| friendly_name(id).map(str::to_owned))
                .unwrap_or_else(|| id.clone());
            let used = usage.get(id);
            Row {
                session_id: id.clone(),
                icon: friendly_name(id)
                    .map(jcode_core::id::session_icon)
                    .filter(|icon| *icon != "💫"),
                state: worker.map_or(WorkerState::Unknown, |w| w.state),
                title: worker
                    .and_then(|w| w.label.clone())
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or(name),
                status: worker
                    .filter(|w| w.state != WorkerState::Unknown)
                    .map(|w| w.state.label().to_owned())
                    .unwrap_or_default(),
                model: used.and_then(|u| {
                    let model = u
                        .model
                        .as_deref()
                        .map(|m| super::composer::pretty_model_label(Some(m)))?;
                    Some(match u.provider.as_deref() {
                        Some(provider) => format!("{model} · {}", provider_label(provider)),
                        None => model,
                    })
                }),
                tokens: used
                    .filter(|u| u.total_tokens > 0)
                    .map(|u| format!("{} tok", format_tokens(u.total_tokens))),
            }
        })
        .collect()
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

    /// Worker ids named by swarm calls in this transcript, so the workspace
    /// reads usage only for workers the user can actually see.
    pub(crate) fn swarm_card_worker_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
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

    pub(super) fn render_swarm_card(
        &self,
        index: usize,
        call_id: &str,
        call: &Call,
        done: bool,
        output: &str,
        error: Option<&str>,
        offset: f32,
        opacity: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let failed = error.is_some();
        let rows = rows(call, &self.swarm_workers, &self.swarm_usage);
        let summary = match (&call.intent, rows.is_empty()) {
            (Some(intent), _) => format!("{} · {}", call.action, intent),
            (None, _) => call.action.clone(),
        };
        let lines = rows
            .into_iter()
            .enumerate()
            .map(|(row_index, row)| {
                let id = row.session_id.clone();
                let mark = row.state.glyph().map(|glyph| {
                    self.swarm_card_marks.borrow_mut().element(
                        &format!("card:{call_id}:{}", row.session_id),
                        Mark::glyph(glyph, 11.0),
                        cx,
                    )
                });
                let detail = [Some(row.status.clone()), row.model.clone(), row.tokens.clone()]
                    .into_iter()
                    .flatten()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                div()
                    .id(("swarm-card-worker", index * 1000 + row_index))
                    .debug_selector(move || format!("swarm-card-worker-{row_index}"))
                    .ml(px(24.0))
                    .h(px(20.0))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w_0()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.PANEL_BG))
                    .text_size(px(12.0))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(OpenSwarmWorker(id.clone()));
                    }))
                    .child(div().flex_none().w(px(14.0)).children(row.icon))
                    .child(div().flex_none().size(px(11.0)).children(mark))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(280.0))
                            .truncate()
                            .text_color(theme.TOOL_TEXT)
                            .child(row.title),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("swarm-card-detail-{row_index}"))
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(theme.TEXT_FAINT)
                            .child(detail),
                    )
            })
            .collect::<Vec<_>>();
        // The card shows worker rows, not the call's own answer. Keep the raw
        // result and error reachable: a `list` call names no workers, so its
        // output is the whole answer, and a failed call explains itself only in
        // the error text.
        let raw = error
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| (text, true))
            .or_else(|| {
                let text = output.trim();
                (!text.is_empty()).then_some((text, false))
            });
        let expanded = self.expanded_regions.contains(call_id);
        let toggle_key = call_id.to_owned();
        let result_section = raw.map(|(text, is_error)| {
            let color = if is_error { theme.ERROR } else { theme.TOOL_TEXT };
            let preview =
                expandable::collapsed(text, expandable::ERROR).unwrap_or_else(|| text.to_owned());
            let body = expandable::expanded_body(text).0;
            div()
                .id(("swarm-card-result", index))
                .debug_selector(move || {
                    if is_error {
                        "tool-swarm-error".into()
                    } else {
                        "tool-swarm-result".into()
                    }
                })
                .flex_none()
                .flex()
                .flex_col()
                .gap_1()
                .ml(px(24.0))
                .px_1()
                .pt(px(2.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(color)
                                .child(if is_error { "error" } else { "result" }),
                        )
                        .when(!expanded, |el| {
                            el.child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(theme.TEXT_FAINT)
                                    .child(preview),
                            )
                        })
                        .when(expanded, |el| el.child(div().flex_1()))
                        .child(
                            div()
                                .id(("swarm-card-result-toggle", index))
                                .debug_selector(|| "tool-swarm-result-toggle".into())
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(12.0))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.TOOL_BORDER))
                                .text_size(px(9.0))
                                .line_height(px(12.0))
                                .text_color(theme.TEXT_FAINT)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    if !this.expanded_regions.remove(&toggle_key) {
                                        this.expanded_regions.insert(toggle_key.clone());
                                    }
                                    this.transcript_measurements.dirty = true;
                                    cx.notify();
                                }))
                                .child(if expanded { "▾" } else { "▸" }),
                        ),
                )
                .when(expanded, |el| {
                    el.child(
                        div()
                            .debug_selector(|| "tool-swarm-result-body".into())
                            .w_full()
                            .child(expandable::scrolled(
                                format!("swarm-card-result-body-{index}").into(),
                                expandable::MAX_HEIGHT,
                                div()
                                    .text_size(px(11.0))
                                    .text_color(color)
                                    .child(body)
                                    .into_any_element(),
                            )),
                    )
                })
        });
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
                    .debug_selector(|| "tool-swarm-header".into())
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .py(px(2.0))
                    .child(crate::tool_icon::render_badge("swarm", done, failed))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.0))
                            .text_color(theme.TOOL_TEXT)
                            .child(summary),
                    ),
            )
            .children(lines)
            .children(result_section)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROSE: &str = "session_rose_1794717290730_574aea8d9f6adcc3";
    const FOX: &str = "session_fox_1794718668160_87674d9271df588d";

    fn worker(id: &str, status: &str, label: Option<&str>) -> Worker {
        let session: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": id, "status": "idle", "parent_session_id": "lead",
            "swarm_status": status, "agent_label": label,
        }))
        .unwrap();
        Worker::from_session(&session, 0)
    }

    fn usage(model: &str, provider: &str, total: u64) -> WorkerUsage {
        WorkerUsage {
            model: Some(model.into()),
            provider: Some(provider.into()),
            total_tokens: total,
        }
    }

    #[test]
    fn spawn_names_the_new_worker_from_its_output() {
        let call = parse(
            "swarm",
            r#"{"action":"spawn","intent":"Spawn desktop voice worker","label":"desktop"}"#,
            &format!("Spawned new agent: {ROSE}\nLane lane::x: 1 todo item(s)"),
        )
        .unwrap();
        assert_eq!(call.action, "spawn");
        assert_eq!(call.session_ids, vec![ROSE]);
    }

    #[test]
    fn await_stop_and_message_name_workers_from_their_arguments() {
        let ids = |input: &str| parse("swarm", input, "").unwrap().session_ids;
        assert_eq!(
            ids(&format!(r#"{{"action":"await_members","session_ids":["{ROSE}","{FOX}","{ROSE}"]}}"#)),
            vec![ROSE, FOX]
        );
        assert_eq!(ids(&format!(r#"{{"action":"stop","target_session":"{FOX}"}}"#)), vec![FOX]);
        assert_eq!(ids(&format!(r#"{{"action":"message","to_session":"{ROSE}","message":"hi"}}"#)), vec![ROSE]);
        assert!(ids(r#"{"action":"list"}"#).is_empty());
        assert!(parse("bash", r#"{"action":"spawn"}"#, "").is_none());
    }

    /// User report: "swarm Rooster done 5s ~15 tok". The row now carries the
    /// worker's status, model, provider and TOTAL spend, and no `~` estimate of
    /// the call's own output.
    #[test]
    fn row_shows_status_model_provider_and_the_workers_total_tokens() {
        let call = parse("swarm", &format!(r#"{{"action":"await_members","session_ids":["{ROSE}"]}}"#), "")
            .unwrap();
        let workers = [worker(ROSE, "ready", Some("desktop-footer"))];
        let usage = HashMap::from([(ROSE.to_owned(), usage("claude-opus-5-5", "claude-oauth", 4_478_262))]);
        let rows = rows(&call, &workers, &usage);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.title, "desktop-footer");
        assert_eq!(row.status, "ready");
        assert_eq!(row.state.glyph(), Some(crate::swarm_view::Glyph::Check));
        assert_eq!(row.tokens.as_deref(), Some("4.5M tok"));
        let model = row.model.as_deref().unwrap();
        assert!(model.ends_with("· claude"), "{model}");
        assert!(!row.tokens.as_deref().unwrap().contains('~'));
        assert_eq!(row.icon, Some(jcode_core::id::session_icon("rose")));
    }

    #[test]
    fn unknown_workers_claim_no_status_and_unread_usage_shows_nothing() {
        let call = parse("swarm", &format!(r#"{{"action":"stop","target_session":"{FOX}"}}"#), "").unwrap();
        let rows = rows(&call, &[], &HashMap::new());
        assert_eq!(rows[0].title, "fox");
        assert_eq!(rows[0].state.glyph(), None);
        assert_eq!(rows[0].status, "");
        assert_eq!((rows[0].model.clone(), rows[0].tokens.clone()), (None, None));
    }

    #[gpui::test]
    fn swarm_call_renders_worker_rows_and_clicking_one_opens_it(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("lead", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sink = opened.clone();
        vcx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &OpenSwarmWorker, _| {
                sink.borrow_mut().push(event.0.clone());
            })
            .detach();
        });
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(
                vec![
                    worker(ROSE, "running", Some("desktop-footer")),
                    worker(FOX, "failed", None),
                ],
                cx,
            );
            panel.set_swarm_usage(
                HashMap::from([(ROSE.to_owned(), usage("claude-opus-5-5", "claude-oauth", 123_456))]),
                cx,
            );
            panel.items.push(Item::Tool {
                call_id: "c1".into(),
                name: "swarm".into(),
                input: format!(r#"{{"action":"await_members","session_ids":["{ROSE}","{FOX}"],"intent":"Wait both"}}"#),
                output: "Watching all of rose, fox in the background.".into(),
                done: true,
                error: None,
            });
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-swarm").is_some());
        assert!(vcx.debug_bounds("swarm-card-worker-0").is_some());
        assert!(vcx.debug_bounds("swarm-card-worker-1").is_some());
        assert!(
            vcx.debug_bounds("tool-output-size").is_none(),
            "no output-size badge on a swarm call"
        );
        assert_eq!(panel.read_with(vcx, |p, _| p.swarm_card_worker_ids()), vec![ROSE, FOX]);
        let second = vcx.debug_bounds("swarm-card-worker-1").unwrap();
        vcx.simulate_click(second.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert_eq!(opened.borrow().as_slice(), [FOX.to_string()]);
    }

    /// A parseable swarm call used to render only its action name plus worker
    /// rows. A `list` call names no workers, so its answer is the output, and a
    /// failed spawn's reason is the error: both must stay reachable.
    #[gpui::test]
    fn swarm_card_keeps_raw_output_and_error_reachable(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("lead", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        panel.update(vcx, |panel, cx| {
            panel.items.push(Item::Tool {
                call_id: "list-1".into(),
                name: "swarm".into(),
                input: r#"{"action":"list"}"#.into(),
                output: "3 workers: rose running, fox failed, owl ready".into(),
                done: true,
                error: None,
            });
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("tool-swarm").is_some(), "a list call still renders the card");
        assert!(
            vcx.debug_bounds("swarm-card-worker-0").is_none(),
            "a list call names no worker rows"
        );
        let toggle = vcx
            .debug_bounds("tool-swarm-result-toggle")
            .expect("the result of a list call stays reachable");
        assert!(
            vcx.debug_bounds("tool-swarm-result-body").is_none(),
            "the raw result starts collapsed"
        );
        vcx.simulate_click(toggle.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("tool-swarm-result-body").is_some(),
            "expanding shows the complete result"
        );

        // A failed spawn shows why it failed.
        panel.update(vcx, |panel, cx| {
            *panel.items = vec![Item::Tool {
                call_id: "spawn-1".into(),
                name: "swarm".into(),
                input: r#"{"action":"spawn","intent":"Spawn a worker"}"#.into(),
                output: String::new(),
                done: true,
                error: Some("swarm spawn failed: no free lane".into()),
            }];
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("tool-swarm-error").is_some(),
            "a failed spawn keeps its failure reason"
        );
    }
}
