//! How a turn ended: the outcome the footer reports, the stop notice a
//! transcript row shows, and whether automatic recovery owns a failure.
//! These are values about a turn, so the panel features that start, stop and
//! retry turns all read them from here instead of from one another.
use super::*;
use jcode_sdk::TurnStopReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TurnOutcome {
    Done,
    Stopped,
    Error,
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StopNotice {
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) provider_stop_reason: Option<String>,
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

