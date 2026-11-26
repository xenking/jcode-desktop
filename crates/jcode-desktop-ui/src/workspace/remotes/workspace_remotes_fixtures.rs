//! Screenshot fixtures for the machines panel.
use super::machines::publish_paired_labels;
use super::*;
use crate::managed_cloud::RegisteredMachine;
use jcode_desktop_harness::paired;

impl Workspace {
    /// Offline Machines screen for the screenshot harness: a cloud default,
    /// paired machines in each failure state, one reachable machine and an
    /// SSH host. Failures arrive as the same updates a live connect sends.
    pub(super) fn show_machines_fixture(&mut self, only_paired: bool, cx: &mut Context<Self>) {
        let paired = |id: &str, label: &str, origin: &str, dir: &str| paired::Destination {
            id: id.repeat(32 / id.len()),
            endpoint: format!("https://{origin}"),
            label: label.into(),
            working_dir: dir.into(),
        };
        self.remotes.registered = if only_paired {
            Vec::new()
        } else {
            vec![
                RegisteredMachine {
                    name: "ai-core".into(),
                    is_default: true,
                    last_seen_at: Some(1_795_236_600_000),
                },
                RegisteredMachine {
                    name: "studio".into(),
                    is_default: false,
                    last_seen_at: None,
                },
            ]
        };
        self.remotes.hosts = if only_paired { Vec::new() } else { vec!["build-server".into()] };
        self.remotes.paired = vec![
            paired("aa", "WORK", "work-mac.tailnet.example", "/Users/me/project"),
            paired("bb", "frame", "frame.tailnet.example", "/home/me/frame"),
            paired("cc", "lab", "lab.tailnet.example", "/srv/lab"),
            paired("dd", "personal", "personal.tailnet.example", "/Users/me/notes"),
        ];
        self.remotes.default_host = Some("cloud:ai-core".into());
        publish_paired_labels(&self.remotes.paired);
        let failure = |kind: jcode_sdk::PairedFailure| {
            crate::connect_failure::ConnectFailure::from_error(&jcode_sdk::Error::new(
                jcode_sdk::ErrorKind::Paired(kind),
                format!("{} (fixture)", kind.code()),
            ))
        };
        for (id, outcome) in [
            ("aa", Some(jcode_sdk::PairedFailure::Unresolved)),
            ("bb", Some(jcode_sdk::PairedFailure::Revoked)),
            ("cc", Some(jcode_sdk::PairedFailure::TimedOut)),
            ("dd", None),
        ] {
            let host = format!("paired:{}", id.repeat(16));
            let update = Update::RemoteStatus {
                host,
                message: outcome.map_or_else(|| "Connected to paired machine".into(), |kind| format!("Remote session failed: {}", kind.code())),
                request_id: None,
                failed: outcome.is_some(),
                failure: outcome.map(failure),
            };
            self.apply(update, cx);
        }
        self.remotes.details_open.insert(format!("paired:{}", "cc".repeat(16)));
        self.show_machines(cx);
    }
}
