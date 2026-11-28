//! Listing and creating sessions on remote hosts.

use std::sync::mpsc::Sender;

use jcode_sdk::{JcodeClient, SessionInfo};

use crate::harness::remote;

use crate::harness::protocol::{Command, Update};
use crate::harness::updates::UpdateSender;
use crate::platform::screenshot_mode;

/// How a destination reads in a message: a paired machine by the label it was
/// paired under, never by its `paired:<id>`.
fn display_host(host: &str) -> String {
    if crate::remote_targets::paired_id(host).is_some() {
        return crate::paired::find(host)
            .map(|destination| destination.label)
            .unwrap_or_else(|_| "this machine".into());
    }
    host.trim().to_owned()
}

pub(in crate::harness) fn list_remote_sessions(
    host: &str,
    connect: impl FnOnce(&str) -> jcode_sdk::Result<JcodeClient>,
) -> Result<Vec<SessionInfo>, String> {
    let host = crate::remote_targets::validate_destination(host)?;
    let client = connect(&host).map_err(|error| {
        if crate::remote_targets::paired_id(&host).is_some() {
            let label = crate::paired::find(&host)
                .map(|destination| destination.label)
                .unwrap_or_else(|_| "this machine".into());
            crate::connect_failure::ConnectFailure::from_error(&error).summary(&label)
        } else {
            error.to_string()
        }
    })?;
    let address = remote::SessionAddress {
        host: Some(host),
        session_id: String::new(),
    };
    let sessions = client.list_sessions().map_err(|error| error.to_string())?;
    Ok(sessions
        .into_iter()
        .map(|session| address.session_info(session))
        .collect())
}

/// A create is deliberately single-attempt, even for a startup request. An
/// ambiguous timeout must not produce extra sessions on a recovered host.
pub(in crate::harness) fn create_remote_session(
    host: String,
    working_dir: Option<String>,
    request_id: Option<String>,
    updates: UpdateSender,
    internal: Sender<Command>,
    connector: impl FnOnce(&str) -> jcode_sdk::Result<(JcodeClient, bool)>,
) {
    let result = crate::remote_targets::validate_destination(&host)
        .map_err(CreateFailure::from)
        .and_then(|host| {
            let managed = crate::managed_cloud::Destination::parse(&host).is_some();
            let paired = crate::remote_targets::paired_id(&host).is_some();
            let _ = updates.send(Update::RemoteStatus {
                host: host.clone(),
                message: if managed {
                    "Checking your Jcode Cloud machine…".into()
                } else if paired {
                    "Connecting to paired machine…".into()
                } else {
                    format!("Connecting to {host} over SSH...")
                },
                request_id: request_id.clone(),
                failed: false,
                failure: None,
            });
            let (client, self_hosted) = connector(&host).map_err(|error| CreateFailure {
                message: error.to_string(),
                failure: paired.then(|| crate::connect_failure::ConnectFailure::from_error(&error)),
            })?;
            let _ = updates.send(Update::RemoteStatus {
                host: host.clone(),
                message: if managed || paired {
                    "Connected. Creating Jcode session…".into()
                } else {
                    "SSH connected. Creating Jcode session...".into()
                },
                request_id: request_id.clone(),
                failed: false,
                failure: None,
            });
            let working_dir = if paired && working_dir.is_none() {
                Some(crate::paired::find(&host)?.working_dir)
            } else {
                working_dir
            }
            .or_else(|| {
                // The managed bootstrap creates this persistent workspace. Naming
                // it explicitly also lets older cloud bridges report the directory.
                // A self-hosted machine has no such folder and starts in its home.
                (managed && !self_hosted)
                    .then(|| crate::managed_cloud::DEFAULT_WORKSPACE.to_owned())
            });
            let mut session = client
                .create_session(working_dir.clone())
                .map_err(|error| CreateFailure::from(error.to_string()))?;
            if session.working_dir.as_deref().is_none_or(str::is_empty) {
                session.working_dir = working_dir;
            }
            if managed
                && !self_hosted
                && !screenshot_mode()
                && !cfg!(any(test, feature = "test-support"))
            {
                // Start on this computer's default model and effort, not the
                // cloud daemon's. A failure keeps the session on its current model.
                let defaults = crate::managed_cloud::parity::Snapshot::session_defaults();
                if let Err(message) = crate::managed_cloud::parity::apply_to_session(
                    &client,
                    &session.session_id,
                    &defaults,
                ) {
                    let _ = updates.send(Update::RemoteStatus {
                        host: host.clone(),
                        message,
                        request_id: request_id.clone(),
                        failed: false,
                        failure: None,
                    });
                }
            }
            session.session_id = remote::namespace(&host, &session.session_id);
            Ok((session, client))
        });
    match result {
        Ok((session, client)) => {
            let _ = updates.send(Update::RemoteStatus {
                host: host.trim().to_owned(),
                message: format!("Connected to {}", display_host(&host)),
                request_id: request_id.clone(),
                failed: false,
                failure: None,
            });
            let _ = internal.send(Command::CreatedInternal {
                session,
                client,
                request_id,
            });
        }
        Err(error) => {
            let _ = updates.send(Update::RemoteStatus {
                host,
                message: format!("Remote session failed: {}", error.message),
                request_id,
                failed: true,
                failure: error.failure,
            });
        }
    }
}

/// Why a remote create failed: a printable message, plus the typed cause when
/// the destination is a paired machine.
pub(super) struct CreateFailure {
    message: String,
    failure: Option<crate::connect_failure::ConnectFailure>,
}

impl From<String> for CreateFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            failure: None,
        }
    }
}
