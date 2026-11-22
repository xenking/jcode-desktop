//! Explicit abnormal endings. A transport outage is not evidence of a crashed agent.
use super::*;
use jcode_sdk::TurnStopReason;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopNotice {
    pub(super) title: String,
    pub(super) detail: String,
    provider_stop_reason: Option<String>,
    pub(super) failure: bool,
    pub(super) provisional: bool,
    /// The complete raw text for Copy, when `detail` is a summary of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) raw: Option<String>,
    /// Set while automatic recovery owns this failure. A failed attempt that
    /// is being retried has not stopped the response.
    #[serde(default, skip_serializing_if = "AutoRetry::is_transient")]
    pub(super) retry: AutoRetry,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) enum AutoRetry {
    #[default]
    None,
    Pending,
    Recovered,
}

impl AutoRetry {
    /// A countdown does not survive a restart, so only the outcome persists.
    fn is_transient(&self) -> bool {
        *self != Self::Recovered
    }
}

impl StopNotice {
    pub(super) fn title(&self) -> &str {
        match self.retry {
            AutoRetry::None => &self.title,
            AutoRetry::Pending => "Request failed. Retrying automatically",
            AutoRetry::Recovered => "Request failed. Retried automatically",
        }
    }

    /// The red recovery box with Resume belongs to a failure nobody is handling.
    pub(super) fn shows_error(&self) -> bool {
        self.failure && self.retry == AutoRetry::None
    }

    /// Text shown under the title when this is not an actionable error.
    pub(super) fn plain_detail(&self) -> String {
        if self.failure {
            crate::friendly_error::summarize(&self.detail)
        } else {
            self.detail.clone()
        }
    }

    pub(super) fn from_event(
        reason: &TurnStopReason,
        message: &str,
        provider: Option<&str>,
    ) -> Self {
        let title = match reason {
            TurnStopReason::Interrupted => "Response interrupted",
            TurnStopReason::Failure => "Response stopped: error",
            TurnStopReason::Crash => "Response stopped: session crashed",
            TurnStopReason::ProviderGuardrail => "Response stopped: provider guardrail",
            TurnStopReason::LimitReached => "Response stopped: limit reached",
            _ => "Response stopped",
        };
        Self {
            title: title.into(),
            detail: message.to_owned(),
            provider_stop_reason: provider.map(str::to_owned),
            failure: *reason != TurnStopReason::Interrupted,
            provisional: false,
            raw: None,
            retry: AutoRetry::None,
        }
    }

    pub(super) fn cancel_requested() -> Self {
        Self {
            title: "Stop requested by you".into(),
            detail:
                "You interrupted this response. Waiting for the runtime to confirm cancellation."
                    .into(),
            provider_stop_reason: None,
            failure: false,
            provisional: true,
            raw: None,
            retry: AutoRetry::None,
        }
    }

    pub(super) fn from_status(status: &str) -> Option<Self> {
        let (reason, message) = match status {
            "cancelled" | "canceled" | "interrupted" => (
                TurnStopReason::Interrupted,
                "The runtime confirmed this response was interrupted.",
            ),
            "crashed" => (
                TurnStopReason::Crash,
                "The runtime reports that the session crashed. No further details were provided.",
            ),
            "error" | "failed" => (
                TurnStopReason::Failure,
                "The runtime reports that the response failed. No further details were provided.",
            ),
            _ => return None,
        };
        Some(Self::from_event(&reason, message, None))
    }
}

impl Panel {
    pub(super) fn has_unconfirmed_stop(&self) -> bool {
        self.items.iter().rev().take_while(|item| !matches!(item, Item::User(_)))
            .any(|item| matches!(item, Item::Stopped(notice) if notice.provisional && !notice.failure))
    }

    pub(super) fn request_user_stop(&mut self, cx: &mut Context<Self>) {
        if self.has_unconfirmed_stop() {
            self.force_stop(cx);
        } else {
            if self.preview_state.is_none() {
                self.bridge.send(Command::Cancel {
                    session_id: self.session_id.clone(),
                });
            }
            self.record_cancel_requested(cx);
        }
    }

    fn force_stop(&mut self, cx: &mut Context<Self>) {
        if self.preview_state.is_none() {
            self.bridge.send(Command::ForceStopTurn {
                session_id: self.session_id.clone(),
            });
        }
        if let Some(notice) = self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .find_map(|item| match item {
                Item::Stopped(notice) if notice.provisional => Some(notice),
                _ => None,
            })
        {
            notice.title = "Force stop requested".into();
            notice.detail = "Waiting for the runtime to confirm the turn has stopped.".into();
        }
        self.transcript_measurements.dirty = true;
        cx.notify();
    }

