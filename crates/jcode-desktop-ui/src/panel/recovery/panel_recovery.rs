//! Native recovery actions. Route changes never pass through the composer.
use super::*;
use super::chrome::{action_pill, copy_pill};
use gpui::AnyElement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecoveryKind {
    Auth,
    Model,
    Quota,
    Failover,
    Other,
}

fn recovery_kind(message: &str) -> RecoveryKind {
    if jcode_base::provider::parse_failover_prompt_message(message).is_some() {
        return RecoveryKind::Failover;
    }
    let message = message.to_ascii_lowercase();
    if [
        "token refresh failed",
        "refresh token was previously rejected",
        "jcode login",
        "refresh_token_invalidated",
        "re-authenticate",
        "invalid_grant",
        "invalid x-api-key",
        "incorrect api key",
        "api key not valid",
        "no refresh token",
        "credentials have been revoked",
        "run /login",
        "unauthorized",
        "authentication",
        "not authenticated",
        "invalid api key",
        "invalid_api_key",
        "api key missing",
        "missing api key",
        "invalid credentials",
        "missing credentials",
        "token expired",
        "expired token",
        "login required",
        "log in",
        "sign in",
        "403 forbidden",
        "http 403",
        "status: 403",
        "status 403",
        "not logged in",
        "http 401",
        "status: 401",
        "status 401",
        "returned 401",
    ]
    .iter()
    .any(|term| message.contains(term))
    {
        RecoveryKind::Auth
    } else if [
        "quota",
        "rate limit",
        "rate_limit",
        "rate-limit",
        "http 429",
        "status: 429",
        "status 429",
        "returned 429",
        "too many requests",
        "insufficient credits",
        "insufficient balance",
        "billing",
        "usage limit",
    ]
    .iter()
    .any(|term| message.contains(term))
    {
        RecoveryKind::Quota
    } else if message.contains("model")
        && [
            "not currently usable",
            "choose another model",
            "not found",
            "not_found",
            "not_supported",
            "deprecated",
            "retired",
            "unavailable",
            "not available",
            "not supported",
            "unsupported",
            "invalid",
            "access",
            "permission",
            "unknown",
            "does not exist",
        ]
        .iter()
        .any(|term| message.contains(term))
    {
        RecoveryKind::Model
    } else {
        RecoveryKind::Other
    }
}

// Provider errors can embed arbitrary CLI commands, nested causes and setup
// instructions. Present a native summary rather than attempting a brittle
// command-by-command rewrite. Copy details retains the complete original.
pub(super) fn native_error_message(message: &str) -> String {
    if let Some(offer) = jcode_base::provider::parse_failover_prompt_message(message) {
        return format!(
            "{} is unavailable. Switching to {} would resend about {} input tokens (~{} characters). Choose model to review and confirm a route. No model has been switched.",
            offer.from_label,
            offer.to_label,
            offer.estimated_input_tokens,
            offer.estimated_input_chars,
        );
    }
    match recovery_kind(message) {
        RecoveryKind::Auth => "Account authentication failed. Log in again or choose another model.",
        RecoveryKind::Model => "This model is unavailable for the current account. Choose another model.",
        RecoveryKind::Quota => "The provider's usage or rate limit was reached. Choose another model or log in to a different account.",
        RecoveryKind::Failover => unreachable!("parsed above"),
        RecoveryKind::Other => return crate::friendly_error::summarize(message),
    }.to_string()
}

impl Panel {
    fn can_resume_error(&self, index: usize, message: &str) -> bool {
        let confirmed_failure = match self.items.get(index) {
            Some(Item::RuntimeError {
                message: current, ..
            }) => current == message,
            Some(Item::Stopped(notice)) => {
                notice.shows_error() && !notice.provisional && notice.detail == message
            }
            _ => false,
        };
        let retryable = match jcode_sdk::classify_failure(message, None) {
            jcode_sdk::FailureKind::Transient => true,
            jcode_sdk::FailureKind::RateLimited { retry_after } => {
                retry_after.is_none_or(|wait| wait <= jcode_sdk::turn_retry::MAX_RATE_LIMIT_WAIT)
            }
            jcode_sdk::FailureKind::Permanent => false,
        };
        retryable
            && matches!(self.status.as_str(), "idle" | "connected")
            && !self.activity_active()
            && !self.is_pending_session()
            && self.pending_users.is_empty()
            // Queued user requests take priority over a synthetic continuation.
            && !self.has_queued_prompts()
            && confirmed_failure
            && self.items[index + 1..]
                .iter()
                .all(|item| matches!(item, Item::ResponseStats(_)))
    }

    fn resume_failed_turn(&mut self, index: usize, message: &str, cx: &mut Context<Self>) {
        // Recheck at click time: the runtime may have started retrying since
        // this pill was painted. Never replay the original user payload.
        if self.can_resume_error(index, message) {
            self.submit_or_queue(
                "Continue the task from where you left off. Do not repeat tool calls or edits that already completed.".into(),
                Vec::new(),
                false,
                cx,
            );
        }
    }


    pub(super) fn render_recovery_error(
        &self,
        index: usize,
        message: &str,
        runtime_error: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = if runtime_error {
            recovery_kind(message)
        } else {
            RecoveryKind::Other
        };
        let details = message.to_string();
        // The native summary above is a rewrite of the provider's text. Keep
        // the original one click away, in full, instead of only on the
        // clipboard.
        let key = expandable::Key::details(index);
        let expanded = self.expanded_regions.contains(&key);
        let raw_truncated = expandable::preview(&details, expandable::NOTICE).is_some();
        div()
            .debug_selector(|| "recovery-error".into())
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_xl()
            .bg(Theme::global().ERROR_BG)
            .text_size(px(12.))
            .text_color(Theme::global().ERROR)
            .child(text_selection::plain(
                self.transcript_selection.clone(),
                format!("{index}-error"),
                if runtime_error {
                    native_error_message(message)
                } else {
                    message.to_owned()
                },
                window,
                cx,
            ))
            .when(expanded, |el| {
                el.child(expandable::Disclosure::scrolled(
                    format!("{index}-error-raw-scroll").into(),
                    expandable::MAX_HEIGHT,
                    text_selection::plain(
                        self.transcript_selection.clone(),
                        format!("{index}-error-raw"),
                        details.clone(),
                        window,
                        cx,
                    ),
                ))
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1p5()
                    .when(raw_truncated, |el| {
                        let key = key.clone();
                        el.child(expandable::Disclosure::toggle(
                            format!("{index}-error-expand").into(),
                            expanded,
                            "▸ Show details",
                            "▾ Hide details",
                            cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.toggle_line(&key, cx);
                            }),
                        ))
                    })
                    .child(copy_pill("recovery-copy", index, details))
                    .when(self.can_resume_error(index, message), |el| {
                        let message = message.to_owned();
                        el.child(action_pill("recovery-resume", index, "Resume").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.resume_failed_turn(index, &message, cx);
                                cx.stop_propagation();
                            }),
                        ))
                    })
                    .when(kind != RecoveryKind::Other, |el| {
                        el.child(
                            action_pill("recovery-choose-model", index, "Choose model").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.open_recovery_models(cx);
                                    cx.stop_propagation();
                                }),
                            ),
                        )
                    })
                    .when(
                        matches!(kind, RecoveryKind::Auth | RecoveryKind::Quota),
                        |el| {
                            el.child(action_pill("recovery-login", index, "Log in").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.open_login_picker(cx);
                                    cx.stop_propagation();
                                }),
                            ))
                        },
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "panel_recovery_tests.rs"]
mod tests;
