//! Machines: where new sessions go, and what the workspace knows about each one.
//!
//! The lowest layer of the remote surfaces. Pending sessions, the machines panel
//! and the default-directory chip all read it, and it reads none of them.
use super::*;
use crate::login_input::LoginInput;
use crate::managed_cloud::{self, Destination, RegisteredMachine};
use jcode_desktop_harness::paired;

/// Reserved managed destination, never an SSH alias or the personal AWS alpha.
pub(super) const MANAGED_CLOUD_HOST: &str = managed_cloud::HOST;
pub(super) const MANAGED_CLOUD_STARTING: &str = "Checking your Jcode Cloud machine…";

pub(super) fn managed_destination(host: &str) -> bool {
    host == cloud_alpha::HOST || Destination::parse(host).is_some()
}

pub(super) fn new_session_host(host: &str) -> &str {
    if host == cloud_alpha::HOST {
        MANAGED_CLOUD_HOST
    } else {
        host
    }
}

/// Labels of paired machines, so every surface names one by what the owner
/// called it and never by its internal `paired:<id>`.
///
/// Per thread: the UI thread publishes and reads them all, and a process-wide
/// list let parallel tests overwrite each other's machines.
thread_local! {
    static PAIRED_LABELS: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn publish_paired_labels(paired: &[paired::Destination]) {
    PAIRED_LABELS.with_borrow_mut(|labels| {
        *labels = paired.iter().map(|d| (d.host(), d.label.clone())).collect();
    });
}

/// What a destination is called on screen. A paired machine is named by its
/// owner's label and never by its `paired:<id>`; an unknown one reads as a
/// generic machine rather than leaking the id.
pub(crate) fn machine_label(host: &str) -> String {
    if crate::remote_targets::paired_id(host).is_some() {
        return PAIRED_LABELS
            .with_borrow(|labels| {
                labels
                    .iter()
                    .find(|(id, _)| id == host)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| "Paired machine".to_owned());
    }
    Destination::parse(new_session_host(host)).map_or(host, Destination::label).to_owned()
}

/// Destinations whose name the owner chose: managed cloud and paired machines.
pub(crate) fn named_destination(host: &str) -> bool {
    managed_destination(host) || crate::remote_targets::paired_id(host).is_some()
}

/// The last component of a remote project path, for a compact chip.
pub(crate) fn path_tail(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// A failed connect or a rejected edit, shown inside the machine's own row.
#[derive(Clone, Debug)]
pub(super) struct RowNotice {
    pub summary: String,
    pub action: Option<crate::connect_failure::FailureAction>,
    pub detail: Option<String>,
}

pub(super) struct MachineSessions {
    pub(super) destination: String,
    pub(super) request_id: u64,
    pub(super) result: Option<Result<Vec<jcode_sdk::SessionInfo>, String>>,
}

pub(super) struct PairingInput {
    /// Host of the machine being paired again. `None` pairs a new machine.
    pub(super) repair: Option<String>,
    pub(super) endpoint: Entity<PromptInput>,
    pub(super) label: Entity<PromptInput>,
    pub(super) directory: Entity<PromptInput>,
    pub(super) code: Entity<LoginInput>,
}

#[path = "workspace_cloud_alpha.rs"]
pub(super) mod cloud_alpha;

#[path = "workspace_cloud_progress.rs"]
pub(super) mod cloud_progress;

pub(super) fn live_cloud_panel(
    closing: bool,
    session_id: &str,
    history_loaded: bool,
    pending: bool,
    connected: bool,
) -> bool {
    !closing
        && history_loaded
        && !pending
        && connected
        && session_id
            .strip_prefix("ssh://jcode-cloud-alpha/")
            .is_some_and(|id| !id.is_empty())
}

#[derive(Default)]
pub(super) struct Machines {
    pub(super) default_host: Option<String>,
    pub(super) hosts: Vec<String>,
    pub(super) status: Option<String>,
    pub(super) failed: bool,
    pub(super) startup_failed: bool,
    pub(super) startup_requested: bool,
    pub(super) input: Option<Entity<PromptInput>>,
    pub(super) notice: Option<String>,
    #[allow(dead_code)]
    pub(super) discovery: Option<gpui::Task<()>>,
    pub(super) registry_fetch: Option<gpui::Task<()>>,
    pub(super) registered: Vec<RegisteredMachine>,
    pub(super) registry_error: Option<String>,
    pub(super) session_request_id: u64,
    pub(super) cloud_sessions: Option<MachineSessions>,
    pub(super) cloud: cloud_alpha::Lifecycle,
    pub(super) connected_cloud_sessions: std::collections::HashSet<String>,
    pub(super) cloud_progress: std::collections::HashMap<String, cloud_progress::Progress>,
    pub(super) cloud_monitor: Option<gpui::Task<()>>,
    pub(super) paired: Vec<paired::Destination>,
    pub(super) pairing: Option<PairingInput>,
    pub(super) pairing_task: Option<gpui::Task<()>>,
    /// One editable project path per paired machine, keyed by host.
    pub(super) path_inputs: std::collections::HashMap<String, Entity<PromptInput>>,
    /// The latest connect failure or edit error per machine host.
    pub(super) row_notices: std::collections::HashMap<String, RowNotice>,
    pub(super) details_open: std::collections::HashSet<String>,
    /// Machines whose last connect worked in this run.
    pub(super) reachable: std::collections::HashSet<String>,
    /// The machine the current `status` line is about.
    pub(super) status_host: Option<String>,
}

impl Machines {
    pub(super) fn from_config() -> Self {
        let config = crate::config::workspace();
        let mut hosts = config.remote_hosts.clone();
        if let Some(host) = &config.default_remote_host
            && !hosts.contains(host)
        {
            hosts.push(host.clone());
        }
        let paired = paired::list().unwrap_or_default();
        publish_paired_labels(&paired);
        Self {
            paired,
            default_host: config
                .default_remote_host
                .as_deref()
                .map(new_session_host)
                .map(str::to_owned),
            hosts,
            ..Self::default()
        }
    }
}

pub(super) fn registered_machine_status(machine: &RegisteredMachine) -> String {
    let seen = machine
        .last_seen_at
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|at| {
            format!(
                "Last seen {}",
                at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
            )
        })
        .unwrap_or_else(|| "Last seen unknown".into());
    if machine.is_default {
        format!("Account default · {seen}")
    } else {
        seen
    }
}
