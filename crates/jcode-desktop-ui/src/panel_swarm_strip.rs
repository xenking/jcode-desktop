//! Coordinator strip above the composer: its swarm workers with live state,
//! the real pending await ("Waiting for N workers ...") while idle, and the
//! latest "worker reported · coordinator woke" event.

use super::*;
use crate::swarm_view::{Strip, WorkerState};

/// A worker chip in the coordinator strip was clicked.
pub(crate) struct OpenSwarmWorker(pub String);

impl gpui::EventEmitter<OpenSwarmWorker> for Panel {}

/// Chips shown before collapsing the rest into `+N`.
const MAX_CHIPS: usize = 12;

pub(crate) fn state_color(state: WorkerState) -> gpui::Rgba {
    let theme = Theme::global();
    match state {
        WorkerState::Running | WorkerState::Queued => theme.ACCENT,
        WorkerState::Failed => theme.ERROR,
        WorkerState::Blocked => theme.WARN,
        WorkerState::Ready | WorkerState::Stopped | WorkerState::Unknown => theme.TEXT_FAINT,
    }
}

impl Panel {
    pub(crate) fn set_swarm_strip(&mut self, strip: Option<Strip>, cx: &mut Context<Self>) {
        if self.swarm_strip != strip {
            self.swarm_strip = strip;
            cx.notify();
        }
    }

    pub(super) fn render_swarm_strip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let strip = self.swarm_strip.as_ref()?;
        let theme = Theme::global();
        let idle = !self.is_busy();
        let line = |id: &'static str, text: String| {
            div()
                .debug_selector(move || id.into())
                .min_w_0()
                .truncate()
                .text_size(px(10.5))
                .text_color(theme.TEXT_DIM)
                .child(text)
        };
        let extra = strip.workers.len().saturating_sub(MAX_CHIPS);
        let chips = strip
            .workers
            .iter()
            .take(MAX_CHIPS)
            .enumerate()
            .map(|(index, worker)| {
                let id = worker.session_id.clone();
                div()
                    .id(("swarm-strip-worker", index))
                    .debug_selector(move || format!("swarm-strip-worker-{index}"))
                    .flex_none()
                    .h(px(18.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .rounded_full()
                    .bg(theme.TOOL_BG)
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.PANEL_BG))
                    .text_size(px(10.5))
                    .text_color(theme.TEXT_DIM)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(OpenSwarmWorker(id.clone()));
                    }))
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(state_color(worker.state)),
                    )
                    .child(format!("{} {}", worker.name, worker.status_text()))
            });
        Some(
            div()
                .debug_selector(|| "swarm-strip".into())
                .w_full()
                .min_w_0()
                .px_1()
                .pb_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .when_some(strip.waiting.clone().filter(|_| idle), |el, text| {
                    el.child(line("swarm-strip-waiting", text))
                })
                .when_some(strip.report.clone(), |el, text| {
                    el.child(line("swarm-strip-report", text))
                })
                .when(!strip.workers.is_empty(), |el| {
                    el.child(div().flex().flex_wrap().gap(px(4.0)).children(chips).when(
                        extra > 0,
                        |el| {
                            el.child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.TEXT_FAINT)
                                    .child(format!("+{extra}")),
                            )
                        },
                    ))
                })
                .into_any_element(),
        )
    }
}
