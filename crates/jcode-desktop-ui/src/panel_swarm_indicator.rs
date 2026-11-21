//! The composer bar's swarm and activity marks. Replaces the old text strip:
//! a coordinator shows a small spinner plus a count while workers run, and
//! every panel shows one icon for what its agent is doing (idle, thinking,
//! running a tool, waiting on workers). Clicking the count lists the workers
//! with their status, and each row opens that worker's chat.

use super::*;
use crate::input::popup::{self, Popup};
use crate::swarm_glyph::{Icon, Mark, Motion};
use crate::swarm_view::{Activity, Indicator, Turn, Worker};

const AWAIT_RECHECK_MS: u64 = 3_000;

fn await_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// A worker row (composer list, sidebar, or transcript card) was clicked.
pub(crate) struct OpenSwarmWorker(pub String);

impl gpui::EventEmitter<OpenSwarmWorker> for Panel {}

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
            if self.swarm_workers.is_empty() && self.background_jobs.tasks.is_empty() {
                self.swarm_list_open = false;
            }
            cx.notify();
        }
    }

    /// The awaits this coordinator is parked on. They outlive the turn that
    /// registered them, so they are what keeps the agent from reading as idle.
    pub(crate) fn set_pending_awaits(
        &mut self,
        mut awaits: Vec<jcode_sdk::PendingAwait>,
        cx: &mut Context<Self>,
    ) {
        let now = await_now_ms();
        awaits.retain(|await_| await_.deadline_unix_ms > now);
        if self.pending_awaits != awaits {
            self.pending_awaits = awaits;
            self.pending_await_refresh = None;
            cx.notify();
        }
        if self.pending_await_refresh.is_some() {
            return;
        }
        let Some(deadline) = self
            .pending_awaits
            .iter()
            .map(|await_| await_.deadline_unix_ms)
            .min()
        else {
            return;
        };
        // Worker/catalog notifications may never arrive for an unknown member.
        // Track local registrations until they resolve, no later than the deadline.
        let delay = Duration::from_millis((deadline - now).min(AWAIT_RECHECK_MS));
        self.schedule_pending_await_refresh(delay, cx);
    }

    pub(crate) fn refresh_local_pending_awaits(&mut self, cx: &mut Context<Self>) {
        if crate::harness::remote_host(&self.session_id).is_none() {
            self.schedule_pending_await_refresh(Duration::ZERO, cx);
        }
    }

    fn schedule_pending_await_refresh(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let session_id = self.session_id.clone();
        self.pending_await_refresh = Some(cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(delay).await;
            let awaits = cx
                .background_executor()
                .spawn(async move { jcode_sdk::pending_awaits_for_session(&session_id) })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.pending_await_refresh = None;
                panel.set_pending_awaits(awaits, cx);
            });
        }));
    }

    /// Ids this coordinator is still waiting for, in request order,
    /// deduplicated. A worker that already finished (ready, stopped, failed)
    /// owes nothing more, so it drops out at once instead of lingering until
    /// the daemon resolves the whole await. Ids the worker list does not know
    /// stay: nothing says they are done.
    pub(crate) fn awaiting_ids(&self) -> Vec<String> {
        if self.pending_awaits.is_empty() {
            return Vec::new();
        }
        let finished = |id: &str| {
            self.swarm_workers
                .iter()
                .any(|worker| worker.session_id == id && worker.state.is_finished())
        };
        let mut ids = Vec::new();
        let now = await_now_ms();
        for await_ in &self.pending_awaits {
            if await_.deadline_unix_ms <= now {
                continue;
            }
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
                .text_color(theme.composer_secondary())
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
        let jobs = self.background_jobs.active();
        let job_total = self.background_jobs.tasks.len();
        if self.swarm_workers.is_empty() && awaiting.is_empty() && job_total == 0 {
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
        if job_total > 0 {
            tooltip.push(format!(
                "Background tasks: {jobs} active, {} finished",
                job_total - jobs
            ));
        }
        let tooltip = tooltip.join("\n");
        let open = self.swarm_list_open;
        let live = tally.running + tally.waiting + jobs;
        let waiting_on_workers = self.agent_activity() == Activity::WaitingOnWorkers;
        // Running breathes, the way the activity mark does while thinking;
        // a swarm parked on dependencies is warm and still; a finished one
        // is faint, there only to carry the hover.
        let (color, motion) = if tally.running + jobs > 0 {
            (theme.ACCENT, Motion::Pulse(Duration::from_millis(1600)))
        } else if live > 0 {
            (theme.WARN, Motion::Still)
        } else {
            (theme.composer_secondary(), Motion::Still)
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
            crate::keyboard::popover_trigger(
                super::composer::composer_pill("composer-workers"),
                &self.swarm_trigger_focus,
            )
            .flex_none()
            .px_2()
            .gap_1()
            .when(open, |el| {
                el.bg(theme.chip_active_background()).text_color(theme.TEXT)
            })
            .tooltip(move |_, cx| {
                cx.new(|_| super::usage::RowsTooltip {
                    title: (if job_total > 0 {
                        "Active work"
                    } else {
                        "Workers"
                    })
                    .into(),
                    rows: tooltip.lines().map(str::to_owned).collect(),
                })
                .into()
            })
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                if this.swarm_list_open {
                    this.close_swarm_to_trigger(window, cx);
                } else {
                    if this.picker_search.take().is_some() {
                        this.model_picker_open = false;
                    }
                    this.composer_popover_trigger = Some(this.swarm_trigger_focus.clone());
                    this.swarm_list_open = true;
                    this.swarm_finished_open = false;
                    this.swarm_list_scroll
                        .set_offset(gpui::point(px(0.), px(0.)));
                    window.focus(&this.swarm_list_focus, cx);
                }
                cx.notify();
            }))
            .children(crate::fixture_control_targets::marker(
                || "swarm-status".into(),
                || Some(self.session_id.clone()),
            ))
            .child(cluster)
            .children(count)
            .children(wait)
            .into_any_element(),
        )
    }

    pub(super) fn close_swarm_to_trigger(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_composer_popover(&self.swarm_list_focus.clone(), false, window, cx);
        window.focus(&self.swarm_trigger_focus, cx);
    }

    /// The list opened by the count: worker chats and session background tasks.
    pub(super) fn render_swarm_list(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.swarm_list_open {
            return None;
        }
        let mut workers: Vec<_> = self
            .swarm_workers
            .iter()
            .filter(|worker| worker.state.is_live())
            .cloned()
            .collect();
        workers.sort_by_key(|worker| !worker.state.is_active());
        let active_count = workers.len();
        let finished = self
            .swarm_workers
            .iter()
            .filter(|worker| worker.state.is_finished());
        let finished_count = finished.clone().count();
        if self.swarm_finished_open {
            workers.extend(finished.cloned());
        }
        if active_count + finished_count == 0 && self.background_jobs.tasks.is_empty() {
            return None;
        }
        let theme = Theme::global();
        let jobs = self.background_jobs.tasks.clone();
        let owner = cx.weak_entity();
        let focus = self.swarm_list_focus.clone();
        let key_focus = focus.clone();
        self.swarm_picker_rows
            .resize_with(workers.len(), || cx.focus_handle());
        let mut worker_rows = workers
            .iter()
            .enumerate()
            .map(|(index, worker)| {
                let id = worker.session_id.clone();
                let label = worker
                    .row_title()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
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
                    theme.composer_secondary(),
                );
                let tokens = worker.tokens_text();
                let cost = worker.cost_text();
                crate::keyboard::tracked_button(
                    div().id(("swarm-list-worker", index)),
                    &self.swarm_picker_rows[index],
                )
                .debug_selector(move || format!("swarm-list-worker-{index}"))
                .flex_none()
                .min_h(px(22.0))
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .px_2()
                .flex()
                .items_center()
                .gap(px(5.0))
                .rounded_full()
                .cursor_pointer()
                .hover(|el| el.bg(theme.USER_BG))
                .text_size(px(11.0))
                .text_color(theme.composer_secondary())
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.dismiss_composer_popover(&this.swarm_list_focus.clone(), true, window, cx);
                    cx.emit(OpenSwarmWorker(id.clone()));
                }))
                .children(crate::fixture_control_targets::marker(
                    || format!("status-worker:{}", worker.session_id),
                    || Some(self.session_id.clone()),
                ))
                .child(
                    div()
                        .debug_selector(move || format!("swarm-list-mark-{index}"))
                        .flex_none()
                        .size(px(11.0))
                        .children(mark),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .children(worker.icon.map(|icon| icon.to_owned())),
                )
                .child(
                    div()
                        .debug_selector(move || format!("swarm-list-title-{index}"))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.TEXT)
                        .child(label),
                )
                .children(model.map(|model| {
                    div()
                        .debug_selector(move || format!("swarm-list-model-{index}"))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .child(provider)
                        .child(div().min_w_0().truncate().child(model))
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
                        .child(tokens)
                }))
                .children(cost.map(|cost| {
                    div()
                        .debug_selector(move || format!("swarm-list-cost-{index}"))
                        .flex_none()
                        .child(cost)
                }))
                .child(
                    div()
                        .debug_selector(move || format!("swarm-list-state-{index}"))
                        .flex_none()
                        .max_w(gpui::relative(0.3))
                        .truncate()
                        .child(worker.status_text()),
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        let mut row_focus = Vec::new();
        let mut scroll_indices = Vec::new();
        if active_count + finished_count > 0 {
            rows.push(
                work_section("Workers", format!("Workers · {active_count}"), false)
                    .into_any_element(),
            );
            scroll_indices.extend(rows.len()..rows.len() + active_count);
            row_focus.extend_from_slice(&self.swarm_picker_rows[..active_count]);
            rows.extend(worker_rows.drain(..active_count));
        }
        if finished_count > 0 {
            let arrow = if self.swarm_finished_open {
                '▾'
            } else {
                '▸'
            };
            scroll_indices.push(rows.len());
            row_focus.push(self.swarm_finished_focus.clone());
            rows.push(
                crate::keyboard::tracked_button(
                    work_section(
                        "Finished workers",
                        format!("{arrow} Finished workers · {finished_count}"),
                        active_count > 0,
                    )
                    .id("swarm-finished-toggle"),
                    &self.swarm_finished_focus,
                )
                .cursor_pointer()
                .rounded_md()
                .hover(|el| el.bg(theme.USER_BG))
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.swarm_finished_open = !this.swarm_finished_open;
                    cx.notify();
                }))
                .children(crate::fixture_control_targets::marker(
                    || "finished-workers".into(),
                    || Some(self.session_id.clone()),
                ))
                .into_any_element(),
            );
            scroll_indices.extend(rows.len()..rows.len() + worker_rows.len());
            row_focus.extend_from_slice(&self.swarm_picker_rows[active_count..]);
            rows.extend(worker_rows);
        }
        if !jobs.is_empty() {
            rows.push(
                work_section(
                    "Background tasks",
                    format!("Background tasks · {}", jobs.len()),
                    !rows.is_empty(),
                )
                .into_any_element(),
            );
        }
        let detail_index = rows.len() + jobs.len();
        for (index, job) in jobs.iter().enumerate() {
            let focus = self
                .background_jobs
                .row_focus
                .entry(job.task_id.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone();
            scroll_indices.push(rows.len());
            rows.push(self.render_background_job_row(index, job, &focus, detail_index, cx));
            row_focus.push(focus);
        }
        let scroll = self.swarm_list_scroll.clone();
        let limit = popup::scroll_height(window.viewport_size().height, false);
        Some(
            gpui::deferred(
                Popup::new(
                    div()
                        .id("swarm-list")
                        .debug_selector(|| "swarm-list".into())
                        .track_focus(&self.swarm_list_focus)
                        .on_key_down(cx.listener(
                            move |panel, event: &gpui::KeyDownEvent, window, cx| {
                                if key_focus.is_focused(window)
                                    && event.keystroke.modifiers == gpui::Modifiers::default()
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    panel.close_swarm_to_trigger(window, cx);
                                    cx.stop_propagation();
                                    return;
                                }
                                let tab_peer = crate::keyboard::tab_direction(event)
                                    .and_then(|reverse| {
                                        if key_focus.is_focused(window) {
                                            return if reverse {
                                                row_focus.last()
                                            } else {
                                                row_focus.first()
                                            };
                                        }
                                        let current = row_focus
                                            .iter()
                                            .position(|row| row.is_focused(window))?;
                                        let peer = if reverse {
                                            current.checked_sub(1)?
                                        } else {
                                            current + 1
                                        };
                                        row_focus.get(peer)
                                    })
                                    .cloned();
                                if let Some(next) = tab_peer.or_else(|| {
                                    crate::keyboard::arrow_peer(event, &row_focus, window, cx)
                                }) {
                                    if let Some(index) =
                                        row_focus.iter().position(|row| row == &next)
                                    {
                                        scroll.scroll_to_item(if index == 0 {
                                            0
                                        } else {
                                            scroll_indices[index]
                                        });
                                    }
                                    window.focus(&next, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .w_full()
                        .min_w_0()
                        .max_h_full()
                        .p_1()
                        .flex()
                        .flex_col()
                        .rounded_xl()
                        .bg(theme.HEADER_BG)
                        .border_1()
                        .border_color(theme.PANEL_BORDER_FOCUS)
                        .shadow_lg()
                        .occlude()
                        .child(
                            div()
                                .id("swarm-list-scroll")
                                .debug_selector(|| "swarm-list-scroll".into())
                                .track_scroll(&self.swarm_list_scroll)
                                .max_h(limit)
                                .min_h_0()
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .children(rows)
                                .children(self.render_background_job_detail(cx)),
                        ),
                    px(5.),
                )
                .at_composer()
                .dismiss_outside(Some(std::rc::Rc::new(move |window, cx| {
                    let _ = owner.update(cx, |panel, cx| {
                        panel.dismiss_composer_popover(&focus, true, window, cx);
                    });
                }))),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

fn work_section(label: &'static str, title: String, separated: bool) -> gpui::Div {
    let theme = Theme::global();
    div()
        .debug_selector(move || format!("work-section-{label}"))
        .flex_none()
        .min_w_0()
        .px_2()
        .py(px(3.0))
        .when(separated, |header| {
            header.mt_1().border_t_1().border_color(theme.PANEL_BORDER)
        })
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.composer_secondary())
        .truncate()
        .child(title)
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

    fn press(vcx: &mut gpui::VisualTestContext, key: &str) {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        vcx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        vcx.simulate_event(gpui::KeyUpEvent { keystroke });
        vcx.run_until_parked();
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
    fn swarm_popover_dismisses_with_escape_and_outside_click(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        for escape in [true, false] {
            let pill = vcx.debug_bounds("composer-workers").expect("workers pill");
            vcx.simulate_click(pill.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("swarm-list").is_some());
            if escape {
                vcx.simulate_keystrokes("escape");
            } else {
                let input = vcx.debug_bounds("prompt-input").expect("composer");
                vcx.simulate_click(input.center(), gpui::Modifiers::default());
            }
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("swarm-list").is_none());
        }
    }

    #[gpui::test]
    fn swarm_overlay_preserves_the_transcript_viewport_and_scroll(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            *panel.items = (0..80)
                .map(|n| Item::Assistant(format!("History {n}")))
                .collect();
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        panel.update(vcx, |panel, cx| panel.scroll_transcript_direct(400.0, cx));
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        let geometry = |panel: &Panel, _app: &App| {
            let top = panel.transcript_list.logical_scroll_top();
            (
                panel.transcript_list.viewport_bounds(),
                top.item_ix,
                top.offset_in_item,
                panel.transcript_end_pad,
            )
        };
        let before = panel.read_with(vcx, geometry);
        let composer_before = vcx.debug_bounds("composer").unwrap();
        for open in [true, false] {
            let pill = vcx.debug_bounds("composer-workers").unwrap();
            vcx.simulate_click(pill.center(), Default::default());
            vcx.run_until_parked();
            scroll_momentum_tests::settle(vcx);
            assert_eq!(panel.read_with(vcx, geometry), before);
            assert_eq!(vcx.debug_bounds("composer").unwrap(), composer_before);
            assert_eq!(vcx.debug_bounds("swarm-list").is_some(), open);
        }
    }

    #[gpui::test]
    fn swarm_background_spans_the_composer_without_covering_it(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let window = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(window, gpui::size(px(1100.), px(650.)));
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        let card = vcx.debug_bounds("swarm-list").unwrap();
        let composer = vcx.debug_bounds("composer").unwrap();
        assert_eq!(card.left(), composer.left(), "{card:?} vs {composer:?}");
        assert_eq!(card.right(), composer.right(), "{card:?} vs {composer:?}");
        assert!(card.bottom() < composer.top(), "{card:?} vs {composer:?}");
        assert!(pill.center().x >= card.left() && pill.center().x <= card.right());
        let outside = gpui::point(card.left() - px(5.), card.center().y);
        vcx.simulate_click(outside, Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list").is_none());
    }

    #[gpui::test]
    fn swarm_trigger_toggles_on_release_without_closing_on_press(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_mouse_down(pill.center(), gpui::MouseButton::Left, Default::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("swarm-list").is_some(),
            "still open on press"
        );
        vcx.simulate_mouse_up(pill.center(), gpui::MouseButton::Left, Default::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("swarm-list").is_none(),
            "closed on release"
        );
        let trigger = panel.read_with(vcx, |panel, _| panel.swarm_trigger_focus.clone());
        assert!(
            vcx.update(|window, _| trigger.is_focused(window)),
            "focus returns to trigger"
        );
        for key in ["space", "enter"] {
            press(vcx, key);
            assert!(vcx.debug_bounds("swarm-list").is_some(), "{key} opens");
            press(vcx, key);
            assert!(vcx.debug_bounds("swarm-list").is_none(), "{key} closes");
            assert!(vcx.update(|window, _| trigger.is_focused(window)));
        }
    }

    #[gpui::test]
    fn effort_and_mode_pills_toggle_with_mouse_and_can_open_with_enter_or_space(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = setup(cx);
        panel.update(vcx, |panel, cx| {
            panel.apply(
                &event(serde_json::json!({
                    "ev":"runtime_info", "provider":"openai", "model":"gpt-5.6-sol",
                    "reasoning_effort":"high", "routes":[]
                })),
                cx,
            );
            panel.active_skill = Some("test-mode".into());
            cx.notify();
        });
        vcx.run_until_parked();
        for (selector, kind) in [
            ("panel-model-effort", PickerKind::Effort),
            ("panel-mode", PickerKind::Mode),
        ] {
            let is_open = |panel: &Panel, _app: &App| {
                panel
                    .picker_search
                    .as_ref()
                    .is_some_and(|(open, _)| *open == kind)
            };
            let pill = vcx.debug_bounds(selector).unwrap();
            vcx.simulate_click(pill.center(), Default::default());
            vcx.run_until_parked();
            assert!(panel.read_with(vcx, is_open), "{selector} opens");
            let pill = vcx.debug_bounds(selector).unwrap();
            vcx.simulate_mouse_down(pill.center(), gpui::MouseButton::Left, Default::default());
            vcx.run_until_parked();
            assert!(
                panel.read_with(vcx, is_open),
                "{selector}: still open on press"
            );
            vcx.simulate_mouse_up(pill.center(), gpui::MouseButton::Left, Default::default());
            vcx.run_until_parked();
            assert!(
                !panel.read_with(vcx, is_open),
                "{selector}: closes on release"
            );
            let trigger =
                panel.read_with(vcx, |panel, _| panel.picker_triggers[kind as usize].clone());
            for key in ["enter", "space"] {
                vcx.update(|window, cx| window.focus(&trigger, cx));
                vcx.run_until_parked();
                press(vcx, key);
                assert!(panel.read_with(vcx, is_open), "{selector}: {key} opens");
                press(vcx, "escape");
                assert!(!panel.read_with(vcx, is_open));
                assert!(vcx.update(|window, _| trigger.is_focused(window)));
            }
        }
    }

    #[gpui::test]
    fn finished_workers_are_collapsed_and_use_the_lane_label(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let session = serde_json::from_value(serde_json::json!({
            "session_id": "session_fox_1_a", "status": "closed", "parent_session_id": "activity-bar",
            "swarm_status": "stopped", "title": "Retry this lane after the previous worker failed",
            "agent_label": "tab-status"
        }))
        .unwrap();
        let finished = Worker::from_session(&session, None, 0);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![finished.clone(), worker("rose", "running")], cx);
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("work-section-Workers").is_some());
        assert!(vcx.debug_bounds("swarm-list-worker-0").is_some());
        assert!(
            vcx.debug_bounds("swarm-list-worker-1").is_none(),
            "history starts collapsed"
        );
        let history = vcx.debug_bounds("work-section-Finished workers").unwrap();
        vcx.simulate_click(history.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list-worker-1").is_some());
        assert_eq!(finished.row_title(), "tab-status");
    }

    #[gpui::test]
    fn a_long_swarm_list_scrolls_without_moving_the_transcript(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let window = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(window, gpui::size(px(1060.), px(590.)));
        panel.update(vcx, |panel, cx| {
            *panel.items = (0..80)
                .map(|n| Item::Assistant(format!("History {n}")))
                .collect();
            panel.set_swarm_workers(
                (0..30)
                    .map(|n| worker(&format!("rose{n}"), "running"))
                    .collect(),
                cx,
            );
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        panel.update(vcx, |panel, cx| panel.scroll_transcript_direct(400.0, cx));
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        vcx.run_until_parked();
        let bounds = vcx.debug_bounds("swarm-list-scroll").unwrap();
        assert!(bounds.size.height <= px(280.));
        let offset = |panel: &Panel, _app: &App| panel.swarm_list_scroll.offset().y;
        let transcript = |panel: &Panel, _app: &App| {
            let top = panel.transcript_list.logical_scroll_top();
            (top.item_ix, top.offset_in_item)
        };
        let before = panel.read_with(vcx, transcript);
        let first = panel.read_with(vcx, |panel, _| panel.swarm_picker_rows[0].clone());
        press(vcx, "tab");
        assert!(vcx.update(|window, _| first.is_focused(window)));
        for _ in 0..29 {
            press(vcx, "tab");
        }
        assert!(panel.read_with(vcx, offset) < px(0.));
        press(vcx, "down");
        assert_eq!(panel.read_with(vcx, offset), px(0.));
        press(vcx, "up"); // wrap to the last worker and reveal it
        let last = panel.read_with(vcx, |panel, _| panel.swarm_picker_rows[29].clone());
        assert!(vcx.update(|window, _| last.is_focused(window)));
        assert!(panel.read_with(vcx, offset) < px(0.));
        press(vcx, "down");
        assert_eq!(panel.read_with(vcx, offset), px(0.));
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-150.))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        vcx.run_until_parked();
        scroll_momentum_tests::settle(vcx);
        assert!(panel.read_with(vcx, offset) < px(0.));
        assert_eq!(panel.read_with(vcx, transcript), before);
    }

    #[gpui::test]
    fn auto_lane_row_can_be_opened_with_mouse_enter_or_space(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let id = "session_puppy_1795159580000_abc";
        let session: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": id, "status": "Active", "parent_session_id": "activity-bar",
            "swarm_status": "running", "title": "btw-enter"
        }))
        .unwrap();
        let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        vcx.update(|_, cx| {
            let opened = opened.clone();
            cx.subscribe(&panel, move |_, event: &OpenSwarmWorker, _| {
                opened.borrow_mut().push(event.0.clone());
            })
            .detach();
        });
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![Worker::from_session(&session, None, 0)], cx);
        });
        vcx.run_until_parked();
        for key in [None, Some("enter"), Some("space")] {
            let pill = vcx.debug_bounds("composer-workers").unwrap();
            vcx.simulate_click(pill.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            let label = vcx.debug_bounds("swarm-list-title-0").unwrap();
            let row = vcx.debug_bounds("swarm-list-worker-0").unwrap();
            assert!(label.size.width > px(0.0) && inside(label, row));
            if let Some(key) = key {
                vcx.simulate_keystrokes("tab");
                let keystroke = gpui::Keystroke::parse(key).unwrap();
                vcx.simulate_event(gpui::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                });
                vcx.simulate_event(gpui::KeyUpEvent { keystroke });
            } else {
                vcx.simulate_click(label.center(), gpui::Modifiers::default());
            }
            vcx.run_until_parked();
            assert_eq!(opened.borrow_mut().pop().as_deref(), Some(id), "{key:?}");
            assert!(vcx.debug_bounds("swarm-list").is_none());
        }
    }

    #[gpui::test]
    fn work_overlay_keeps_job_metadata_on_one_row_and_wraps_the_hover(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = setup(cx);
        let window = vcx.update(|window, _| window.window_handle());
        let title = "A task name long enough to need more than one tooltip line ".repeat(6);
        panel.update(vcx, |panel, cx| {
            panel.set_swarm_workers(vec![worker("rose", "ready")], cx);
            panel.apply(&event(serde_json::json!({
                "ev": "background_tasks",
                "snapshot": {"tasks": [
                    {"task_id":"active", "label":title, "status":"running", "last_output_line":"Compiling renderer", "started_at":"2026-11-20T13:00:00Z", "stalled":false, "can_cancel":true},
                    {"task_id":"failed", "label":"Check API", "status":"failed", "exit_code":17, "duration_secs":4.0, "started_at":"2026-11-20T13:00:00Z", "stalled":false, "can_cancel":false},
                    {"task_id":"cancelled", "label":"Stop watcher", "status":"failed", "cancelled":true, "duration_secs":1.0, "started_at":"2026-11-20T13:00:00Z", "stalled":false, "can_cancel":false},
                    {"task_id":"done", "label":"Run tests", "status":"completed", "duration_secs":2.0, "started_at":"2026-11-20T13:00:00Z", "stalled":false, "can_cancel":false}
                ]}
            })), cx);
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("composer-workers").unwrap();
        vcx.simulate_click(pill.center(), Default::default());
        for width in [1100., 430.] {
            vcx.simulate_window_resize(window, gpui::size(px(width), px(700.)));
            vcx.run_until_parked();
            let workers = vcx.debug_bounds("work-section-Workers").unwrap();
            let jobs = vcx.debug_bounds("work-section-Background tasks").unwrap();
            assert!(workers.bottom() <= jobs.top());
            for (index, selectors) in [
                (0, ["mark", "title", "progress", "duration"].as_slice()),
                (1, ["mark", "title", "exit", "duration"].as_slice()),
                (2, ["mark", "title", "duration"].as_slice()),
                (3, ["mark", "title", "duration"].as_slice()),
            ] {
                let mut bounds = |field| {
                    let selector = format!("background-job-{field}-{index}");
                    vcx.debug_bounds(Box::leak(selector.into_boxed_str()))
                        .unwrap()
                };
                let row = bounds("row");
                assert_eq!(row.size.height, px(22.0));
                let mut right = row.left();
                for field in selectors {
                    let field = bounds(field);
                    assert!(inside(field, row));
                    assert!(field.left() >= right);
                    right = field.right();
                }
            }
        }
        let row = vcx.debug_bounds("background-job-row-0").unwrap();
        vcx.simulate_mouse_move(row.center(), None, Default::default());
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(1));
        vcx.run_until_parked();
        let hover = vcx
            .debug_bounds("rows-tooltip-title")
            .expect("job title on hover");
        assert!(
            hover.size.height > row.size.height,
            "tooltip title must wrap"
        );
    }

    #[gpui::test]
    fn background_job_row_opens_live_output(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let snapshot = |status: &str, tail: Option<&str>| {
            let frame = serde_json::json!({
                "ev": "background_tasks", "session_id": "activity-bar",
                "snapshot": {"tasks": [{"task_id":"job123","label":"Compile","status":status,
                    "started_at":"2026-11-20T13:00:00Z","stalled":false,"can_cancel": status == "running"}],
                "output": tail.map(|tail| serde_json::json!({"task_id":"job123","tail":tail,"truncated":false}))},
            });
            serde_json::from_value::<jcode_sdk::ApiEvent>(frame).unwrap()
        };
        panel.update(vcx, |panel, cx| panel.apply(&snapshot("running", None), cx));
        vcx.run_until_parked();
        for key in [None, Some("enter"), Some("space")] {
            let pill = vcx.debug_bounds("composer-workers").expect("tasks pill");
            vcx.simulate_click(pill.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("swarm-list").is_some(),
                "opened list: {key:?}"
            );
            let row = vcx
                .debug_bounds("background-job-row-0")
                .unwrap_or_else(|| panic!("background job row: {key:?}"));
            if let Some(key) = key {
                vcx.simulate_keystrokes("tab");
                let keystroke = gpui::Keystroke::parse(key).unwrap();
                vcx.simulate_event(gpui::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                });
                vcx.simulate_event(gpui::KeyUpEvent { keystroke });
            } else {
                vcx.simulate_click(row.center(), gpui::Modifiers::default());
            }
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("background-job-detail").is_some(),
                "{key:?}"
            );
            panel.update(vcx, |panel, cx| {
                panel.apply(&snapshot("completed", Some("last line")), cx)
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("background-job-log").is_some());
            assert!(vcx.debug_bounds("background-job-cancel").is_none());
            vcx.simulate_click(pill.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("swarm-list").is_none(),
                "dismissed list: {key:?}"
            );
            panel.update(vcx, |panel, cx| panel.apply(&snapshot("running", None), cx));
            vcx.run_until_parked();
        }
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
    fn finishing_the_last_worker_moves_its_status_into_history(cx: &mut gpui::TestAppContext) {
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
        assert!(vcx.debug_bounds("composer-workers-count").is_none());
        assert!(vcx.debug_bounds("swarm-list-state-0").is_none());
        let history = vcx.debug_bounds("work-section-Finished workers").unwrap();
        vcx.simulate_click(history.center(), Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list-state-0").is_some());
        assert!(vcx.debug_bounds("swarm-list-mark-0").is_some());
        panel.update(vcx, |panel, cx| panel.set_swarm_workers(Vec::new(), cx));
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

    #[gpui::test]
    fn an_expired_await_cannot_keep_an_idle_tab_waiting(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = setup(cx);
        let requested_ids = vec!["session_hog_1_a".into(), "session_dromedary_2_b".into()];
        let mode = |panel: &Panel| panel.tab_ring().map(|ring| ring.mode.label());
        panel.update(vcx, |panel, cx| {
            panel.set_pending_awaits(
                vec![jcode_sdk::PendingAwait {
                    requested_ids: requested_ids.clone(),
                    deadline_unix_ms: u64::MAX,
                }],
                cx,
            );
            assert_eq!(mode(panel), Some("waiting"));
            panel.set_pending_awaits(
                vec![jcode_sdk::PendingAwait {
                    requested_ids,
                    deadline_unix_ms: 1,
                }],
                cx,
            );
            assert_eq!(mode(panel), None, "an elapsed registration is not a wait");
            assert!(panel.awaiting_ids().is_empty());
        });
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
        assert_eq!(
            panel.read_with(vcx, |panel, _| panel.tab_ring().map(|r| r.mode.label())),
            Some("waiting")
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
        assert_ne!(
            panel.read_with(vcx, |panel, _| panel.tab_ring().map(|r| r.mode.label())),
            Some("waiting")
        );
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
