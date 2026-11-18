//! The composer bar's swarm and activity marks. Replaces the old text strip:
//! a coordinator shows a small spinner plus a count while workers run, and
//! every panel shows one icon for what its agent is doing (idle, thinking,
//! running a tool, waiting on workers). Clicking the count lists the workers
//! with their status, and each row opens that worker's chat.

use super::*;
use crate::swarm_glyph::{Icon, Mark, Motion};
use crate::swarm_view::{Activity, Indicator, Turn, Worker};

/// A worker row (composer list, sidebar, or transcript card) was clicked.
pub(crate) struct OpenSwarmWorker(pub String);

impl gpui::EventEmitter<OpenSwarmWorker> for Panel {}

/// Rows listed before the rest fold into `+N`.
const MAX_ROWS: usize = 12;

impl Panel {
    /// This panel's own session record, when the workspace read one for it.
    /// Only swarm workers have one; an ordinary chat learns its identity from
    /// the runtime and ignores this.
    pub(crate) fn set_worker_record(
        &mut self,
        record: Option<jcode_sdk::WorkerMeta>,
        cx: &mut Context<Self>,
    ) {
        if self.worker_record != record {
            self.worker_record = record;
            cx.notify();
        }
    }

    pub(crate) fn set_swarm_workers(&mut self, workers: Vec<Worker>, cx: &mut Context<Self>) {
        if self.swarm_workers != workers {
            self.swarm_workers = workers;
            if self.swarm_indicator().is_none() {
                self.swarm_list_open = false;
            }
            cx.notify();
        }
    }

    /// The awaits this coordinator is parked on. They outlive the turn that
    /// registered them, so they are what keeps the agent from reading as idle.
    pub(crate) fn set_pending_awaits(
        &mut self,
        awaits: Vec<jcode_sdk::PendingAwait>,
        cx: &mut Context<Self>,
    ) {
        if self.pending_awaits != awaits {
            self.pending_awaits = awaits;
            cx.notify();
        }
    }