    pub(super) fn record_cancel_requested(&mut self, cx: &mut Context<Self>) {
        self.record_stop(StopNotice::cancel_requested());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(notice) = panel.items.iter_mut().rev()
                    .take_while(|item| !matches!(item, Item::User(_)))
                    .find_map(|item| match item { Item::Stopped(notice) => Some(notice), _ => None })
                    && notice.provisional && notice.title == "Stop requested by you"
                {
                    notice.title = "Stop not confirmed".into();
                    notice.detail = "The runtime did not confirm cancellation. The turn may still be running; check the connection and try again.".into();
                    panel.transcript_measurements.dirty = true;
                    cx.notify();
                }
            });
        }).detach();
    }

    /// Update the failure of the current turn as automatic recovery progresses.
    pub(super) fn set_auto_retry(&mut self, retry: AutoRetry) {
        let notice = self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .find_map(|item| match item {
                Item::Stopped(notice) if notice.failure && !notice.provisional => Some(notice),
                _ => None,
            });
        if let Some(notice) = notice
            && notice.retry != retry
            // Only a countdown that was shown can be recovered from or abandoned.
            && (retry == AutoRetry::Pending || notice.retry == AutoRetry::Pending)
        {
            notice.retry = retry;
            self.transcript_measurements.dirty = true;
        }
    }

    pub(super) fn record_stop(&mut self, notice: StopNotice) {
        if !notice.provisional {
            self.flush_reasoning();
            self.settle_assistant_output(AssistantOutputPhase::Update);
            // Confirmed rejection/cancellation settles unacknowledged prompts.
            // A transport loss alone is ambiguous and must preserve them for
            // reconnect reconciliation rather than claiming delivery.
            self.pending_users.clear();
            self.turn_outcome = Some(if notice.failure {
                session_status::TurnOutcome::Error
            } else {
                session_status::TurnOutcome::Stopped
            });
        }
        self.pause_queue_for_stop();
        self.sound_events.cancel();
        // Reconnect can replay processing and usage without delivering new
        // output. Do not append another accounting footer for that replay.
        let replayed_stop = self.streaming_text.trim().is_empty()
            && self.streaming_reasoning.trim().is_empty()
            && matches!(
                self.items
                    .iter()
                    .rev()
                    .find(|item| !matches!(item, Item::ResponseStats(_))),
                Some(Item::Stopped(_))
            );
        if replayed_stop {
            self.response_stats.finish();
        }
        self.finish_response();
        // Settle tool spinners without claiming the underlying command completed
        // successfully. Detached background tasks remain independent.
        for item in self
            .items
            .iter_mut()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
        {
            if let Item::Tool { done, error, .. } = item
                && !*done
            {
                *done = true;
                *error = Some("Response stopped before the tool result was received.".into());
            }
        }
        // Accounting can be appended by finish_response or replayed during
        // reconnect. It is not new response output, so look past it, but never
        // cross a user message or any actual assistant/tool output.
        let previous = self
            .items
            .iter_mut()
            .rev()
            .find(|item| !matches!(item, Item::ResponseStats(_)));
        if let Some(Item::Stopped(previous)) = previous {
            // A confirmed reason replaces our optimistic local cancellation or
            // unknown transport outcome. Replayed statuses never erase details.
            if previous.provisional || !notice.provisional {
                *previous = notice;
            }
        } else {
            self.items.push(Item::Stopped(notice));
        }
    }

    pub(crate) fn connection_lost(&mut self, reason: &str, cx: &mut Context<Self>) {
        self.lose_context_action();
        // A reconnect is not evidence that the turn stopped. Keep streaming
        // state and tool rows intact until the runtime confirms an outcome.
        self.status = format!("lost: {}", crate::friendly_error::summarize(reason));
        cx.notify();
    }

    pub(super) fn render_stop_notice(
        &self,
        index: usize,
        notice: &StopNotice,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let compact = crate::config::display().compact_notifications;
        div()
            .debug_selector(|| "response-stop-notice".into())
            .flex()
            .flex_col()
            .map(|el| {
                if compact {
                    el.gap_0().px_2().py_1()
                } else {
                    el.gap_1().px_3().py_2()
                }
            })
            .rounded_xl()
            .bg(Theme::global().HEADER_BG)
            .text_size(px(if compact { 11. } else { 12. }))
            .text_color(Theme::global().TEXT_DIM)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Theme::global().TEXT)
                    .child(notice.title().to_owned()),
            )
            .when(notice.shows_error(), |el| {
                el.child(self.render_recovery_error(index, &notice.detail, true, window, cx))
            })
            .when(!notice.shows_error(), |el| {
                let copy = notice.raw.clone().unwrap_or_else(|| notice.detail.clone());
                el.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(text_selection::plain(
                            self.transcript_selection.clone(),
                            format!("{index}-stop-reason"),
                            notice.plain_detail(),
                            window,
                            cx,
                        )))
                        .child(super::recovery::copy_pill("stop-notice-copy", index, copy)),
                )
            })
            .when(
                notice.provisional
                    && (notice.title == "Stop not confirmed"
                        || notice.title == "Force stop requested"),
                |el| {
                    el.child(
                        div()
                            .id(("force-stop-button", index))
                            .debug_selector(|| "force-stop-button".into())
                            .flex_none()
                            .px_2p5()
                            .py(px(2.))
                            .rounded_full()
                            .bg(Theme::global().ACCENT_DIM)
                            .text_color(Theme::global().TEXT)
                            .cursor_pointer()
                            .hover(|el| el.opacity(0.8))
                            .on_click(cx.listener(|panel, _, _, cx| {
                                cx.stop_propagation();
                                panel.force_stop(cx);
                            }))
                            .child("Force stop"),
                    )
                },
            )
            .children(
                notice
                    .provider_stop_reason
                    .as_ref()
                    .map(|reason| div().child(format!("Provider stop reason: {reason}"))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "panel_stop_reason_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_reconnect_notice_tests.rs"]
mod reconnect_notice_tests;
