use super::*;
use jcode_sdk::api::{
    BackgroundTaskOutput, BackgroundTaskStatus as Status, BackgroundTaskSummary as Job,
    BackgroundTasksSnapshot, format_progress_summary,
};

#[derive(Default)]
pub(super) struct BackgroundJobs {
    pub tasks: Vec<Job>,
    pub row_focus: HashMap<String, FocusHandle>,
    selected: Option<String>,
    output: Option<BackgroundTaskOutput>,
    scroll: gpui::ScrollHandle,
    started: HashMap<String, Instant>,
    tickers: crate::pulse_text::Tickers,
}

impl BackgroundJobs {
    pub fn control_state(&self) -> serde_json::Value {
        serde_json::json!({"tasks": self.tasks, "selected": self.selected, "output": self.output})
    }

    pub fn active(&self) -> usize {
        self.tasks
            .iter()
            .filter(|job| job.status == Status::Running)
            .count()
    }
}

impl Panel {
    pub(super) fn apply_background_tasks(
        &mut self,
        snapshot: &BackgroundTasksSnapshot,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.background_jobs;
        state.tasks.clone_from(&snapshot.tasks);
        // Keep running jobs reachable when there is a long history of completed tasks.
        state.tasks.sort_by_key(|job| job.status != Status::Running);
        state
            .row_focus
            .retain(|id, _| state.tasks.iter().any(|job| job.task_id == *id));
        state.started.retain(|id, _| {
            state
                .tasks
                .iter()
                .any(|job| job.task_id == *id && job.status == Status::Running)
        });
        for job in state
            .tasks
            .iter()
            .filter(|job| job.status == Status::Running)
        {
            state.started.entry(job.task_id.clone()).or_insert_with(|| {
                let elapsed = chrono::DateTime::parse_from_rfc3339(&job.started_at)
                    .ok()
                    .and_then(|start| {
                        chrono::Utc::now()
                            .signed_duration_since(start)
                            .to_std()
                            .ok()
                    })
                    .unwrap_or_default();
                let now = Instant::now();
                now.checked_sub(elapsed).unwrap_or(now)
            });
        }
        state.tickers.retain(|key| state.started.contains_key(key));
        if let Some(output) = &snapshot.output
            && state.selected.as_ref() == Some(&output.task_id)
            && state.output.as_ref() != Some(output)
        {
            state.output = Some(output.clone());
            state.scroll.scroll_to_bottom();
        }
        if self.background_jobs.tasks.is_empty() && self.swarm_indicator().is_none() {
            self.swarm_list_open = false;
            self.select_background_job(None, cx);
        }
        cx.notify();
    }

    pub(super) fn select_background_job(
        &mut self,
        task_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.background_jobs.selected != task_id {
            self.background_jobs.selected = task_id.clone();
            self.background_jobs.output = None;
            self.bridge.send(Command::WatchBackgroundTaskOutput {
                session_id: self.session_id.clone(),
                task_id,
            });
            cx.notify();
        }
    }

