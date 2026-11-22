use super::composer::{BAR_ICON, TAB_HEIGHT};
use super::*;
use crate::swarm_glyph::{Icon, Mark, Motion};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum TurnOutcome {
    Done,
    Stopped,
    Error,
}

struct FooterStatus {
    selector: &'static str,
    label: String,
    detail: Option<String>,
    hover: Option<&'static str>,
    mark: Mark,
}

impl FooterStatus {
    fn new(selector: &'static str, label: impl Into<String>, mark: Mark) -> Self {
        Self {
            selector,
            label: label.into(),
            detail: None,
            hover: None,
            mark,
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn waiting(detail: impl Into<String>) -> Self {
        Self::new(
            "session-status-waiting",
            "Waiting",
            Mark {
                icon: Icon::Waiting,
                color: Theme::global().WARN,
                size: BAR_ICON,
                motion: Motion::Pulse(Duration::from_millis(2200)),
            },
        )
        .detail(detail)
    }
}

impl Panel {
    pub(super) fn finished_turn_outcome(&self) -> TurnOutcome {
        if let Some(outcome @ (TurnOutcome::Stopped | TurnOutcome::Error)) = self.turn_outcome {
            return outcome;
        }
        // A cancel from another client can reach this panel as a cancelled
        // tool followed by a generic TurnDone, with no explicit TurnStopped.
        // A subsequent assistant reply is still a successful final response.
        let cancelled = self
            .items
            .iter()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_) | Item::Assistant(_)))
            .any(|item| {
                matches!(item, Item::Tool { error: Some(error), .. }
                if error == "cancelled by the user")
            });
        if cancelled {
            TurnOutcome::Stopped
        } else {
            TurnOutcome::Done
        }
    }

    fn footer_status(&self) -> FooterStatus {
        let theme = Theme::global();
        let idle = Mark::still(Icon::Idle, theme.composer_secondary(), BAR_ICON);
        let thinking = Mark::thinking(BAR_ICON);
        let error = Mark::still(Icon::Cross, theme.ERROR, BAR_ICON);
        let arrive = |icon, color| Mark {
            icon,
            color,
            size: BAR_ICON,
            motion: Motion::Arrive(Duration::from_millis(350)),
        };
        let status = self.lowercase_status();
        if status.starts_with("lost:") || status == "disconnected" {
            return FooterStatus::new(
                "session-status-disconnected",
                "Disconnected",
                Mark::still(Icon::Cross, theme.WARN, BAR_ICON),
            )
            .detail(self.status.as_str());
        }
        if self.has_unconfirmed_stop() {
            return FooterStatus::new(
                "session-status-working",
                "Stopping",
                Mark {
                    color: theme.WARN,
                    ..thinking
                },
            );
        }
        if let Some(card) = &self.ask_card {
            return if card.submitting() {
                FooterStatus::new("session-status-working", "Submitting answer", thinking)
            } else {
                FooterStatus::new(
                    "session-status-ask",
                    "Ask",
                    Mark {
                        icon: Icon::Ask,
                        color: theme.WARN,
                        size: BAR_ICON,
                        motion: Motion::Pulse(Duration::from_millis(1200)),
                    },
                )
                .detail("Waiting for your answer")
            };
        }
        if self.activity_active() {
            if self.has_live_wait_tool() {
                return FooterStatus::waiting("Waiting for background work");
            }
            return match status.as_ref() {
                "running_tools" if self.connection_phase.is_empty() => FooterStatus::new(
                    "session-status-tools",
                    "Running tool",
                    Mark {
                        icon: Icon::Tool,
                        color: theme.ACCENT,
                        size: BAR_ICON,
                        motion: Motion::Spin(Duration::from_millis(3200)),
                    },
                ),
                _ => {
                    let label = self.status_line();
                    let mut status = FooterStatus::new("session-status-working", label, thinking);
                    if matches!(status.label.as_str(), "Responding" | "Working") {
                        status.hover = Some("Thinking");
                    }
                    status
                }
            };
        }
        if self.turn_outcome != Some(TurnOutcome::Error) {
            if let Some(line) = self.await_line() {
                return FooterStatus::waiting(line);
            }
            if self.tab_dependency_waiting {
                return FooterStatus::waiting("Waiting for a dependency");
            }
        }
        match self.turn_outcome {
            Some(TurnOutcome::Done) => {
                FooterStatus::new("session-status-done", "Done", arrive(Icon::Check, theme.OK))
            }
            Some(TurnOutcome::Stopped) => FooterStatus::new(
                "session-status-stopped",
                "Stopped",
                arrive(Icon::Stop, theme.WARN),
            ),
            Some(TurnOutcome::Error) => FooterStatus::new("session-status-error", "Error", error),
            None if matches!(status.as_ref(), "idle" | "connected" | "attached") => {
                FooterStatus::new("session-status-idle", "Idle", idle)
            }
            None if status.contains("error") || status.contains("crash") => {
                FooterStatus::new("session-status-error", "Error", error)
                    .detail(self.status.as_str())
            }
            None => FooterStatus::new("session-status-other", self.status_line(), idle),
        }
    }

    pub(super) fn render_session_status(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let FooterStatus {
            selector,
            label,
            detail,
            mark,
            hover,
        } = self.footer_status();
        let icon = self.swarm_marks.retained_element("activity", mark, cx);
        let tooltip = hover.or(detail.as_deref()).unwrap_or(&label).to_owned();
        div()
            .id("composer-activity")
            .debug_selector(|| "session-status".into())
            .relative()
            .flex_none()
            .h(px(TAB_HEIGHT))
            .w(px(18.0))
            .flex()
            .items_center()
            .justify_center()
            .role(gpui::Role::Status)
            .aria_label(label)
            .when_some(detail, |el, detail| el.aria_description(detail))
            .children(crate::fixture_control_targets::marker(
                || selector.into(),
                || Some(self.session_id.clone()),
            ))
            .tooltip(move |_, cx| {
                cx.new(|_| super::usage::MeterTooltip(tooltip.clone().into()))
                    .into()
            })
            .child(div().debug_selector(move || selector.into()).child(icon))
            .into_any_element()
    }
}
