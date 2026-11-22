use super::*;
use crate::swarm_glyph::{Icon, Mark};
use crate::swarm_view::Glyph;
use jcode_sdk::api::{
    BackgroundTaskOutput, BackgroundTaskStatus as Status, BackgroundTaskSummary as Job,
    BackgroundTasksSnapshot, format_progress_summary,
};

struct JobTooltip(super::usage::RowsTooltip);

impl Render for JobTooltip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.content(Some(px(320.0).min(window.viewport_size().width)))
    }
}

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
    pub fn control_state(&self) -> serde_json::Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct State<'a> {
            tasks: &'a [Job],
            selected: Option<&'a str>,
            output: Option<&'a BackgroundTaskOutput>,
        }
        serde_json::to_value(State {
            tasks: &self.tasks,
            selected: self.selected.as_deref(),
            output: self.output.as_ref(),
        })
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
        self.swarm_marks.retain(|key| {
            key.strip_prefix("background:")
                .is_none_or(|id| state.started.contains_key(id))
        });
        if let Some(output) = &snapshot.output
            && state.selected.as_ref() == Some(&output.task_id)
            && state.output.as_ref() != Some(output)
        {
            state.output = Some(output.clone());
            state.scroll.scroll_to_bottom();
        }
        if self.background_jobs.tasks.is_empty() && self.swarm_workers.is_empty() {
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
        detail_index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let id = job.task_id.clone();
        let title = one_line(&job.label);
        let selected = self.background_jobs.selected.as_ref() == Some(&id);
        let (status, mark) = job_status(job, theme);
        let icon = self
            .swarm_marks
            .element(&format!("background:{id}"), mark, cx);
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
        let progress = (job.status == Status::Running)
            .then(|| {
                job.progress
                    .as_ref()
                    .map(format_progress_summary)
                    .or_else(|| job.last_output_line.clone())
            })
            .flatten()
            .map(|value| one_line(&value))
            .filter(|value| !value.is_empty());
        let exit = (status == "failed")
            .then_some(job.exit_code)
            .flatten()
            .map(|code| format!("exit {code}"));
        let tooltip_title = title.clone();
        let tooltip_rows = [
            Some(status.into()),
            exit.clone(),
            duration.clone(),
            progress.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        crate::keyboard::tracked_button(
            div().id(SharedString::from(format!("background-job-{id}"))),
            focus,
        )
        .debug_selector(move || format!("background-job-row-{index}"))
        .flex_none()
        .w_full()
        .min_w_0()
        .min_h(px(22.0))
        .px_2()
        .flex()
        .items_center()
        .gap(px(5.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded_full()
        .cursor_pointer()
        .text_size(px(11.0))
        .text_color(theme.composer_secondary())
        .border_1()
        .border_color(if selected {
            theme.PANEL_BORDER_FOCUS
        } else {
            gpui::transparent_black().into()
        })
        .when(selected, |el| el.bg(theme.USER_BG))
        .hover(|el| el.bg(theme.USER_BG))
        .tooltip(move |_, cx| {
            cx.new(|_| JobTooltip(super::usage::RowsTooltip {
                title: tooltip_title.clone(),
                rows: tooltip_rows.clone(),
            }))
            .into()
        })
        .children(crate::fixture_control_targets::marker(
            || format!("background-job:{}", job.task_id),
            || Some(self.session_id.clone()),
        ))
        .child(
            div()
                .debug_selector(move || format!("background-job-mark-{index}"))
                .flex_none()
                .child(icon),
        )
        .child(
            div()
                .debug_selector(move || format!("background-job-title-{index}"))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.TEXT)
                .child(title),
        )
        .children(progress.map(|progress| {
            div()
                .debug_selector(move || format!("background-job-progress-{index}"))
                .min_w_0()
                .max_w(gpui::relative(0.3))
                .truncate()
                .child(progress)
        }))
        .child(div().flex_none().child(status))
        .children(exit.map(|exit| {
            div()
                .debug_selector(move || format!("background-job-exit-{index}"))
                .flex_none()
                .child(exit)
        }))
        .child(
            div()
                .debug_selector(move || format!("background-job-duration-{index}"))
                .flex_none()
                .children(duration)
                .children(ticker),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            let task_id = (this.background_jobs.selected.as_ref() != Some(&id)).then(|| id.clone());
            if task_id.is_some() {
                // The detail follows all rows. Reveal it even when opening a
                // task from a list taller than the viewport.
                this.swarm_list_scroll.scroll_to_item(detail_index);
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
            let mut parts = vec![job_status(job, theme).0.to_owned()];
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
                .text_color(theme.composer_secondary())
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

fn job_status(job: &Job, theme: &Theme) -> (&'static str, Mark) {
    let glyph = |glyph| Mark::glyph(glyph, 11.0);
    let stopped = || Mark::still(Icon::Stop, theme.composer_secondary(), 11.0);
    match job.status {
        Status::Running if job.stalled => ("stalled", glyph(Glyph::Pause)),
        Status::Running => ("running", glyph(Glyph::Spinner)),
        Status::Completed => ("completed", glyph(Glyph::Check)),
        Status::Failed if job.cancelled || job.error.as_deref() == Some("Cancelled by user") => {
            ("cancelled", stopped())
        }
        Status::Failed => ("failed", glyph(Glyph::Cross)),
        Status::Superseded => ("superseded", stopped()),
        Status::Interrupted => ("interrupted", stopped()),
    }
}