    pub(super) fn render_background_job_row(
        &mut self,
        index: usize,
        job: &Job,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let id = job.task_id.clone();
        let tooltip = job.label.clone();
        let selected = self.background_jobs.selected.as_ref() == Some(&id);
        let status = job_status(job);
        let color = if job.stalled || matches!(job.status, Status::Failed | Status::Interrupted) {
            theme.WARN
        } else if job.status == Status::Running {
            theme.ACCENT
        } else {
            theme.TEXT_FAINT
        };
        let duration = job
            .duration_secs
            .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
            .map(format_tool_elapsed);
        let ticker = self.background_jobs.started.get(&id).map(|&since| {
            self.background_jobs.tickers.get(
                &id,
                crate::pulse_text::Kind::Elapsed {
                    since,
                    format: format_tool_elapsed,
                },
                cx,
            )
        });
        let progress = job
            .progress
            .as_ref()
            .and_then(|progress| progress.percent)
            .map(|p| format!("{p:.0}%"));
        crate::keyboard::tracked_button(
            div().id(SharedString::from(format!("background-job-{id}"))),
            focus,
        )
        .debug_selector(move || format!("background-job-row-{index}"))
        .flex_none()
        .w_full()
        .min_w_0()
        .min_h(px(20.0))
        .px_2()
        .flex()
        .items_center()
        .gap(px(5.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded_full()
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(theme.TEXT_DIM)
        .when(selected, |el| el.bg(theme.chip_active_background()))
        .hover(|el| el.bg(theme.USER_BG))
        .tooltip(move |_, cx| {
            cx.new(|_| super::usage::MeterTooltip(tooltip.clone().into()))
                .into()
        })
        .children(crate::fixture_control_targets::marker(
            || format!("background-job:{}", job.task_id),
            || Some(self.session_id.clone()),
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(one_line(&job.label)),
        )
        .children(progress.map(|progress| div().flex_none().child(progress)))
        .child(div().flex_none().text_color(color).child(status))
        .child(div().flex_none().children(duration).children(ticker))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            let task_id = (this.background_jobs.selected.as_ref() != Some(&id)).then(|| id.clone());
            if task_id.is_some() {
                // The detail follows all rows. Reveal it even when opening a
                // task from a list taller than the viewport.
                this.swarm_list_scroll.scroll_to_item(
                    this.swarm_picker_rows.len() + this.background_jobs.tasks.len(),
                );
            }
            this.select_background_job(task_id, cx);
        }))
        .into_any_element()
    }

    pub(super) fn render_background_job_detail(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let state = &self.background_jobs;
        let id = state.selected.as_ref()?;
        let job = state.tasks.iter().find(|job| &job.task_id == id);
        let theme = Theme::global();
        let text = state.output.as_ref().map_or_else(
            || {
                if job.is_some() {
                    "No output yet".into()
                } else {
                    "Task is no longer available".into()
                }
            },
            |output| {
                if output.tail.is_empty() {
                    return "No output yet".into();
                }
                if output.truncated {
                    format!("…\n{}", output.tail)
                } else {
                    output.tail.clone()
                }
            },
        );
        let status = job.map(|job| {
            let mut parts = vec![job_status(job).to_owned()];
            if let Some(progress) = &job.progress {
                parts.push(format_progress_summary(progress));
            }
            if let Some(error) = &job.error {
                parts.push(error.clone());
            }
            parts.join(" · ")
        });
        let cancel = job.filter(|job| job.can_cancel).map(|job| {
            let task_id = job.task_id.clone();
            crate::keyboard::button(div().id("background-job-cancel"))
                .debug_selector(|| "background-job-cancel".into())
                .flex_none()
                .px_2()
                .rounded_full()
                .cursor_pointer()
                .text_color(theme.WARN)
                .hover(|el| el.bg(theme.USER_BG))
                .child("Cancel task")
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    // The cancel button disappears when the task stops. Keep focus in
                    // the list instead of blurring and closing the final log.
                    if let Some(focus) = this.background_jobs.row_focus.get(&task_id) {
                        window.focus(focus, cx);
                    } else {
                        window.focus(&this.swarm_list_focus, cx);
                    }
                    this.bridge.send(Command::CancelBackgroundTask {
                        session_id: this.session_id.clone(),
                        task_id: task_id.clone(),
                    });
                }))
        });
        Some(
            div()
                .id("background-job-detail")
                .debug_selector(|| "background-job-detail".into())
                .flex_none()
                .w_full()
                .min_w_0()
                .mt_1()
                .p_2()
                .rounded_xl()
                .bg(theme.USER_BG)
                .text_size(px(11.0))
                .text_color(theme.TEXT_DIM)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(status.unwrap_or_default()),
                        )
                        .children(cancel),
                )
                .child(
                    div()
                        .id("background-job-log")
                        .debug_selector(|| "background-job-log".into())
                        .track_scroll(&state.scroll)
                        .max_h(px(144.0))
                        .mt_1()
                        .overflow_y_scroll()
                        .font_family(theme.FONT_MONO)
                        .child(text),
                )
                .into_any_element(),
        )
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn job_status(job: &Job) -> &'static str {
    match job.status {
        Status::Running if job.stalled => "stalled",
        Status::Running => "running",
        Status::Completed => "completed",
        Status::Failed => "failed",
        Status::Superseded => "superseded",
        Status::Interrupted => "interrupted",
    }
}
