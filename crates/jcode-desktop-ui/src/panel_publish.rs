//! Checkout eligibility and the live stage tracker for Desktop publishing.
use super::*;
use crate::publish::{self, STAGES, StageState};

impl Panel {
    pub(crate) fn is_publish_tracker(&self) -> bool {
        self.publish_tracker || self.title.as_ref() == publish::SESSION_TITLE
    }

    pub(crate) fn set_publish_tracker(&mut self, tracker: bool) {
        self.publish_tracker = tracker;
    }

    /// The checkout this panel can publish, only in Desktop self-development.
    pub(crate) fn publish_checkout(&self) -> Option<std::path::PathBuf> {
        publish::checkout_root(self.local_conversation_dir()?)
    }

    fn local_conversation_dir(&self) -> Option<&str> {
        if self.transcript_only
            || self.preview_state.is_some()
            || self.is_side_document()
            || crate::harness::remote_host(&self.session_id).is_some()
        {
            return None;
        }
        self.working_dir.as_deref()
    }

    /// Offline screenshot transcript for a tracker partway through a run.
    pub(crate) fn seed_publish_fixture(&mut self) {
        let statuses = [
            "completed",
            "completed",
            "completed",
            "completed",
            "in_progress",
            "pending",
        ];
        *self.items = vec![
            Item::User("Commit, push, release, and publish Jcode Desktop.".into()),
            Item::Todos(TodoCardPayload {
                todos: STAGES
                    .iter()
                    .zip(statuses)
                    .map(|((name, detail), status)| TodoCardItem {
                        content: format!("{name}: {detail}"),
                        status: status.into(),
                        group: None,
                        blocked_by: vec![],
                        confidence: None,
                        completion_confidence: None,
                        stage: None,
                        assigned_to: None,
                    })
                    .collect(),
                plan: TodoCardPlan::default(),
                goals: Vec::new(),
            }),
            Item::Assistant(
                "Tagged `desktop-v0.3.2` at `origin/main` and dispatched the native builds. Waiting on the macOS and cross-platform workflows.".into(),
            ),
        ];
    }

    /// Fixed stage rows fed by the session's latest todo snapshot. Shown in
    /// place of the collapsible todo summary so progress is always visible.
    pub(super) fn render_publish_tracker(
        &self,
        payload: Option<&TodoCardPayload>,
    ) -> Option<gpui::AnyElement> {
        if !self.is_publish_tracker() {
            return None;
        }
        let theme = Theme::global();
        let states = publish::stage_states(payload.into_iter().flat_map(|payload| {
            payload.todos.iter().map(|todo| {
                (
                    todo.content.as_str(),
                    todo.status.as_str(),
                    !todo.blocked_by.is_empty(),
                )
            })
        }));
        let done = states.iter().filter(|s| **s == StageState::Done).count();
        let blocked = states.contains(&StageState::Blocked);
        let started = payload.is_some_and(|payload| !payload.todos.is_empty());
        let headline = if done == STAGES.len() {
            "Published".to_string()
        } else if blocked {
            "Publish blocked".to_string()
        } else if !started {
            if self.is_pending_session() {
                "Starting session…".to_string()
            } else {
                "Planning release…".to_string()
            }
        } else {
            let current = states
                .iter()
                .position(|s| *s != StageState::Done)
                .map(|index| STAGES[index].0)
                .unwrap_or("Publish");
            format!("{current}…")
        };
        let headline_color = if done == STAGES.len() {
            theme.OK
        } else if blocked {
            theme.WARN
        } else {
            theme.TEXT
        };
        Some(
            div()
                .debug_selector(|| "publish-tracker".into())
                .flex_none()
                .mx_3()
                .mt_1()
                .mb_2()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .bg(theme.prompt_background(0))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .debug_selector(|| "publish-tracker-header".into())
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            gpui::svg()
                                .data(include_bytes!("../../../assets/icons/publish.svg")
                                    as &'static [u8])
                                .text_color(headline_color)
                                .flex_none()
                                .size(px(13.)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(headline_color)
                                .child(headline),
                        )
                        .child(
                            div()
                                .flex_none()
                                .font_family(theme.FONT_MONO)
                                .text_size(px(10.5))
                                .text_color(theme.TEXT_DIM)
                                .child(format!("{done}/{}", STAGES.len())),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "publish-tracker-bar".into())
                        .flex()
                        .gap(px(3.))
                        .children(states.iter().map(|state| {
                            div().flex_1().h(px(3.)).rounded_full().bg(match state {
                                StageState::Done => theme.OK,
                                StageState::Running => theme.ACCENT,
                                StageState::Blocked => theme.WARN,
                                StageState::Pending => theme.TEXT_FAINT.opacity(0.35),
                            })
                        })),
                )
                .child(div().flex().flex_col().gap(px(2.)).children(
                    STAGES.iter().zip(states).enumerate().map(
                        |(index, ((name, detail), state))| stage_row(index, name, detail, state),
                    ),
                ))
                .into_any_element(),
        )
    }
}

fn stage_row(
    index: usize,
    name: &'static str,
    detail: &'static str,
    state: StageState,
) -> gpui::Div {
    let theme = Theme::global();
    let color = match state {
        StageState::Done => theme.OK,
        StageState::Running => theme.ACCENT,
        StageState::Blocked => theme.WARN,
        StageState::Pending => theme.TEXT_FAINT,
    };
    let marker = div()
        .flex_none()
        .size(px(11.))
        .rounded_full()
        .border_1()
        .border_color(color)
        .p(px(2.))
        .when(state != StageState::Pending, |marker| {
            marker.child(div().size_full().rounded_full().bg(color))
        });
    div()
        .debug_selector(move || format!("publish-stage-{index}"))
        .flex()
        .items_center()
        .gap_2()
        .h(px(20.))
        .min_w_0()
        .text_size(px(12.))
        .child(marker)
        .child(
            div()
                .flex_none()
                .w(px(56.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(if state == StageState::Pending {
                    theme.TEXT_DIM
                } else {
                    theme.TEXT
                })
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(theme.TEXT_FAINT)
                .child(detail),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.FONT_MONO)
                .text_size(px(10.))
                .text_color(color)
                .child(match state {
                    StageState::Done => "done",
                    StageState::Running => "running",
                    StageState::Blocked => "blocked",
                    StageState::Pending => "",
                }),
        )
}
