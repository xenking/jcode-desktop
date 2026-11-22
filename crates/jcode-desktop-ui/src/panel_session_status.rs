use super::*;

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
    color: gpui::Rgba,
}

impl FooterStatus {
    fn new(selector: &'static str, label: impl Into<String>, color: gpui::Rgba) -> Self {
        Self {
            selector,
            label: label.into(),
            detail: None,
            color,
        }
    }

    fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
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
        let muted = theme.composer_secondary();
        let status = self.lowercase_status();
        if status.starts_with("lost:") || status == "disconnected" {
            return FooterStatus::new("session-status-disconnected", "Disconnected", theme.WARN)
                .detail(self.status.as_str());
        }
        if self.has_unconfirmed_stop() {
            return FooterStatus::new("session-status-working", "Stopping", theme.WARN);
        }
        if let Some(card) = &self.ask_card {
            return if card.submitting {
                FooterStatus::new("session-status-working", "Submitting answer", theme.ACCENT)
            } else {
                FooterStatus::new("session-status-ask", "Ask", theme.WARN)
                    .detail("Waiting for your answer")
            };
        }
        if self.activity_active() {
            if self.has_live_wait_tool() {
                return FooterStatus::new("session-status-waiting", "Waiting", theme.WARN)
                    .detail("Waiting for background work");
            }
            return match status.as_ref() {
                "running_tools" if self.connection_phase.is_empty() => {
                    FooterStatus::new("session-status-tools", "Tools", theme.ACCENT)
                        .detail("Running tools")
                }
                _ => FooterStatus::new("session-status-working", self.status_line(), theme.ACCENT),
            };
        }
        if self.turn_outcome != Some(TurnOutcome::Error) {
            if let Some(line) = self.await_line() {
                return FooterStatus::new("session-status-waiting", "Waiting", theme.WARN)
                    .detail(line);
            }
            if self.tab_dependency_waiting {
                return FooterStatus::new("session-status-waiting", "Waiting", theme.WARN)
                    .detail("Waiting for a dependency");
            }
        }
        match self.turn_outcome {
            Some(TurnOutcome::Done) => FooterStatus::new("session-status-done", "Done", theme.OK),
            Some(TurnOutcome::Stopped) => {
                FooterStatus::new("session-status-stopped", "Stopped", theme.WARN)
            }
            Some(TurnOutcome::Error) => {
                FooterStatus::new("session-status-error", "Error", theme.ERROR)
            }
            None if matches!(status.as_ref(), "idle" | "connected" | "attached") => {
                FooterStatus::new("session-status-idle", "Idle", muted)
            }
            None if status.contains("error") || status.contains("crash") => {
                FooterStatus::new("session-status-error", "Error", theme.ERROR)
                    .detail(self.status.as_str())
            }
            None => FooterStatus::new("session-status-other", self.status_line(), muted),
        }
    }

    pub(super) fn render_session_status(&self) -> gpui::AnyElement {
        let FooterStatus {
            selector,
            label,
            detail,
            color,
        } = self.footer_status();
        div()
            .id("composer-session-status")
            .debug_selector(|| "session-status".into())
            .relative()
            .min_w_0()
            .flex_shrink_1()
            .truncate()
            .text_color(color)
            .children(crate::fixture_control_targets::marker(
                || selector.into(),
                || Some(self.session_id.clone()),
            ))
            .tooltip({
                let detail = detail.unwrap_or_else(|| label.clone());
                move |_, cx| {
                    cx.new(|_| super::usage::MeterTooltip(detail.clone().into()))
                        .into()
                }
            })
            .child(
                div()
                    .debug_selector(move || selector.into())
                    .truncate()
                    .child(label),
            )
            .into_any_element()
    }
}