    /// Ids this coordinator is still waiting for, in request order,
    /// deduplicated. A worker that already finished (ready, stopped, failed)
    /// owes nothing more, so it drops out at once instead of lingering until
    /// the daemon resolves the whole await. Ids the worker list does not know
    /// stay: nothing says they are done.
    pub(crate) fn awaiting_ids(&self) -> Vec<String> {
        let finished = |id: &str| {
            self.swarm_workers
                .iter()
                .any(|worker| worker.session_id == id && worker.state.is_finished())
        };
        let mut ids = Vec::new();
        for await_ in &self.pending_awaits {
            for id in &await_.requested_ids {
                if !ids.contains(id) && !finished(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }

    /// `Waiting for kangaroo, fox · wakes when they report`, or nothing.
    /// Names come from the live worker list, then the session id.
    pub(crate) fn await_line(&self) -> Option<String> {
        let ids = self.awaiting_ids();
        if ids.is_empty() {
            return None;
        }
        let names = ids
            .iter()
            .map(|id| {
                self.swarm_workers
                    .iter()
                    .find(|worker| &worker.session_id == id)
                    .map(|worker| worker.name.clone())
                    .or_else(|| crate::swarm_view::friendly_name(id).map(str::to_owned))
                    .unwrap_or_else(|| id.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("Waiting: {names}"))
    }

    #[cfg(test)]
    pub(crate) fn test_swarm_indicator_count(&self) -> usize {
        self.swarm_indicator().map_or(0, |i| i.count())
    }

    pub(super) fn swarm_indicator(&self) -> Option<Indicator> {
        Indicator::from_workers(&self.swarm_workers)
    }

    /// What this agent is doing right now, for the bar's activity icon.
    pub(super) fn agent_activity(&self) -> Activity {
        let turn = if !self.activity_active() {
            Turn::Idle
        } else if self.status_line() == "Running tools" {
            Turn::Tools
        } else {
            Turn::Thinking
        };
        // A pending background await is live work even when every worker row
        // still carries a stale terminal status.
        let live = self
            .swarm_indicator()
            .map_or(0, |i| i.count())
            .max(usize::from(!self.awaiting_ids().is_empty()));
        Activity::resolve(turn, live)
    }

    /// The activity icon, present so a quiet agent reads as idle rather than
    /// missing. Waiting on workers is drawn inside the swarm pill, next to the
    /// count it is waiting on, so this slot steps aside for it.
    pub(super) fn render_activity_icon(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let theme = Theme::global();
        let activity = self.agent_activity();
        if activity == Activity::WaitingOnWorkers {
            return None;
        }
        let label = activity.label();
        let icon = self.swarm_marks.element(
            "activity",
            Mark::activity(activity, super::composer::BAR_ICON),
            cx,
        );
        Some(
            div()
                .id("composer-activity")
                .debug_selector(move || {
                    format!(
                        "composer-activity-{}",
                        label.to_ascii_lowercase().replace(' ', "-")
                    )
                })
                .flex_none()
                .h(px(super::composer::TAB_HEIGHT))
                .w(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.TEXT_FAINT)
                .tooltip(move |_, cx| cx.new(|_| super::usage::MeterTooltip(label.into())).into())
                .child(icon)
                .into_any_element(),
        )
    }

    /// The swarm pill: the worker cluster (the upstream sidebar's swarm mark,
    /// breathing while anything runs), the number of live workers (running or
    /// waiting on a dependency), and, when the coordinator is parked on them,
    /// the hourglass right beside that number. Finished workers are not
    /// counted: they are listed in the hover.
    pub(super) fn render_swarm_indicator(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let tally = crate::swarm_view::SwarmTally::of(&self.swarm_workers);
        let awaiting = self.awaiting_ids();
        if tally.total() == 0 && awaiting.is_empty() {
            return None;
        }
        let theme = Theme::global();
        let mut tooltip = Vec::new();
        if let Some(line) = self.await_line() {
            tooltip.push(line);
        }
        let summary = tally.summary();
        if !summary.is_empty() {
            tooltip.push(summary);
        }
        if let Some(indicator) = self.swarm_indicator() {
            tooltip.push(indicator.tooltip());
        }
        let tooltip = tooltip.join("\n");
        let open = self.swarm_list_open;
        let live = tally.running + tally.waiting;
        let waiting_on_workers = self.agent_activity() == Activity::WaitingOnWorkers;
        // Running breathes, the way the activity mark does while thinking;
        // a swarm parked on dependencies is warm and still; a finished one
        // is faint, there only to carry the hover.
        let (color, motion) = if tally.running > 0 {
            (theme.ACCENT, Motion::Pulse(Duration::from_millis(1600)))
        } else if live > 0 {
            (theme.WARN, Motion::Still)
        } else {
            (theme.TEXT_FAINT, Motion::Still)
        };
        let cluster = div()
            .debug_selector(|| "composer-workers-icon".into())
            .flex_none()
            .child(self.swarm_marks.element(
                "workers-cluster",
                Mark {
                    icon: Icon::Workers,
                    color,
                    size: super::composer::BAR_ICON,
                    motion,
                },
                cx,
            ));
        let count = (live > 0).then(|| {
            div()
                .debug_selector(|| "composer-workers-count".into())
                .child(live.to_string())
        });
        let wait = waiting_on_workers.then(|| {
            div()
                .debug_selector(|| "composer-workers-awaiting".into())
                .flex_none()
                .child(self.swarm_marks.element(
                    "workers-awaiting",
                    Mark::activity(Activity::WaitingOnWorkers, super::composer::BAR_ICON),
                    cx,
                ))
        });
        Some(
            super::composer::composer_pill("composer-workers")
                .flex_none()
                .px_2()
                .gap_1()
                .when(open, |el| el.bg(theme.chip_active_background()).text_color(theme.TEXT))
                .tooltip(move |_, cx| {
                    cx.new(|_| super::usage::RowsTooltip {
                        title: "Workers".into(),
                        rows: tooltip.lines().map(str::to_owned).collect(),
                    })
                    .into()
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.swarm_list_open = !this.swarm_list_open;
                    cx.notify();
                }))
                .child(cluster)
                .children(count)
                .children(wait)
                .into_any_element(),
        )
    }

    /// The list opened by the count: one row per live worker, click to open.
    pub(super) fn render_swarm_list(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.swarm_list_open {
            return None;
        }
        let indicator = self.swarm_indicator()?;
        let theme = Theme::global();
        let extra = indicator.workers.len().saturating_sub(MAX_ROWS);
        let rows = indicator
            .workers
            .iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(index, worker)| {
                let id = worker.session_id.clone();
                // A worker's task name can be a whole sentence. Keep one line
                // while collapsed and wrap it in place from the chevron.
                let expand_key = expandable::key::worker(&worker.session_id);
                let expanded = self.expanded_regions.contains(&expand_key);
                let toggle_key = expand_key.clone();
                let label_truncated = worker
                    .label
                    .as_deref()
                    .is_some_and(|label| expandable::preview(label, expandable::LABEL).is_some());
                let mark = worker.state.glyph().map(|glyph| {
                    self.swarm_marks.element(
                        &format!("worker:{}", worker.session_id),
                        Mark::glyph(glyph, 11.0),
                        cx,
                    )
                });
                // One line carries the identity: the animal stands in for the
                // name, the row's tooltip spells the name out, then the model,
                // what it has spent and what it is doing.
                let tooltip = worker.tooltip();
                let tooltip_name = worker.name.clone();
                let model = worker.model_label();
                let effort = worker
                    .effort_level()
                    .and_then(|effort| crate::effort_icon::icon(&effort, 11.0));
                let provider = crate::provider_mark::render(
                    worker.meta.provider.as_deref(),
                    worker.meta.model.as_deref(),
                    11.0,
                    theme.TEXT_FAINT,
                );
                let tokens = worker.tokens_text();
                let cost = worker.cost_text();
                div()
                    .id(("swarm-list-worker", index))
                    .debug_selector(move || format!("swarm-list-worker-{index}"))
                    .min_h(px(20.0))
                    .when(expanded, |el| el.items_start().flex_wrap())
                    .px_2()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.USER_BG))
                    .text_size(px(11.0))
                    .text_color(theme.TEXT_DIM)
                    .tooltip(move |_, cx| {
                        cx.new(|_| {
                            super::usage::RowsTooltip::separated(
                                tooltip_name.clone(),
                                tooltip
                                    .strip_prefix(&format!("{} · ", tooltip_name))
                                    .unwrap_or(&tooltip),
                            )
                        })
                        .into()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.swarm_list_open = false;
                        cx.emit(OpenSwarmWorker(id.clone()));
                        cx.notify();
                    }))
                    .child(div().flex_none().size(px(11.0)).children(mark))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.5))
                            .children(worker.icon.map(|icon| icon.to_owned())),
                    )
                    .children(worker.label.clone().map(|label| {
                        div()
                            .min_w_0()
                            .when(!expanded, |el| el.truncate())
                            .when(expanded, |el| el.max_w_full())
                            .text_color(theme.TEXT_FAINT)
                            .child(label)
                    }))
                    .when(label_truncated, |el| {
                        let key = toggle_key.clone();
                        el.child(expandable::glyph_toggle(
                            format!("swarm-list-worker-expand-{index}").into(),
                            expanded,
                            cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                expandable::flip(&mut this.expanded_regions, &key);
                                this.transcript_measurements.dirty = true;
                                cx.notify();
                            }),
                        ))
                    })
                    .child(div().flex_1())
                    .children(model.map(|model| {
                        div()
                            .debug_selector(move || format!("swarm-list-model-{index}"))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(3.0))
                            .text_color(theme.TEXT_FAINT)
                            .child(provider)
                            .child(model)
                            // Same circle as the composer pill, right after the
                            // model it applies to.
                            .children(effort.map(|effort| {
                                div()
                                    .debug_selector(move || format!("swarm-list-effort-{index}"))
                                    .flex_none()
                                    // A drawn rung in its own heat tint.
                                    .child(effort)
                            }))
                    }))
                    .children(tokens.map(|tokens| {
                        div()
                            .debug_selector(move || format!("swarm-list-tokens-{index}"))
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(tokens)
                    }))
                    .children(cost.map(|cost| {
                        div()
                            .debug_selector(move || format!("swarm-list-cost-{index}"))
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(cost)
                    }))
                    .child(
                        div()
                            .debug_selector(move || format!("swarm-list-state-{index}"))
                            .flex_none()
                            .text_color(theme.TEXT_FAINT)
                            .child(worker.status_text()),
                    )
            })
            .collect::<Vec<_>>();
        Some(
            div()
                .debug_selector(|| "swarm-list".into())
                .w_full()
                .min_w_0()
                .mb(px(4.0))
                .p_1()
                .flex()
                .flex_col()
                .rounded_lg()
                .bg(theme.HEADER_BG)
                .border_1()
                .border_color(theme.PANEL_BORDER)
                .children(rows)
                .when(extra > 0, |el| {
                    el.child(
                        div()
                            .px_2()
                            .text_size(px(10.5))
                            .text_color(theme.TEXT_FAINT)
                            .child(format!("+{extra} more")),
                    )
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(cx: &mut gpui::TestAppContext) -> (Entity<Panel>, &mut gpui::VisualTestContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace =
                crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("activity-bar", cx);
            workspace
        });
        let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
        (panel, vcx)
    }

    fn worker(name: &str, status: &str) -> Worker {
        let session: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": format!("session_{name}_1_a"), "status": "idle",
            "parent_session_id": "lead", "swarm_status": status,
        }))
        .unwrap();
        Worker::from_session(&session, None, 0)
    }

    fn event(value: serde_json::Value) -> ApiEvent {
        let mut value = value;
        value["session_id"] = serde_json::json!("activity-bar");
        serde_json::from_value(value).unwrap()
    }

    #[gpui::test]
    fn activity_icon_tracks_idle_thinking_tool_and_waiting(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let selector = |label: &str| -> &'static str {
            Box::leak(format!("composer-activity-{label}").into_boxed_str())
        };
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector("idle")).is_some(), "idle at rest");

        panel.update(vcx, |panel, cx| {
            panel.apply(
                &event(serde_json::json!({"ev":"reasoning_delta","text":"hm"})),
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector("thinking")).is_some());

        panel.update(vcx, |panel, cx| {
            panel.apply(
                &event(serde_json::json!({"ev":"tool_start","call_id":"c1","name":"bash"})),
                cx,
            )
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector("running-tool")).is_some());

        panel.update(vcx, |panel, cx| {
            panel.apply(
                &ApiEvent::SessionStatus {
                    session_id: "activity-bar".into(),
                    status: "idle".into(),
                    pending_soft_interrupts: None,
                },
                cx,
            );
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        // Idle with a live worker is waiting, not hung. The wait mark is drawn
        // inside the swarm pill, beside the count it waits on.
        assert!(
            vcx.debug_bounds("composer-workers-awaiting").is_some(),
            "idle with a live worker is waiting, not hung"
        );
        assert!(vcx.debug_bounds(selector("waiting-on-workers")).is_none());
        assert!(vcx.debug_bounds("composer-workers").is_some());

        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "ready")], cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector("idle")).is_some());
        // The worker reported, so the bar no longer claims anything runs: no
        // live count, no wait mark.
        assert!(vcx.debug_bounds("composer-workers-count").is_none());
        assert!(vcx.debug_bounds("composer-workers-awaiting").is_none());
    }

    #[gpui::test]
    fn stalled_worker_is_visible_in_the_live_swarm_list(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        std::fs::write(dir.path().join("swarm.json"), serde_json::json!({
            "updated_at_unix_ms": now,
            "members": [{"session_id":"session_rose_1_a", "status":"running", "stalled_secs":601, "report_back_to_session_id":"lead"}]
        }).to_string()).unwrap();
        let mut sessions: Vec<jcode_sdk::SessionInfo> =
            serde_json::from_value(serde_json::json!([{
                "session_id":"session_rose_1_a", "status":"idle"
            }]))
            .unwrap();
        jcode_sdk::enrich_sessions_from_swarm_state(&mut sessions, dir.path());
        let stalled = Worker::from_session(&sessions[0], None, now);
        assert_eq!(stalled.state_text(), "stalled");
        assert!(
            stalled.state.is_live(),
            "a stalled worker still belongs to the live swarm"
        );
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![stalled], cx);
            panel.swarm_list_open = true;
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list-state-0").is_some());
        assert!(vcx.debug_bounds("composer-workers-count").is_some());
    }

    #[gpui::test]
    fn closing_the_last_live_worker_closes_the_list(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
            panel.swarm_list_open = true;
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list").is_some());
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "ready")], cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list").is_none());
    }

    /// The reload sequence: every worker row still carries the pre-reload word
    /// (`ready`), so nothing is live by state and the coordinator would read
    /// idle. The persisted await is what says it is waiting, and for whom.
    #[gpui::test]
    fn a_pending_background_await_names_the_workers_and_keeps_the_agent_waiting(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = setup(cx);
        assert!(
            vcx.debug_bounds("composer-workers-awaiting").is_none(),
            "no await, no wait mark"
        );
        // kangaroo is not in the reloaded worker list at all, so nothing says
        // it finished; fox finished before the reload.
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("fox", "ready")], cx);
            panel.set_pending_awaits(
                vec![jcode_sdk::PendingAwait {
                    requested_ids: vec!["session_kangaroo_1_a".into()],
                    deadline_unix_ms: u64::MAX,
                }],
                cx,
            );
        });
        vcx.run_until_parked();
        assert_eq!(
            panel
                .read_with(vcx, |panel, _| panel.await_line())
                .as_deref(),
            Some("Waiting: kangaroo")
        );
        // The information the removed text line carried now lives in the
        // pill: a pause mark in the bar and the names in the hover.
        assert!(
            vcx.debug_bounds("composer-workers-awaiting").is_some(),
            "the pill says the coordinator is waiting"
        );
        assert!(
            vcx.debug_bounds("composer-workers").is_some(),
            "no live worker, but the await still earns a pill"
        );
        assert!(
            vcx.debug_bounds("composer-activity-idle").is_none(),
            "a parked coordinator is waiting, not idle"
        );

        // The wake delivered: the mark goes away with the registration.
        panel.update(vcx, |panel, cx| panel.set_pending_awaits(Vec::new(), cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-workers-awaiting").is_none());
        assert!(vcx.debug_bounds("composer-activity-idle").is_some());
    }

    /// "Waiting for kangaroo, hedgehog, calf, sunflower" kept naming kangaroo
    /// and hedgehog after both had finished and stopped. A finished member
    /// drops out of the wait at once; when every member is finished, the
    /// coordinator is no longer waiting at all.
    #[gpui::test]
    fn finished_workers_drop_out_of_a_pending_await(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let ids = ["kangaroo", "hedgehog", "calf", "sunflower"]
            .map(|name| format!("session_{name}_1_a"))
            .to_vec();
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(
                vec![
                    worker("kangaroo", "ready"),
                    worker("hedgehog", "stopped"),
                    worker("calf", "running"),
                    worker("sunflower", "running"),
                ],
                cx,
            );
            panel.set_pending_awaits(
                vec![jcode_sdk::PendingAwait {
                    requested_ids: ids.clone(),
                    deadline_unix_ms: u64::MAX,
                }],
                cx,
            );
        });
        vcx.run_until_parked();
        assert_eq!(
            panel
                .read_with(vcx, |panel, _| panel.await_line())
                .as_deref(),
            Some("Waiting: calf, sunflower")
        );
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(
                vec![
                    worker("kangaroo", "ready"),
                    worker("hedgehog", "stopped"),
                    worker("calf", "failed"),
                    worker("sunflower", "ready"),
                ],
                cx,
            )
        });
        vcx.run_until_parked();
        assert_eq!(panel.read_with(vcx, |panel, _| panel.await_line()), None);
        assert!(vcx.debug_bounds("composer-workers-awaiting").is_none());
    }

    /// The row the user reported as "poodle compact-div… running 23m": the
    /// animal stands in for the name, then the task, the model, what the
    /// worker has spent and what it is doing right now.
    #[gpui::test]
    fn worker_rows_carry_the_model_spend_and_live_state(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let mut row = worker("poodle", "running");
        row.label = Some("compact-divider".into());
        row.meta = jcode_sdk::WorkerMeta {
            model: Some("deepseek-v4.1-flash".into()),
            provider: Some("opencode-go".into()),
            effort: Some("high".into()),
            input_tokens: Some(12_400),
            output_tokens: Some(3_100),
            phase: Some(jcode_sdk::WorkerPhase::Tool),
            tool: Some("bash".into()),
            last_activity_ms: Some(0),
            ..jcode_sdk::WorkerMeta::default()
        };
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![row], cx);
            panel.swarm_list_open = true;
        });
        vcx.run_until_parked();
        for selector in [
            "swarm-list-worker-0",
            "swarm-list-model-0",
            "swarm-list-effort-0",
            "swarm-list-tokens-0",
            "swarm-list-state-0",
        ] {
            assert!(vcx.debug_bounds(selector).is_some(), "{selector}");
        }
        // The cost cell appears exactly when the route is priced: a machine
        // with no models.dev cache shows tokens and no dollars, never "$0.00".
        let priced =
            jcode_base::model_pricing::lookup("opencode-go", "deepseek-v4.1-flash").is_some();
        assert_eq!(vcx.debug_bounds("swarm-list-cost-0").is_some(), priced);
        // The row is one line: the model is to the right of the task label and
        // the state is the last cell.
        let label = vcx.debug_bounds("swarm-list-model-0").unwrap();
        let state = vcx.debug_bounds("swarm-list-state-0").unwrap();
        assert!(label.right() <= state.left(), "model sits before the state");
        // The reasoning rung rides directly after the model it belongs to.
        let effort = vcx.debug_bounds("swarm-list-effort-0").unwrap();
        assert!(
            label.left() < effort.left() && effort.right() <= state.left(),
            "effort sits after the model and before the state: {label:?} {effort:?} {state:?}"
        );
    }

    /// A worker whose record names no effort shows no circle; the row must not
    /// claim the default rung.
    #[gpui::test]
    fn worker_rows_without_effort_show_no_circle(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let mut row = worker("poodle", "running");
        row.meta = jcode_sdk::WorkerMeta {
            model: Some("deepseek-v4.1-flash".into()),
            provider: Some("opencode-go".into()),
            ..jcode_sdk::WorkerMeta::default()
        };
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![row], cx);
            panel.swarm_list_open = true;
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list-model-0").is_some());
        assert!(vcx.debug_bounds("swarm-list-effort-0").is_none());
    }

    fn inside(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) -> bool {
        inner.left() >= outer.left()
            && inner.right() <= outer.right()
            && inner.top() >= outer.top()
            && inner.bottom() <= outer.bottom()
    }

    /// The `38 ◦ ▮` report: finished workers inflated the count and the pill
    /// had no worker icon. The number is live workers only (running plus
    /// waiting), the drawn swarm mark leads, and the done ones live in the
    /// hover.
    #[gpui::test]
    fn the_pill_counts_live_workers_only_behind_a_drawn_swarm_icon(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let mut workers = vec![
            worker("rose", "running"),
            worker("tulip", "running"),
            worker("fox", "blocked"),
        ];
        for index in 0..35 {
            workers.push(worker(&format!("done{index}"), "ready"));
        }
        panel.update(vcx, |panel, cx| panel.set_swarm_workers(workers, cx));
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").expect("pill");
        let icon = vcx.debug_bounds("composer-workers-icon").expect("icon");
        let count = vcx.debug_bounds("composer-workers-count").expect("count");
        assert!(inside(icon, pill) && inside(count, pill));
        assert!(icon.right() <= count.left(), "the icon leads the count");
        assert_eq!(
            panel.read_with(vcx, |panel, _| {
                let tally = crate::swarm_view::SwarmTally::of(&panel.swarm_workers);
                tally.running + tally.waiting
            }),
            3
        );
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.test_swarm_indicator_count()),
            3
        );
        // The old segment bar is gone: no text-ish blocks after the number.
        for gone in [
            "composer-workers-bar",
            "composer-workers-done",
            "composer-workers-waiting",
        ] {
            assert!(vcx.debug_bounds(gone).is_none(), "{gone} must not render");
        }
    }

    /// The hourglass used to sit apart from the pill, further right. It is
    /// drawn inside the pill, right after the count, while the coordinator
    /// waits; the separate activity slot steps aside.
    #[gpui::test]
    fn the_wait_mark_sits_inside_the_swarm_pill_beside_the_count(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
            panel.set_pending_awaits(
                vec![jcode_sdk::PendingAwait {
                    requested_ids: vec!["session_rose_1_a".into()],
                    deadline_unix_ms: u64::MAX,
                }],
                cx,
            );
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").expect("pill");
        let count = vcx.debug_bounds("composer-workers-count").expect("count");
        let wait = vcx.debug_bounds("composer-workers-awaiting").expect("wait");
        assert!(
            inside(wait, pill),
            "wait mark inside the pill: {wait:?} {pill:?}"
        );
        assert!(count.right() <= wait.left(), "wait mark follows the count");
        assert!(
            vcx.debug_bounds("composer-activity-waiting-on-workers")
                .is_none(),
            "no second hourglass outside the pill"
        );
    }
}
