//! A paired machine's connect failure, in words the owner can act on.
//!
//! The SDK classifies the failure at its source. This module only chooses the
//! sentence and the next step, so no raw machine ids or error codes reach a row.

use jcode_sdk::{Error, ErrorKind, PairedFailure};

/// What the owner should do about a failed connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureAction {
    /// Trying again can succeed once the cause clears (network up, host awake).
    Retry,
    /// Only pairing again helps: the machine no longer accepts this device.
    Repair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Paired(PairedFailure),
    /// Any other failure while connecting to a paired machine.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectFailure {
    pub kind: FailureKind,
    /// Technical cause for a details disclosure. Never contains a token.
    pub detail: String,
}

impl ConnectFailure {
    pub fn from_error(error: &Error) -> Self {
        let kind = match error.kind {
            ErrorKind::Paired(failure) => FailureKind::Paired(failure),
            _ => FailureKind::Other,
        };
        Self {
            kind,
            detail: error.to_string(),
        }
    }

    pub fn action(&self) -> FailureAction {
        match self.kind {
            FailureKind::Paired(
                PairedFailure::Revoked | PairedFailure::WrongOrigin | PairedFailure::NoToken,
            ) => FailureAction::Repair,
            _ => FailureAction::Retry,
        }
    }

    /// One line naming the machine, the cause and, where useful, the fix.
    pub fn summary(&self, label: &str) -> String {
        match self.kind {
            FailureKind::Paired(PairedFailure::Unresolved) => format!(
                "Can't reach {label} (name did not resolve). Is Tailscale or the VPN on?"
            ),
            FailureKind::Paired(PairedFailure::Refused) => format!(
                "Can't reach {label} (connection refused). Is Jcode running there?"
            ),
            FailureKind::Paired(PairedFailure::TimedOut) => {
                format!("{label} did not answer in time. It may be asleep or offline.")
            }
            FailureKind::Paired(PairedFailure::Unreachable) => {
                format!("Can't reach {label} (network unavailable). Check the network or VPN.")
            }
            FailureKind::Paired(PairedFailure::Tls) => format!(
                "Can't open a secure connection to {label}. Check its address and certificate."
            ),
            FailureKind::Paired(PairedFailure::Revoked) => {
                format!("{label} no longer accepts this device. Pair it again.")
            }
            FailureKind::Paired(PairedFailure::WrongOrigin) => {
                format!("The saved address for {label} does not serve Jcode. Pair it again.")
            }
            FailureKind::Paired(PairedFailure::UpgradeRejected) => {
                format!("{label} rejected the connection. Try again in a moment.")
            }
            FailureKind::Paired(PairedFailure::TooOld) => {
                format!("{label} runs a Jcode too old for Desktop. Update Jcode there.")
            }
            FailureKind::Paired(PairedFailure::NoToken) => {
                format!("This Mac has no saved login for {label}. Pair it again.")
            }
            FailureKind::Other => format!("Can't connect to {label}. Try again."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(kind: PairedFailure) -> ConnectFailure {
        ConnectFailure::from_error(&Error::new(ErrorKind::Paired(kind), "cause"))
    }

    #[test]
    fn each_cause_has_its_own_sentence_and_no_raw_id() {
        let all = [
            PairedFailure::Unresolved,
            PairedFailure::Refused,
            PairedFailure::TimedOut,
            PairedFailure::Unreachable,
            PairedFailure::Tls,
            PairedFailure::Revoked,
            PairedFailure::WrongOrigin,
            PairedFailure::UpgradeRejected,
            PairedFailure::TooOld,
            PairedFailure::NoToken,
        ];
        let lines: std::collections::HashSet<_> =
            all.iter().map(|kind| failure(*kind).summary("WORK")).collect();
        assert_eq!(lines.len(), all.len());
        for line in &lines {
            assert!(line.contains("WORK"), "{line}");
            assert!(!line.contains("paired:") && !line.contains("connect_failed"), "{line}");
        }
    }

    #[test]
    fn dns_failure_names_the_vpn_and_offers_retry() {
        let f = failure(PairedFailure::Unresolved);
        assert_eq!(
            f.summary("WORK"),
            "Can't reach WORK (name did not resolve). Is Tailscale or the VPN on?"
        );
        assert_eq!(f.action(), FailureAction::Retry);
    }

    #[test]
    fn rejected_credentials_offer_repair_not_retry() {
        for kind in [PairedFailure::Revoked, PairedFailure::WrongOrigin, PairedFailure::NoToken] {
            assert_eq!(failure(kind).action(), FailureAction::Repair);
        }
        for kind in [PairedFailure::Unresolved, PairedFailure::TimedOut, PairedFailure::TooOld] {
            assert_eq!(failure(kind).action(), FailureAction::Retry);
        }
    }

    #[test]
    fn unclassified_errors_keep_their_detail_for_the_disclosure() {
        let f = ConnectFailure::from_error(&Error::new(ErrorKind::Timeout, "slow"));
        assert_eq!(f.kind, FailureKind::Other);
        assert_eq!(f.detail, "timeout: slow");
    }
}
