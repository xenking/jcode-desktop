//! The composer bar's swarm and activity marks. Replaces the old text strip:
//! a coordinator shows a small spinner plus a count while workers run, and
//! every panel shows one icon for what its agent is doing (idle, thinking,
//! running a tool, waiting on workers). Clicking the count lists the workers
//! with their status, and each row opens that worker's chat.

use super::*;
use crate::swarm_glyph::Mark;
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
        let live = self.swarm_indicator().map_or(0, |i| i.count());
        Activity::resolve(turn, live)
    }

    /// The activity icon, always present so a quiet agent reads as idle
    /// rather than missing.
    pub(super) fn render_activity_icon(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let activity = self.agent_activity();
        let label = activity.label();
        let icon = self.swarm_marks.element(
            "activity",
            Mark::activity(activity, 13.0),
            cx,
        );
        div()
            .id("composer-activity")
            .debug_selector(move || format!("composer-activity-{}", label.to_ascii_lowercase().replace(' ', "-")))
            .flex_none()
            .h(px(super::composer::TAB_HEIGHT))
            .w(px(18.0))
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.TEXT_FAINT)
            .tooltip(move |_, cx| cx.new(|_| super::usage::MeterTooltip(label.into())).into())
            .child(icon)
            .into_any_element()
    }

    /// Spinner plus the live worker count. Nothing when no worker is live.
    pub(super) fn render_swarm_indicator(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let indicator = self.swarm_indicator()?;
        let theme = Theme::global();
        let tooltip = indicator.tooltip();
        let count = indicator.count();
        let open = self.swarm_list_open;
        let icon = self.swarm_marks.element(
            "workers",
            Mark::glyph(indicator.glyph(), 12.0),
            cx,
        );
        Some(
            super::composer::composer_pill("composer-workers")
                .flex_none()
                .px_2()
                .gap_1()
                .when(open, |el| el.bg(theme.USER_BG).text_color(theme.TEXT))
                .tooltip(move |_, cx| cx.new(|_| super::usage::MeterTooltip(tooltip.clone())).into())
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.swarm_list_open = !this.swarm_list_open;
                    cx.notify();
                }))
                .child(icon)
                .child(
                    div()
                        .debug_selector(|| "composer-workers-count".into())
                        .child(count.to_string()),
                )
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
                let label_truncated = worker.label.as_deref().is_some_and(|label| {
                    expandable::preview(label, expandable::LABEL).is_some()
                });
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
                let model = worker.model_label();
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
                        cx.new(|_| super::usage::MeterTooltip(tooltip.clone())).into()
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
            panel.apply(&event(serde_json::json!({"ev":"reasoning_delta","text":"hm"})), cx)
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
        assert!(
            vcx.debug_bounds(selector("waiting-on-workers")).is_some(),
            "idle with a live worker is waiting, not hung"
        );
        assert!(vcx.debug_bounds("composer-workers").is_some());

        panel.update(vcx, |panel, cx| panel.set_swarm_workers(vec![worker("rose", "ready")], cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector("idle")).is_some());
        assert!(vcx.debug_bounds("composer-workers").is_none(), "nothing when none run");
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
        panel.update(vcx, |panel, cx| panel.set_swarm_workers(vec![worker("rose", "ready")], cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list").is_none());
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
    }
}
