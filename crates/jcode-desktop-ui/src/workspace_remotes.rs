//! Machine selection is a workspace preference, not a mutation of existing panels.
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
static PAIRED_LABELS: std::sync::RwLock<Vec<(String, String)>> = std::sync::RwLock::new(Vec::new());

fn publish_paired_labels(paired: &[paired::Destination]) {
    if let Ok(mut labels) = PAIRED_LABELS.write() {
        *labels = paired.iter().map(|d| (d.host(), d.label.clone())).collect();
    }
}

pub(super) fn machine_label(host: &str) -> String {
    if crate::remote_targets::paired_id(host).is_some() {
        return PAIRED_LABELS
            .read()
            .ok()
            .and_then(|labels| labels.iter().find(|(id, _)| id == host).map(|(_, l)| l.clone()))
            .unwrap_or_else(|| "Paired machine".to_owned());
    }
    Destination::parse(new_session_host(host)).map_or(host, Destination::label).to_owned()
}

/// A failed connect or a rejected edit, shown inside the machine's own row.
#[derive(Clone, Debug)]
pub(super) struct RowNotice {
    pub summary: String,
    pub action: Option<crate::connect_failure::FailureAction>,
    pub detail: Option<String>,
}

struct MachineSessions {
    destination: String,
    request_id: u64,
    result: Option<Result<Vec<jcode_sdk::SessionInfo>, String>>,
}

struct PairingInput {
    /// Host of the machine being paired again. `None` pairs a new machine.
    repair: Option<String>,
    endpoint: Entity<PromptInput>,
    label: Entity<PromptInput>,
    directory: Entity<PromptInput>,
    code: Entity<LoginInput>,
}

#[path = "workspace_cloud_alpha.rs"]
mod cloud_alpha;

#[path = "workspace_cloud_progress.rs"]
mod cloud_progress;

fn live_cloud_panel(
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

pub(super) struct HeaderTooltip(pub gpui::SharedString);

impl Render for HeaderTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("HeaderTooltip");
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(Theme::global().PANEL_BG)
            .border_1()
            .border_color(Theme::global().PANEL_BORDER)
            .text_size(px(12.0))
            .text_color(Theme::global().TEXT)
            .child(self.0.clone())
    }
}

#[derive(Default)]
pub(super) struct Machines {
    pub default_host: Option<String>,
    pub hosts: Vec<String>,
    pub status: Option<String>,
    pub failed: bool,
    pub startup_failed: bool,
    startup_requested: bool,
    pub(super) input: Option<Entity<PromptInput>>,
    notice: Option<String>,
    #[allow(dead_code)]
    discovery: Option<gpui::Task<()>>,
    registry_fetch: Option<gpui::Task<()>>,
    registered: Vec<RegisteredMachine>,
    registry_error: Option<String>,
    session_request_id: u64,
    cloud_sessions: Option<MachineSessions>,
    cloud: cloud_alpha::Lifecycle,
    connected_cloud_sessions: std::collections::HashSet<String>,
    cloud_progress: std::collections::HashMap<String, cloud_progress::Progress>,
    cloud_monitor: Option<gpui::Task<()>>,
    paired: Vec<paired::Destination>,
    pairing: Option<PairingInput>,
    pairing_task: Option<gpui::Task<()>>,
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
    pub fn from_config() -> Self {
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

impl Workspace {
    fn text_input(cx: &mut Context<Self>, placeholder: &'static str, content: &str) -> Entity<PromptInput> {
        let content = content.to_owned();
        cx.new(|cx| {
            let mut input = PromptInput::new(cx, placeholder, |_, _, _, _| {})
                .without_chrome()
                .with_compact_padding()
                .without_command_completion();
            if !content.is_empty() {
                input.set_content(content, cx);
            }
            input
        })
    }

    fn open_pairing(&mut self, cx: &mut Context<Self>) {
        self.remotes.pairing = Some(PairingInput {
            repair: None,
            endpoint: Self::text_input(cx, "https://work-mac.example", ""),
            label: Self::text_input(cx, "Work Mac", ""),
            directory: Self::text_input(cx, "/Users/me/project", ""),
            code: cx.new(|cx| LoginInput::new(cx, "Pairing code")),
        });
        cx.notify();
    }

    /// Pair a machine again, keeping its id, label, address and project path.
    fn begin_repair(&mut self, host: &str, cx: &mut Context<Self>) {
        let Some(machine) = self.remotes.paired.iter().find(|m| m.host() == host).cloned() else {
            return;
        };
        self.remotes.row_notices.remove(host);
        self.remotes.notice = None;
        self.remotes.pairing = Some(PairingInput {
            repair: Some(host.to_owned()),
            endpoint: Self::text_input(cx, "https://work-mac.example", &machine.endpoint),
            label: Self::text_input(cx, "Work Mac", &machine.label),
            directory: Self::text_input(cx, "/Users/me/project", &machine.working_dir),
            code: cx.new(|cx| LoginInput::new(cx, "Pairing code")),
        });
        cx.notify();
    }

    /// Make sure every paired machine has an editable project path.
    fn ensure_path_inputs(&mut self, cx: &mut Context<Self>) {
        let hosts: Vec<_> = self.remotes.paired.iter().map(|m| (m.host(), m.working_dir.clone())).collect();
        self.remotes.path_inputs.retain(|host, _| hosts.iter().any(|(h, _)| h == host));
        for (host, dir) in hosts {
            if !self.remotes.path_inputs.contains_key(&host) {
                let input = Self::text_input(cx, "Absolute remote project path", &dir);
                self.remotes.path_inputs.insert(host, input);
            }
        }
        publish_paired_labels(&self.remotes.paired);
    }

    fn row_path(&self, host: &str, cx: &App) -> String {
        self.remotes
            .path_inputs
            .get(host)
            .map(|input| input.read(cx).snapshot().content.trim().to_owned())
            .unwrap_or_default()
    }

    fn save_machine_path(&mut self, host: &str, cx: &mut Context<Self>) {
        let path = self.row_path(host, cx);
        let Some(id) = crate::remote_targets::paired_id(host) else { return };
        match paired::set_working_dir(id, &path) {
            Ok(updated) => {
                if let Some(machine) = self.remotes.paired.iter_mut().find(|m| m.id == updated.id) {
                    *machine = updated;
                }
                self.remotes.row_notices.remove(host);
            }
            Err(error) => {
                self.remotes.row_notices.insert(host.to_owned(), RowNotice { summary: error, action: None, detail: None });
            }
        }
        cx.notify();
    }

    /// Connect from a row. A paired machine starts in the row's project path.
    fn connect_row(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        let Some(host) = host.filter(|h| crate::remote_targets::paired_id(h).is_some()) else {
            return self.connect_machine(None, cx);
        };
        let path = self.row_path(&host, cx);
        if !path.starts_with('/') || path.chars().any(char::is_control) {
            self.remotes.row_notices.insert(host, RowNotice {
                summary: "Enter an absolute remote project path".into(),
                action: None,
                detail: None,
            });
            cx.notify();
            return;
        }
        self.remotes.row_notices.remove(&host);
        self.remotes.reachable.remove(&host);
        self.remotes.status_host = Some(host.clone());
        self.remotes.status = Some("Connecting…".into());
        self.remotes.failed = false;
        self.open_remote_draft_in(host, Some(path), cx);
        cx.notify();
    }

    /// Retry a failed machine without reopening the screen: reuse its pending
    /// draft when one exists, else start a new connect.
    fn retry_machine(&mut self, host: &str, cx: &mut Context<Self>) {
        self.remotes.row_notices.remove(host);
        self.remotes.reachable.remove(host);
        let pending = self.slots.iter().position(|slot| {
            !slot.closing
                && slot.panel.read_chat(cx).is_some_and(|panel| {
                    panel.is_pending_session()
                        && pending::remote_draft_host(&panel.session_id) == Some(host)
                })
        });
        self.remotes.status_host = Some(host.to_owned());
        self.remotes.status = Some("Connecting…".into());
        self.remotes.failed = false;
        match pending {
            Some(index) => self.retry_pending_session(index, false, cx),
            None => self.connect_row(Some(host.to_owned()), cx),
        }
        cx.notify();
    }

    fn submit_pairing(&mut self, cx: &mut Context<Self>) {
        if self.remotes.pairing_task.is_some() {
            return;
        }
        let Some(input) = self.remotes.pairing.as_ref() else {
            return;
        };
        let repair = input.repair.clone();
        let existing_id = repair
            .as_deref()
            .and_then(crate::remote_targets::paired_id)
            .map(str::to_owned);
        let fields = (
            input.endpoint.read(cx).snapshot().content,
            input.label.read(cx).snapshot().content,
            input.directory.read(cx).snapshot().content,
        );
        let destination = match &existing_id {
            Some(id) => paired::Destination::with_id(id.clone(), &fields.0, &fields.1, &fields.2),
            None => paired::Destination::new(&fields.0, &fields.1, &fields.2),
        };
        let destination = match destination {
            Ok(destination) => destination,
            Err(error) => {
                self.remotes.notice = Some(error);
                cx.notify();
                return;
            }
        };
        let code = input.code.update(cx, |input, cx| input.take(cx));
        if code.trim().is_empty() {
            self.remotes.notice = Some("Enter the pairing code".into());
            cx.notify();
            return;
        }
        self.remotes.notice = Some(format!("Pairing with {}…", destination.label));
        self.remotes.pairing_task = Some(cx.spawn(async move |this, cx| {
            let repairing = existing_id.is_some();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let name = std::env::var("HOSTNAME").unwrap_or_else(|_| "Jcode Desktop".into());
                    let result = jcode_sdk::pair_harness(
                        &destination.endpoint,
                        code.trim(),
                        &destination.id,
                        &name,
                    )
                    .map_err(|_| "Pairing failed. Check the address and code.".to_owned())
                    .and_then(|pair| {
                        if repairing {
                            paired::save_repaired(&destination, &pair.token)
                        } else {
                            paired::save(&destination, &pair.token)
                        }
                        .map(|_| destination)
                    });
                    code.into_bytes().fill(0);
                    result
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.remotes.pairing_task = None;
                match result {
                    Ok(destination) => {
                        let host = destination.host();
                        match this.remotes.paired.iter_mut().find(|m| m.id == destination.id) {
                            Some(machine) => *machine = destination,
                            None => this.remotes.paired.push(destination),
                        }
                        this.remotes.path_inputs.remove(&host);
                        this.ensure_path_inputs(cx);
                        this.remotes.row_notices.remove(&host);
                        this.remotes.pairing = None;
                        this.remotes.notice = Some("Machine paired".into());
                    }
                    Err(error) => this.remotes.notice = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_cloud_picker_fixture(&mut self, cx: &mut Context<Self>) {
        self.remotes.registered = vec![
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
        ];
        self.remotes.default_host = Some("cloud:ai-core".into());
        self.show_machines(cx);
    }

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

    pub(super) fn set_cloud_transport_connected(&mut self, session_id: &str, connected: bool) {
        if harness::remote_host(session_id).as_deref() != Some(cloud_alpha::HOST) {
            return;
        }
        if connected {
            self.remotes
                .connected_cloud_sessions
                .insert(session_id.to_owned());
        } else {
            self.remotes.connected_cloud_sessions.remove(session_id);
        }
    }

    pub(super) fn invalidate_cloud_connection(&self, host: &str) {
        if host == cloud_alpha::HOST {
            self.remotes.cloud.invalidate_ready();
        }
    }

    /// Connection progress belongs beside the draft, not only in Machines.
    /// A stale remote result must not change a draft explicitly moved locally.
    pub(super) fn update_startup_status(
        &mut self,
        message: &str,
        failed: bool,
        cx: &mut Context<Self>,
    ) {
        self.remotes.startup_failed = failed;
        for slot in &self.slots {
            if !slot.closing
                && slot
                    .panel
                    .read_chat(cx)
                    .is_some_and(Panel::is_startup_draft)
            {
                slot.panel.update_chat(cx, |panel, cx| {
                    panel.status = message.to_owned();
                    cx.notify();
                });
            }
        }
    }

    pub(super) fn update_pending_remote_status(
        &mut self,
        request_id: Option<&str>,
        message: &str,
        failed: bool,
        cx: &mut Context<Self>,
    ) {
        // Keep only live pending requests. Completed, closed, retried and local
        // replacement panels must never inherit another request's checklist.
        self.remotes.cloud_progress.retain(|id, _| {
            self.slots.iter().any(|slot| {
                !slot.closing
                    && slot
                        .panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_pending_session)
                    && slot.panel.session_id(cx) == *id
            })
        });
        if let Some(id) = request_id {
            let cloud = pending::remote_draft_host(id).is_some_and(managed_destination)
                || (id == Panel::STARTUP_SESSION_ID
                    && self
                        .remotes
                        .default_host
                        .as_deref()
                        .is_some_and(managed_destination));
            let live = self
                .slots
                .iter()
                .any(|slot| !slot.closing && slot.panel.session_id(cx) == id);
            if cloud && live {
                self.remotes
                    .cloud_progress
                    .entry(id.to_owned())
                    .or_default()
                    .observe(message, failed);
            }
        }
        if request_id == Some(Panel::STARTUP_SESSION_ID) {
            self.update_startup_status(message, failed, cx);
        } else if let Some(request_id) = request_id {
            for slot in &self.slots {
                if !slot.closing
                    && slot.panel.session_id(cx) == request_id
                    && slot
                        .panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_pending_session)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        // Helper phases and SSH results use separate channels.
                        // A buffered wake-success phase must not erase an SSH
                        // failure. Retry gets a fresh ID and resets the status.
                        if !failed && panel.status.starts_with("Session creation failed:") {
                            return;
                        }
                        panel.status = if failed {
                            format!("Session creation failed: {message}")
                        } else {
                            message.to_owned()
                        };
                        cx.notify();
                    });
                }
            }
        }
    }

    fn retry_pending_session(&mut self, index: usize, local: bool, cx: &mut Context<Self>) {
        let Some(panel) = self
            .slots
            .get(index)
            .filter(|slot| !slot.closing)
            .and_then(|slot| slot.panel.chat().cloned())
        else {
            return;
        };
        if !panel.read(cx).is_pending_session() {
            return;
        }
        // An explicit retry is a request for the session.
        panel.update(cx, |panel, _| panel.mark_session_requested());
        let remote_host = pending::remote_draft_host(&panel.read(cx).session_id).map(str::to_owned);
        if let Some(host) = remote_host.as_ref().filter(|_| !local) {
            let host = new_session_host(host).to_owned();
            let request_id = pending::next_remote_draft_id(&host);
            panel.update(cx, |panel, cx| {
                panel.session_id = request_id.clone();
                panel.status = format!("Retrying connection to {host}…");
                cx.notify();
            });
            self.create_remote_draft_session(host.clone(), request_id, cx);
        } else if panel.read(cx).is_startup_draft() && !local {
            self.remotes
                .cloud_progress
                .remove(Panel::STARTUP_SESSION_ID);
            self.update_startup_status("Retrying connection to the default machine…", false, cx);
            self.create_default_session(
                self.default_working_dir(),
                Some(Panel::STARTUP_SESSION_ID.into()),
                cx,
            );
        } else {
            // Keep the editor, attachments and queued prompts. A fresh request
            // ID rejects a late remote/create reply rather than changing target.
            let directory = if panel.read(cx).is_startup_draft() || remote_host.is_some() {
                self.default_working_dir()
            } else {
                panel.read(cx).working_dir.clone()
            };
            let mut request_id = pending::next_draft_id();
            if panel
                .read(cx)
                .session_id
                .starts_with("startup://draft/help/")
            {
                request_id = request_id.replacen("startup://draft/", "startup://draft/help/", 1);
            } else if crate::publish::is_publish_draft(&panel.read(cx).session_id) {
                request_id =
                    request_id.replacen("startup://draft/", crate::publish::DRAFT_PREFIX, 1);
            }
            panel.update(cx, |panel, cx| {
                panel.session_id = request_id.clone();
                panel.working_dir = directory.clone();
                panel.title = "New local session".into();
                panel.status = "Connecting to this computer…".into();
                cx.notify();
            });
            self.bridge.send(Command::CreateSession {
                working_dir: directory,
                request_id: Some(request_id),
            });
        }
        cx.notify();
    }

    pub(super) fn render_pending_session(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(panel) = self.slots[index].panel.chat().cloned() else {
            return self.slots[index].panel.view().into_any_element();
        };
        if panel.read(cx).session_deferred() {
            return panel.into_any_element();
        }
        let startup = panel.read(cx).is_startup_draft();
        let host = pending::remote_draft_host(&panel.read(cx).session_id).or_else(|| {
            startup
                .then_some(self.remotes.default_host.as_deref())
                .flatten()
        });
        let remote = host.is_some();
        let cloud = host.is_some_and(managed_destination);
        let failed = if startup {
            self.remotes.startup_failed
        } else {
            panel.read(cx).status.starts_with("Session creation failed")
        };
        let theme = Theme::global();
        let owner = cx.weak_entity();
        let recover = |id: &'static str, label, local| {
            let owner = owner.clone();
            crate::keyboard::press_button(div().id(id), move |window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                let _ = owner.update(cx, |this, cx| {
                    this.retry_pending_session(index, local, cx);
                    this.set_active(index, cx);
                    this.focus_active(window, cx);
                });
            })
            .debug_selector(move || id.into())
            .px_3()
            .py_1()
            .rounded_full()
            .bg(theme.ACCENT_DIM)
            .cursor_pointer()
            .child(label)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("pending-session-status")
                    .debug_selector(|| "pending-session-status".into())
                    .flex_none()
                    .mx_3()
                    .mt_2()
                    .p_2()
                    .rounded_md()
                    .bg(theme.TOOL_BG)
                    .text_size(px(12.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(cloud, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "pending-cloud-label".into())
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(14.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.TEXT)
                                .child(
                                    gpui::svg()
                                        .data(include_bytes!("../../../assets/icons/cloud.svg"))
                                        .w(px(22.))
                                        .h(px(16.))
                                        .text_color(theme.TEXT_DIM),
                                )
                                .child("Jcode Cloud"),
                        )
                    })
                    .child(
                        div()
                            .text_color(if failed { theme.ERROR } else { theme.TEXT_DIM })
                            .child(panel.read(cx).status.clone()),
                    )
                    .when_some(
                        cloud
                            .then(|| {
                                self.remotes
                                    .cloud_progress
                                    .get(&panel.read(cx).session_id)
                            })
                            .flatten(),
                        |el, progress| el.child(progress.render()),
                    )
                    .child(div().text_color(theme.TEXT_DIM).child(if failed {
                        "Not sent. Your draft and queued prompts are preserved."
                    } else {
                        if cloud { "This panel shares your cloud VM. Enter queues your prompt until connected." } else if panel.read(cx).session_deferred() { "Nothing is created until you send. Enter starts the session with your prompt." } else { "Enter queues your prompt until the connection is ready." }
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .when(failed, |el| el.child(recover("pending-session-retry", "Retry connection", false)))
                            .when(cloud && failed, |el| {
                                el.child(crate::keyboard::button(div().id("pending-cloud-account"))
                                    .debug_selector(|| "pending-cloud-account".into())
                                    .px_3().py_1().rounded_full().bg(theme.ACCENT_DIM)
                                    .cursor_pointer().child("Jcode account")
                                    .on_click(|_, _, cx| {
                                        cx.stop_propagation();
                                        cx.open_url("https://jcode.sh/account");
                                    }))
                            })
                            .when(remote, |el| el.child(recover("pending-session-local", "Use this computer", true))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .when(cloud, |el| {
                        el.child(
                            // This is a background watermark, never a hit-test overlay
                            // or a loading spinner that competes with the editable draft.
                            div()
                                .debug_selector(|| "pending-cloud-background".into())
                                .absolute()
                                .inset_0()
                                .flex()
                                .justify_center()
                                .pt(px(32.))
                                .child(
                                    gpui::svg()
                                        .data(include_bytes!("../../../assets/icons/cloud.svg"))
                                        .w(px(300.))
                                        .h(px(188.))
                                        .text_color(theme.TEXT)
                                        .opacity(0.07),
                                ),
                        )
                    })
                    .child(panel),
            )
            .into_any_element()
    }

    pub(super) fn restore_hidden_machine_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (self.overview
            || self.slots.get(self.active).is_none_or(|slot| {
                slot.closing || slot.row != self.active_row
                    || self.panel_metadata(slot, cx).session_id != Panel::MACHINES_SESSION_ID
            }))
            && self
                .remotes
                .input
                .as_ref()
                .is_some_and(|input| input.read(cx).focus_handle.is_focused(window))
        {
            self.focus_active(window, cx);
        }
    }

    pub(super) fn start_default_startup(&mut self, cx: &mut Context<Self>) {
        self.start_cloud_monitor(cx);
        if !self.remotes.startup_requested
            && self.slots.iter().any(|slot| {
                !slot.closing
                    && slot
                        .panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_startup_draft)
            })
        {
            if self.remotes.default_host.is_none() {
                // A local startup draft creates its session with the first
                // prompt. Not marked requested: a machine chosen before then
                // still connects through the branch below.
                for slot in &self.slots {
                    if !slot.closing
                        && slot
                            .panel
                            .read_chat(cx)
                            .is_some_and(Panel::is_startup_draft)
                    {
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.defer_session(cx));
                    }
                }
                return;
            }
            self.remotes.startup_requested = true;
            for slot in &self.slots {
                if !slot.closing
                    && slot
                        .panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_startup_draft)
                {
                    slot.panel
                        .update_chat(cx, |panel, _| panel.mark_session_requested());
                }
            }
            self.create_default_session(
                self.default_working_dir(),
                Some(Panel::STARTUP_SESSION_ID.into()),
                cx,
            );
        }
    }

    /// A local favorite must never be passed to a different filesystem. Remote
    /// defaults start in the remote home, while explicit local folder actions
    /// and terminal panels continue to refer to this computer.
    pub(super) fn create_remote_draft_session(
        &mut self,
        host: String,
        request_id: String,
        cx: &mut Context<Self>,
    ) {
        let host = new_session_host(&host).to_owned();
        if managed_destination(&host) {
            // Restored defaults and drafts from the old personal-alpha button
            // move to the managed destination. They never run the AWS helper.
            let request_id = if pending::remote_draft_host(&request_id) == Some(cloud_alpha::HOST) {
                let managed_id = pending::next_remote_draft_id(MANAGED_CLOUD_HOST);
                for slot in &self.slots {
                    if !slot.closing
                        && slot.panel.session_id(cx) == request_id
                        && slot
                            .panel
                            .read_chat(cx)
                            .is_some_and(Panel::is_pending_session)
                    {
                        slot.panel.update_chat(cx, |panel, cx| {
                            panel.session_id = managed_id.clone();
                            cx.notify();
                        });
                    }
                }
                managed_id
            } else {
                request_id
            };
            if self.remotes.default_host.as_deref() == Some(cloud_alpha::HOST) {
                self.remotes.default_host = Some(MANAGED_CLOUD_HOST.into());
            }
            self.remotes.cloud_progress.remove(&request_id);
            self.update_pending_remote_status(Some(&request_id), MANAGED_CLOUD_STARTING, false, cx);
            self.bridge.send(Command::CreateRemoteSession {
                host,
                working_dir: None,
                request_id: Some(request_id),
            });
        } else {
            let directory = if crate::remote_targets::paired_id(&host).is_some() {
                self.slots
                    .iter()
                    .find(|slot| !slot.closing && slot.panel.session_id(cx) == request_id)
                    .and_then(|slot| slot.panel.working_dir(cx).map(str::to_owned))
            } else {
                None
            };
            self.bridge.send(Command::CreateRemoteSession {
                host,
                working_dir: directory,
                request_id: Some(request_id),
            });
        }
    }

    pub(super) fn create_default_session(
        &mut self,
        local_directory: Option<String>,
        request_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(host) = self
            .remotes
            .default_host
            .as_deref()
            .filter(|host| managed_destination(host))
        {
            let host = new_session_host(host).to_owned();
            self.remotes.default_host = Some(host.clone());
            for slot in &self.slots {
                if Some(slot.panel.session_id(cx)) == request_id.as_deref()
                    && slot
                        .panel
                        .read_chat(cx)
                        .is_some_and(Panel::is_pending_session)
                {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.title = machine_label(&host).to_owned().into();
                        cx.notify();
                    });
                }
            }
            if let Some(id) = &request_id {
                self.remotes.cloud_progress.remove(id);
            }
            self.update_pending_remote_status(
                request_id.as_deref(),
                MANAGED_CLOUD_STARTING,
                false,
                cx,
            );
            self.bridge.send(Command::CreateRemoteSession {
                host,
                working_dir: None,
                request_id,
            });
            return;
        }
        self.bridge.send(match &self.remotes.default_host {
            Some(host) => Command::CreateRemoteSession {
                host: host.clone(),
                working_dir: None,
                request_id,
            },
            None => Command::CreateSession {
                working_dir: local_directory,
                request_id,
            },
        });
    }

    pub(super) fn start_cloud_monitor(&mut self, cx: &mut Context<Self>) {
        if self.remotes.cloud_monitor.is_some() || harness::screenshot_mode() || cfg!(test) {
            return;
        }
        self.remotes.cloud_monitor = Some(cx.spawn(async move |this, cx| {
            let mut previous_summary = String::new();
            loop {
                if this
                    .update(cx, |this, cx| {
                        let active_cloud_panels: Vec<String> = this
                            .slots
                            .iter()
                            .filter_map(|slot| {
                                let panel = slot.panel.read_chat(cx)?;
                                live_cloud_panel(
                                    slot.closing,
                                    &panel.session_id,
                                    panel.history_loaded(),
                                    panel.is_pending_session(),
                                    this.remotes
                                        .connected_cloud_sessions
                                        .contains(&panel.session_id),
                                )
                                .then(|| panel.session_id.clone())
                            })
                            .collect();
                        let configured = !active_cloud_panels.is_empty();
                        this.remotes.cloud.refresh_active(active_cloud_panels);
                        if configured {
                            this.remotes.cloud.refresh();
                        }
                        let updates = this.remotes.cloud.take_updates();
                        let changed = !updates.is_empty();
                        for (message, failed, request_id) in updates {
                            this.update_pending_remote_status(
                                request_id.as_deref(),
                                &message,
                                failed,
                                cx,
                            );
                            this.remotes.status = Some(message);
                            this.remotes.failed = failed;
                        }
                        let summary = if configured {
                            this.remotes.cloud.summary()
                        } else {
                            String::new()
                        };
                        let mut elapsed_changed = false;
                        for progress in this.remotes.cloud_progress.values_mut() {
                            elapsed_changed |= progress.tick();
                        }
                        if summary != previous_summary || changed || elapsed_changed {
                            previous_summary = summary;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        }));
    }

    pub(super) fn open_machines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_machines(cx);
        self.focus_active(window, cx);
    }

    pub(super) fn show_machines(&mut self, cx: &mut Context<Self>) {
        self.start_cloud_monitor(cx);
        self.ensure_path_inputs(cx);
        if let Some(index) = self.machines_panel_index(cx) {
            self.set_active(index, cx);
        } else {
            self.refresh_cloud_registry(cx);
            let input = self.create_machine_input(cx);
            self.remotes.input = Some(input.clone());
            let panel = cx.new(|cx| {
                let mut panel = Panel::new(
                    Panel::MACHINES_SESSION_ID.into(),
                    Some("New session destination".into()),
                    None,
                    self.bridge.clone(),
                    cx,
                );
                panel.input = input;
                panel
            });
            let width_fraction = 0.5;
            let insert_at = if self.slots.is_empty() {
                0
            } else {
                self.active + 1
            };
            self.slots.insert(
                insert_at,
                Slot {
                    panel: PanelItem::Chat(panel),
                    row: self.active_row,
                    width_fraction,
                    animated_width: AnimatedValue::new(
                        width_fraction,
                        transition::policy(Transition::PanelOpen).duration,
                    ),
                    order_offset: AnimatedValue::new(
                        0.0,
                        transition::policy(Transition::PanelOrder).duration,
                    ),
                    order_distance_fraction: width_fraction,
                    close_progress: AnimatedValue::new(
                        1.0,
                        transition::policy(Transition::PanelClose).duration,
                    ),
                    closing: false,
                    restore_fraction: None,
                },
            );
            self.set_active(insert_at, cx);
            crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        }
        self.overview = false;
        self.overview_progress.set(0.0, Instant::now());
        self.focus_pending = true;
        #[cfg(not(test))]
        if !harness::screenshot_mode() {
            self.remotes.discovery = Some(cx.spawn(async move |this, cx| {
                let hosts = cx
                    .background_executor()
                    .spawn(async { crate::remote_targets::discover_hosts() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    for host in hosts {
                        if !this.remotes.hosts.contains(&host) {
                            this.remotes.hosts.push(host);
                        }
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    pub(super) fn refresh_cloud_registry(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || harness::screenshot_mode() {
            return;
        }
        self.remotes.registered.clear();
        self.remotes.cloud_sessions = None;
        self.remotes.registry_error = None;
        self.remotes.registry_fetch = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async {
                    let Ok(account) = managed_cloud::account() else {
                        return Ok(Vec::new());
                    };
                    let result = managed_cloud::list_machines(&account);
                    if managed_cloud::account().is_ok_and(|now| now == account) {
                        result
                    } else {
                        Ok(Vec::new())
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(machines) => this.remotes.registered = machines,
                    Err(error) => this.remotes.registry_error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    fn browse_machine_sessions(&mut self, destination: String, cx: &mut Context<Self>) {
        self.remotes.session_request_id += 1;
        let request_id = self.remotes.session_request_id;
        self.remotes.cloud_sessions = Some(MachineSessions {
            destination: destination.clone(),
            request_id,
            result: None,
        });
        self.bridge.send(Command::ListRemoteSessions {
            host: destination,
            request_id,
        });
        cx.notify();
    }

    pub(super) fn apply_machine_sessions(
        &mut self,
        request_id: u64,
        result: Result<Vec<jcode_sdk::SessionInfo>, String>,
    ) {
        if let Some(list) = self
            .remotes
            .cloud_sessions
            .as_mut()
            .filter(|list| list.request_id == request_id)
        {
            list.result = Some(result);
        }
    }

    pub(super) fn machines_panel_index(&self, cx: &App) -> Option<usize> {
        self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.read_chat(cx).is_some_and(Panel::is_machines)
        })
    }

    pub(super) fn create_machine_input(&self, cx: &mut Context<Self>) -> Entity<PromptInput> {
        let weak = cx.weak_entity();
        let cancel = weak.clone();
        cx.new(|cx| {
            PromptInput::new(cx, "SSH alias or user@hostname", move |host, _, _, app| {
                let _ = weak.update(app, |this, cx| this.connect_typed_ssh(&host, cx));
            })
            .with_on_overlay_cancel(move |app| {
                let _ = cancel.update(app, |this, cx| this.close_machines(cx));
                true
            })
        })
    }

    pub(super) fn close_machines(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.machines_panel_index(cx) else {
            return;
        };
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let removed_id = self.slots.remove(index).panel.entity_id();
        crate::sounds::play(crate::sounds::Cue::PanelClose, cx);
        if self.previous == Some(removed_id) {
            self.previous = None;
        }
        for remembered in &mut self.row_focus {
            if *remembered == Some(removed_id) {
                *remembered = None;
            }
        }
        self.active = active_id
            .filter(|id| *id != removed_id)
            .and_then(|id| {
                self.slots
                    .iter()
                    .position(|slot| slot.panel.entity_id() == id)
            })
            .unwrap_or_else(|| {
                let remaining: Vec<_> = self
                    .row_indices(self.active_row)
                    .filter(|&index| !self.slots[index].closing)
                    .collect();
                focus_after_close(index, &remaining)
            });
        if let Some(slot) = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
        {
            self.row_focus[self.active_row] = Some(slot.panel.entity_id());
        }
        self.remotes.input = None;
        self.remotes.cloud_sessions = None;
        self.remotes.registry_fetch = None;
        self.focus_pending = true;
        self.retarget_camera();
        cx.notify();
    }

    fn connect_typed_ssh(&mut self, host: &str, cx: &mut Context<Self>) {
        match crate::remote_targets::validate_host(host) {
            Ok(host) => self.connect_machine(Some(host), cx),
            Err(error) => {
                self.remotes.notice = Some(error);
                cx.notify();
            }
        }
    }

    pub(super) fn connect_machine(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        let host = match host {
            Some(host) => match crate::remote_targets::validate_destination(&host) {
                Ok(host) => Some(new_session_host(&host).to_owned()),
                Err(error) => {
                    self.remotes.notice = Some(error);
                    cx.notify();
                    return;
                }
            },
            None => None,
        };
        if let Some(host) = host {
            self.remotes.failed = false;
            self.remotes.notice = None;
            if !managed_destination(&host) && crate::remote_targets::paired_id(&host).is_none() && !self.remotes.hosts.contains(&host) {
                self.remotes.hosts.push(host.clone());
                self.remotes.notice = crate::config::persist_remote_hosts(&self.remotes.hosts)
                    .err()
                    .map(|error| format!("Could not save machines: {error}"));
            }
            self.remotes.status = Some(format!("Connecting to {}…", machine_label(&host)));
            self.remotes.status_host = Some(host.clone());
            self.remotes.row_notices.remove(&host);
            self.remotes.reachable.remove(&host);
            self.open_remote_draft(host, cx);
        } else {
            self.remotes.notice = None;
            self.open_local_draft(self.default_working_dir(), cx);
        }
        cx.notify();
    }

    pub(super) fn set_default_machine(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        let host = match host {
            Some(host) => match crate::remote_targets::validate_destination(&host) {
                Ok(host) => Some(new_session_host(&host).to_owned()),
                Err(error) => {
                    self.remotes.notice = Some(error);
                    cx.notify();
                    return;
                }
            },
            None => None,
        };
        match crate::config::persist_default_remote_host(host.as_deref()) {
            Ok(()) => {
                self.remotes.default_host = host;
                self.remotes.notice = None;
            }
            Err(error) => self.remotes.notice = Some(format!("Could not save default: {error}")),
        }
        cx.notify();
    }

    /// The destination row. Folder mode passes the sidebar's section menu and
    /// toggle as `trailing`, making this the sidebar's only header line.
    pub(super) fn render_machine_switcher(
        &self,
        trailing: Option<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        div()
            .debug_selector(|| "machine-destination-switcher".into())
            .flex_none()
            .px_3()
            .py_1p5()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(theme.PANEL_BORDER)
            .text_size(px(10.5))
            .child(
                div()
                    .when(trailing.is_some(), |el| {
                        el.debug_selector(|| "sidebar-navigation-tabs".into())
                    })
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.render_default_directory_button(cx))
                    .children(trailing),
            )
            .when(self.remotes.failed, |el| {
                el.child(
                    div()
                        .text_color(theme.ERROR)
                        .child("Connection needs attention"),
                )
            })
            .children(
                self.remotes
                    .notice
                    .as_ref()
                    .map(|notice| div().text_color(theme.ERROR).child(notice.clone())),
            )
            .children(self.remotes.cloud.lease_warning().map(|warning| {
                div()
                    .debug_selector(|| "cloud-alpha-lease-warning".into())
                    .text_color(Theme::global().ERROR)
                    .child(warning)
            }))
            .into_any_element()
    }

    /// Machine and directory preferences live together, without starting a session.
    fn render_destination_preferences(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let host = self.remotes.default_host.as_deref();
        let mode = |id: &'static str, label: &'static str, selected: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .px_3()
                .py_1()
                .rounded_full()
                .cursor_pointer()
                .bg(if selected {
                    theme.ACCENT_DIM
                } else {
                    theme.TOOL_BG
                })
                .text_color(if selected { theme.ACCENT } else { theme.TEXT })
                .hover(|el| el.bg(theme.ACCENT_DIM))
                .child(label)
        };
        let path = self
            .pinned_working_dir
            .clone()
            .or_else(|| self.default_working_dir())
            .unwrap_or_else(|| "Not set".into());
        div().flex().flex_col().gap_2()
            .child(div().flex().flex_wrap().gap_2()
                .child(mode("machine-destination-local", "This computer", host.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_default_machine(None, cx);
                    })))
                .child(mode("machine-destination-cloud", "Cloud", host.is_some_and(managed_destination))
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_default_machine(Some(MANAGED_CLOUD_HOST.into()), cx);
                    }))))
            .child(div().text_color(theme.TEXT_DIM).child(match host {
                None => "Default directory on this computer".to_owned(),
                Some(host) if managed_destination(host) => format!("{} · ~ (remote home)", machine_label(host)),
                Some(host) => format!("SSH · {host} · ~ (remote home)"),
            }))
            .when(host.is_some(), |el| el.child(
                div().text_color(theme.TEXT_DIM)
                    .child("Remote sessions start in the remote home. Custom remote directories are not configurable yet.")))
            .child(div().id("destination-change-directory")
                .debug_selector(|| "destination-change-directory".into())
                .px_3().py_2().rounded_full().bg(theme.TOOL_BG)
                .cursor_pointer().hover(|el| el.bg(theme.ACCENT_DIM))
                .child(if host.is_none() {
                    format!("{path} · Change directory…")
                } else {
                    format!("Local directory: {path} · Change…")
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_default_directory_picker(window, cx);
                })))
            .into_any_element()
    }

    fn machine_row_exists(&self, host: &str) -> bool {
        self.remotes.default_host.as_deref() == Some(host)
            || host == MANAGED_CLOUD_HOST
            || self.remotes.paired.iter().any(|m| m.host() == host)
            || self.remotes.hosts.iter().any(|h| h == host)
            || self.remotes.registered.iter().any(|m| {
                Destination::named(&m.name).is_some_and(|d| d.preference() == host)
            })
    }

    pub(super) fn render_machines(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let status_in_row = self
            .remotes
            .status_host
            .as_deref()
            .is_some_and(|host| self.machine_row_exists(host));
        let mut picker = div().id("machines-picker").debug_selector(|| "machines-picker".into())
            .size_full().p_3().flex().flex_col().gap_2().overflow_y_scroll().text_size(px(11.0))
            .child(div().flex().items_center().justify_between()
                .child(div().text_size(px(14.0)).child("New session destination"))
                .child(div().id("machines-panel-close").debug_selector(|| "machines-panel-close".into())
                    .flex_none().cursor_pointer().px_2().py_1().rounded_full().bg(theme.TOOL_BG).child("close")
                    .on_click(cx.listener(|this, _, _, cx| { cx.stop_propagation(); this.close_machines(cx); }))))
            .child(div().text_color(theme.TEXT_DIM)
                .child("Choose where new sessions start. Changing the default leaves existing sessions alone."))
            .child(self.render_destination_preferences(cx))
            .children((!status_in_row).then(|| div().debug_selector(|| "machine-connection-status".into())
                .text_color(if self.remotes.failed { theme.ERROR } else { theme.ACCENT })
                .child(self.remotes.status.clone().unwrap_or_default())))
            .children((self.remotes.startup_failed && self.slots.iter().any(|slot| !slot.closing && slot.panel.read_chat(cx).is_some_and(Panel::is_startup_draft))).then(|| {
                div().id("machine-retry-startup").debug_selector(|| "machine-retry-startup".into())
                    .px_3().py_1().rounded_full().cursor_pointer().bg(theme.ACCENT_DIM)
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                        if let Some(index) = this.slots.iter().position(|slot| !slot.closing && slot.panel.read_chat(cx).is_some_and(Panel::is_startup_draft)) {
                            this.retry_pending_session(index, false, cx);
                        }
                    })).child("Retry startup on default machine")
            }))
            .child(div().mt_1().text_size(px(12.0)).child("Machines"))
            .child(self.render_machine_row(None, 0, None, false, cx));
        let cloud_index = self.remotes.hosts.len() + 1;
        picker = picker.child(self.render_machine_row(
            Some(MANAGED_CLOUD_HOST.into()),
            cloud_index,
            None,
            false,
            cx,
        ));
        for (index, machine) in self.remotes.registered.iter().enumerate() {
            if let Some(destination) = Destination::named(&machine.name) {
                picker = picker.child(self.render_machine_row(
                    Some(destination.preference()),
                    cloud_index + index + 1,
                    Some(machine),
                    true,
                    cx,
                ));
            }
        }
        if let Some(error) = &self.remotes.registry_error {
            picker = picker.child(div().text_color(theme.TEXT_DIM).child(error.clone()));
        }
        for (index, machine) in self.remotes.paired.iter().enumerate() {
            picker = picker.child(self.render_machine_row(
                Some(machine.host()),
                cloud_index + self.remotes.registered.len() + index + 1,
                None,
                false,
                cx,
            ));
        }
        for (index, host) in self.remotes.hosts.iter().enumerate().filter(|(_, host)| {
            crate::remote_targets::paired_id(host).is_none()
                && (!managed_destination(host) || host.as_str() == cloud_alpha::HOST)
        }) {
            picker = picker.child(self.render_machine_row(Some(host.clone()), index + 1, None, false, cx));
        }
        picker = picker.child(
            div()
                .id("machine-pair")
                .flex_none()
                .self_start()
                .px_3()
                .py_1()
                .rounded_full()
                .cursor_pointer()
                .bg(theme.ACCENT_DIM)
                .on_click(cx.listener(|this, _, _, cx| this.open_pairing(cx)))
                .child("Pair a machine"),
        );
        if self.remotes.pairing.as_ref().is_some_and(|p| p.repair.is_none()) {
            picker = picker.child(self.render_pairing_form(cx));
        }
        picker = picker.child(div().mt_2().child("Connect another machine"));
        if let Some(input) = &self.remotes.input {
            picker = picker
                .child(
                    div()
                        .id("machine-host-input")
                        .debug_selector(|| "machine-host-input".into())
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                if let Some(input) = &this.remotes.input {
                                    let focus = input.read(cx).focus_handle.clone();
                                    if let Some(index) = this.machines_panel_index(cx) {
                                        this.set_active(index, cx);
                                    }
                                    window.focus(&focus, cx);
                                    this.focus_pending = false;
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .child(input.clone()),
                )
                .child(
                    div()
                        .id("machine-connect-input")
                        .debug_selector(|| "machine-connect-input".into())
                        .flex_none()
                        .self_start()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(theme.ACCENT_DIM)
                        .hover(|el| el.bg(theme.TOOL_BG))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                if let Some(input) = &this.remotes.input {
                                    let host = input.read(cx).snapshot().content;
                                    this.connect_typed_ssh(&host, cx);
                                }
                            }),
                        )
                        .child("Connect via SSH"),
                );
        } else {
            picker = picker.child(
                div()
                    .id("machine-add-host")
                    .px_2()
                    .py_2()
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.open_machines(window, cx);
                        }),
                    )
                    .child("Add SSH host…"),
            );
        }
        picker.child(div().text_color(theme.TEXT_DIM)
                .child("Uses your SSH config and keys. First connect in a terminal to trust the host and set up key access. Jcode must be installed on the remote machine."))
            .child(div().text_color(theme.TEXT_DIM)
                .child("Remote panels start in the remote home. Folder browsing and terminal panels stay local."))
            .children(self.remotes.notice.as_ref().map(|notice| div()
                .debug_selector(|| "machine-error".into()).text_color(theme.ERROR).child(notice.clone())))
            .into_any_element()
    }

    fn render_pairing_form(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let Some(input) = &self.remotes.pairing else {
            return div().into_any_element();
        };
        let repair = input.repair.is_some();
        let field = |element: gpui::AnyElement| {
            div().px_3().py_1().rounded_full().bg(theme.PANEL_BG).child(element)
        };
        div()
            .debug_selector(|| "machine-pairing-form".into())
            .flex()
            .flex_col()
            .gap_1p5()
            .child(field(input.endpoint.clone().into_any_element()))
            .child(field(input.label.clone().into_any_element()))
            .child(field(input.directory.clone().into_any_element()))
            .child(field(input.code.clone().into_any_element()))
            .child(
                div()
                    .flex()
                    .gap_1p5()
                    .child(
                        div()
                            .id("machine-pair-submit")
                            .flex_none()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(theme.ACCENT_DIM)
                            .on_click(cx.listener(|this, _, _, cx| this.submit_pairing(cx)))
                            .child(if repair { "Pair again" } else { "Pair and save" }),
                    )
                    .child(
                        div()
                            .id("machine-pair-cancel")
                            .flex_none()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(theme.PANEL_BG)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.remotes.pairing = None;
                                cx.notify();
                            }))
                            .child("Cancel"),
                    ),
            )
            .into_any_element()
    }

    /// The one row every machine kind uses: name and status on the left,
    /// Connect, Sessions and Default on the right in fixed slots, then any
    /// error, project path or session list that belongs to this machine.
    pub(super) fn render_machine_row(
        &self,
        host: Option<String>,
        index: usize,
        machine: Option<&RegisteredMachine>,
        nested: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        const CONNECT_W: f32 = 66.0;
        const SESSIONS_W: f32 = 74.0;
        const DEFAULT_W: f32 = 92.0;
        let theme = Theme::global();
        let is_default = self.remotes.default_host == host;
        let key = host.clone().map(|h| new_session_host(&h).to_owned()).unwrap_or_default();
        let paired = host
            .as_deref()
            .and_then(|host| self.remotes.paired.iter().find(|m| m.host() == host));
        let label = match host.as_deref() {
            Some(MANAGED_CLOUD_HOST) => "Jcode Cloud".into(),
            Some(cloud_alpha::HOST) => "Personal cloud alpha (existing sessions only)".into(),
            Some(host) => machine_label(host),
            None => "This computer".into(),
        };
        let base_status = if let Some(machine) = machine {
            registered_machine_status(machine)
        } else if host.as_deref() == Some(MANAGED_CLOUD_HOST) {
            if self.remotes.registered.is_empty() {
                "Your own machine, run by Jcode. Sleeps when idle."
            } else {
                "Uses the account’s default machine."
            }
            .to_owned()
        } else if let Some(paired) = paired {
            let origin = paired.endpoint.trim_start_matches("https://");
            format!("Paired · {origin}")
        } else if host.is_none() {
            "Runs on this Mac".to_owned()
        } else {
            "SSH".to_owned()
        };
        let notice = self.remotes.row_notices.get(&key).cloned();
        let connecting = self.remotes.status_host.as_deref() == Some(key.as_str())
            && !self.remotes.failed
            && self.remotes.status.is_some();
        let (status, status_color) = if let Some(notice) = &notice {
            let state = match notice.action {
                Some(crate::connect_failure::FailureAction::Repair) => "Needs pairing",
                _ => "Not reachable",
            };
            (format!("{state} · {base_status}"), theme.ERROR)
        } else if paired.is_some() && self.remotes.reachable.contains(&key) {
            (format!("Reachable · {base_status}"), theme.TEXT_DIM)
        } else if connecting {
            ("Connecting…".to_owned(), theme.ACCENT)
        } else {
            (base_status, theme.TEXT_DIM)
        };
        let browse_host = host
            .clone()
            .filter(|host| machine.is_some() || crate::remote_targets::paired_id(host).is_some());
        let expanded = self.remotes.cloud_sessions.as_ref().is_some_and(|list| {
            Some(&list.destination) == browse_host.as_ref()
                && list.result.as_ref().is_none_or(Result::is_ok)
        });
        let default_host = host.clone();
        let connect_host = host.clone();
        let pill = |width: f32| {
            div()
                .flex_none()
                .w(px(width))
                .py_1()
                .flex()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
        };
        let connect = pill(CONNECT_W)
            .id(("machine-connect", index))
            .debug_selector(move || format!("machine-connect-{index}"))
            .bg(theme.ACCENT_DIM)
            .hover(|el| el.bg(theme.PANEL_BG))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    if connect_host.as_deref().is_some_and(|h| crate::remote_targets::paired_id(h).is_some()) {
                        this.connect_row(connect_host.clone(), cx);
                    } else {
                        this.connect_machine(connect_host.clone(), cx);
                    }
                }),
            )
            // A real runtime can attach the new local panel between press and
            // release. Do not let the old Machines surface reclaim selection.
            .on_mouse_up(gpui::MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child("Connect");
        let sessions = match browse_host {
            Some(host) => pill(SESSIONS_W)
                .id(("machine-sessions", index))
                .debug_selector(move || format!("machine-sessions-{index}"))
                .bg(theme.PANEL_BG)
                .hover(|el| el.bg(theme.ACCENT_DIM))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if expanded {
                        this.remotes.cloud_sessions = None;
                        cx.notify();
                    } else {
                        this.browse_machine_sessions(host.clone(), cx);
                    }
                }))
                .child(if expanded { "Hide" } else { "Sessions" })
                .into_any_element(),
            // Keeps Connect and Default in the same columns on every row.
            None => div().flex_none().w(px(SESSIONS_W)).into_any_element(),
        };
        let default = pill(DEFAULT_W)
            .id(("machine-default", index))
            .debug_selector(move || format!("machine-default-{index}"))
            .bg(theme.PANEL_BG)
            .text_color(if is_default { theme.ACCENT } else { theme.TEXT_DIM })
            .hover(|el| el.bg(theme.ACCENT_DIM))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.set_default_machine(default_host.clone(), cx);
                }),
            )
            .child(if is_default { "✓ Default" } else { "Set default" });
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_x_3()
            .gap_y_1p5()
            .child(
                div()
                    .flex_1()
                    .min_w(px(140.0))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().font_family(theme.FONT_MONO).truncate().child(label.clone()))
                    .child(div().debug_selector(move || format!("machine-status-{index}")).truncate().text_color(status_color).child(status)),
            )
            .child(
                div()
                    .flex_none()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(connect)
                    .child(sessions)
                    .child(default),
            );
        let path_line = paired.map(|machine| {
            let host = machine.host();
            let saved = machine.working_dir.clone();
            let dirty = self.row_path(&host, cx) != saved;
            let focus_host = host.clone();
            let save_host = host.clone();
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_none().text_color(theme.TEXT_DIM).child("Project"))
                .child(
                    div()
                        .id(("machine-path", index))
                        .debug_selector(move || format!("machine-path-{index}"))
                        .flex_1()
                        .min_w_0()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .bg(theme.PANEL_BG)
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                if let Some(input) = this.remotes.path_inputs.get(&focus_host) {
                                    let focus = input.read(cx).focus_handle.clone();
                                    if let Some(i) = this.machines_panel_index(cx) {
                                        this.set_active(i, cx);
                                    }
                                    window.focus(&focus, cx);
                                    this.focus_pending = false;
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .children(self.remotes.path_inputs.get(&host).cloned()),
                )
                .children(dirty.then(|| {
                    div()
                        .id(("machine-path-save", index))
                        .debug_selector(move || format!("machine-path-save-{index}"))
                        .flex_none()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(theme.ACCENT_DIM)
                        .on_click(cx.listener(move |this, _, _, cx| this.save_machine_path(&save_host, cx)))
                        .child("Save")
                }))
        });
        let notice_block = notice.map(|notice| {
            let retry_host = key.clone();
            let details_host = key.clone();
            let open = self.remotes.details_open.contains(&key);
            let (action_label, repair) = match notice.action {
                Some(crate::connect_failure::FailureAction::Retry) => (Some("Retry"), false),
                Some(crate::connect_failure::FailureAction::Repair) => (Some("Re-pair"), true),
                None => (None, false),
            };
            div()
                .debug_selector(move || format!("machine-notice-{index}"))
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_3()
                        .gap_y_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(180.0))
                                .text_color(theme.ERROR)
                                .child(notice.summary.clone()),
                        )
                        .child(
                            div()
                        .flex_none()
                        .ml_auto()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .children(action_label.map(|label| {
                            div()
                                .id(("machine-notice-action", index))
                                .debug_selector(move || format!("machine-notice-action-{index}"))
                                .flex_none()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(theme.ACCENT_DIM)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if repair {
                                        this.begin_repair(&retry_host, cx);
                                    } else {
                                        this.retry_machine(&retry_host, cx);
                                    }
                                }))
                                .child(label)
                        }))
                        .children(notice.detail.clone().map(|_| {
                            div()
                                .id(("machine-notice-details", index))
                                .flex_none()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(theme.PANEL_BG)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.remotes.details_open.remove(&details_host) {
                                        this.remotes.details_open.insert(details_host.clone());
                                    }
                                    cx.notify();
                                }))
                                .child(if open { "Hide details" } else { "Details" })
                        })),
                        ),
                )
                .children(open.then(|| notice.detail.clone()).flatten().map(|detail| {
                    div().text_color(theme.TEXT_DIM).font_family(theme.FONT_MONO).child(detail)
                }))
        });
        let repair_form = host
            .as_deref()
            .filter(|h| self.remotes.pairing.as_ref().and_then(|p| p.repair.as_deref()) == Some(*h))
            .map(|_| self.render_pairing_form(cx));
        let sessions_list = host.as_deref().and_then(|h| self.render_machine_sessions(h, cx));
        div()
            .when(nested, |el| el.ml_3())
            .debug_selector(move || format!("machine-row-{index}"))
            .px_3()
            .py_2()
            .rounded_xl()
            .bg(theme.TOOL_BG)
            .flex()
            .flex_col()
            .gap_1p5()
            .child(header)
            .children(path_line)
            .children(notice_block)
            .children(repair_form)
            .children(sessions_list)
            .into_any_element()
    }

    fn render_machine_sessions(
        &self,
        host: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let list = self
            .remotes
            .cloud_sessions
            .as_ref()
            .filter(|list| list.destination == host)?;
        let theme = Theme::global();
        let content = div()
            .pl_3()
            .flex()
            .flex_col()
            .gap_1()
            .text_color(theme.TEXT_DIM);
        Some(
            match &list.result {
                None => content.child("Loading sessions…"),
                Some(Err(error)) => {
                    content.child(div().text_color(theme.ERROR).child(error.clone()))
                }
                Some(Ok(sessions)) if sessions.is_empty() => {
                    content.child("No sessions on this machine")
                }
                Some(Ok(sessions)) => {
                    content.children(sessions.iter().enumerate().map(|(index, session)| {
                        let session = session.clone();
                        let title = session
                            .title
                            .clone()
                            .or_else(|| session.working_dir.clone())
                            .unwrap_or_else(|| "Untitled session".into());
                        div()
                            .id(("cloud-open-session", index))
                            .debug_selector(move || format!("cloud-open-session-{index}"))
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .truncate()
                            .bg(theme.TOOL_BG)
                            .hover(|el| el.bg(theme.ACCENT_DIM))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_session(session.clone(), window, cx);
                            }))
                            .child(title)
                    }))
                }
            }
            .into_any_element(),
        )
    }
}

fn registered_machine_status(machine: &RegisteredMachine) -> String {
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

#[cfg(test)]
mod cloud_routing_tests {
    use super::super::remote_tests::{click, session};
    use super::*;

    const WORK_ID: &str = "0123456789abcdef0123456789abcdef";
    const WORK_HOST: &str = "paired:0123456789abcdef0123456789abcdef";

    fn paired_workspace(
        cx: &mut gpui::TestAppContext,
    ) -> (
        Entity<Workspace>,
        &mut gpui::VisualTestContext,
        std::sync::mpsc::Receiver<Command>,
    ) {
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = bridge;
            w.remotes = Machines::default();
            w.remotes.paired.push(paired::Destination {
                id: WORK_ID.into(),
                endpoint: "https://work.example".into(),
                label: "WORK".into(),
                working_dir: "/work/default".into(),
            });
            w.show_machines(cx);
            w
        });
        (workspace, vcx, commands)
    }

    fn fail(
        workspace: &Entity<Workspace>,
        vcx: &mut gpui::VisualTestContext,
        kind: jcode_sdk::PairedFailure,
    ) {
        workspace.update(vcx, |w, cx| {
            w.apply(
                Update::RemoteStatus {
                    host: WORK_HOST.into(),
                    message: format!("Remote session failed: {kind:?}"),
                    request_id: None,
                    failed: true,
                    failure: Some(crate::connect_failure::ConnectFailure::from_error(
                        &jcode_sdk::Error::new(jcode_sdk::ErrorKind::Paired(kind), "cause"),
                    )),
                },
                cx,
            );
        });
        vcx.run_until_parked();
        // Machines slides into view. Settle it so the row has painted bounds.
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
    }

    #[gpui::test]
    fn paired_machine_creates_sessions_in_multiple_remote_projects(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx, commands) = paired_workspace(cx);
        for path in ["/work/one", "/work/two"] {
            workspace.update(vcx, |w, cx| {
                w.remotes.path_inputs[WORK_HOST].update(cx, |input, cx| input.set_content(path.into(), cx));
            });
            click(vcx, "machine-connect-2");
            assert!(matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: Some(dir), .. })
                if host == WORK_HOST && dir == path));
        }
    }

    #[gpui::test]
    fn machine_rows_keep_their_actions_inside_the_row_at_narrow_and_wide_widths(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx, _commands) = paired_workspace(cx);
        fail(&workspace, vcx, jcode_sdk::PairedFailure::Unresolved);
        for width in [420.0, 700.0, 1600.0] {
            vcx.simulate_resize(gpui::size(px(width), px(900.0)));
            vcx.executor().advance_clock(Duration::from_secs(2));
            vcx.run_until_parked();
            let row = vcx.debug_bounds("machine-row-2").unwrap();
            let mut previous_right = None;
            for selector in ["machine-connect-2", "machine-sessions-2", "machine-default-2"] {
                let pill = vcx.debug_bounds(selector).unwrap();
                assert!(
                    pill.left() >= row.left() && pill.right() <= row.right(),
                    "{selector} escapes its row at {width}: {pill:?} vs {row:?}"
                );
                if let Some(right) = previous_right {
                    assert!(pill.left() >= right, "{selector} overlaps its neighbour at {width}");
                }
                previous_right = Some(pill.right());
            }
            let status = vcx.debug_bounds("machine-status-2").unwrap();
            let connect = vcx.debug_bounds("machine-connect-2").unwrap();
            let same_line = status.top() < connect.bottom() && connect.top() < status.bottom();
            assert!(
                !same_line || status.right() <= connect.left(),
                "status runs under the actions at {width}: {status:?} vs {connect:?}"
            );
        }
    }

    #[gpui::test]
    fn paired_machine_path_is_editable_and_must_be_absolute(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx, commands) = paired_workspace(cx);
        assert!(vcx.debug_bounds("machine-path-save-2").is_none());
        workspace.update(vcx, |w, cx| {
            w.remotes.path_inputs[WORK_HOST].update(cx, |input, cx| input.set_content("relative".into(), cx));
        });
        click(vcx, "machine-connect-2");
        assert!(commands.try_recv().is_err());
        assert!(vcx.debug_bounds("machine-notice-2").is_some());
        assert!(vcx.debug_bounds("machine-path-save-2").is_some());
    }

    #[gpui::test]
    fn unreachable_paired_machine_shows_its_cause_inside_its_row_and_retry_reconnects(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx, commands) = paired_workspace(cx);
        fail(&workspace, vcx, jcode_sdk::PairedFailure::Unresolved);
        workspace.read_with(vcx, |w, _| {
            let notice = &w.remotes.row_notices[WORK_HOST];
            assert_eq!(
                notice.summary,
                "Can't reach WORK (name did not resolve). Is Tailscale or the VPN on?"
            );
            assert!(!w.remotes.status.clone().unwrap().contains("paired:"));
        });
        assert!(vcx.debug_bounds("machine-notice-2").is_some());
        assert!(vcx.debug_bounds("machine-connection-status").is_none());
        click(vcx, "machine-notice-action-2");
        assert!(matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, .. }) if host == WORK_HOST));
        assert!(vcx.debug_bounds("machines-picker").is_some());
        // The next attempt's progress clears the old error from the row.
        workspace.update(vcx, |w, cx| {
            w.apply(
                Update::RemoteStatus {
                    host: WORK_HOST.into(),
                    message: "Connected to WORK".into(),
                    request_id: None,
                    failed: false,
                    failure: None,
                },
                cx,
            );
        });
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("machine-notice-2").is_none());
        workspace.read_with(vcx, |w, _| assert!(w.remotes.reachable.contains(WORK_HOST)));
    }

    #[gpui::test]
    fn revoked_paired_machine_offers_repair_inside_its_row(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx, commands) = paired_workspace(cx);
        fail(&workspace, vcx, jcode_sdk::PairedFailure::Revoked);
        workspace.read_with(vcx, |w, _| {
            assert_eq!(w.remotes.row_notices[WORK_HOST].summary, "WORK no longer accepts this device. Pair it again.");
        });
        click(vcx, "machine-notice-action-2");
        assert!(commands.try_recv().is_err());
        workspace.read_with(vcx, |w, cx| {
            let form = w.remotes.pairing.as_ref().unwrap();
            assert_eq!(form.repair.as_deref(), Some(WORK_HOST));
            assert_eq!(form.endpoint.read(cx).snapshot().content, "https://work.example");
            assert_eq!(form.label.read(cx).snapshot().content, "WORK");
        });
        assert!(vcx.debug_bounds("machine-pairing-form").is_some());
    }

    #[gpui::test]
    fn machines_picker_routes_named_cloud_connect_default_and_session_open(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.bridge = bridge;
            workspace.remotes = Machines::default();
            workspace.show_machines(cx);
            workspace
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("machine-connect-1").is_some());
        assert!(vcx.debug_bounds("machine-connect-2").is_none());

        workspace.update(vcx, |w, cx| {
            w.remotes.registered = vec![
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
            ];
            cx.notify();
        });
        click(vcx, "machine-default-2");
        workspace.read_with(vcx, |w, _| {
            assert_eq!(w.remotes.default_host.as_deref(), Some("cloud:ai-core"))
        });
        assert!(commands.try_recv().is_err());
        click(vcx, "machine-sessions-2");
        let first = match commands.try_recv().unwrap() {
            Command::ListRemoteSessions { host, request_id } if host == "cloud:ai-core" => {
                request_id
            }
            _ => panic!("expected ai-core catalog request"),
        };
        click(vcx, "machine-sessions-3");
        let second = match commands.try_recv().unwrap() {
            Command::ListRemoteSessions { host, request_id } if host == "cloud:studio" => {
                request_id
            }
            _ => panic!("expected studio catalog request"),
        };
        workspace.update(vcx, |w, cx| {
            w.apply_machine_sessions(first, Ok(vec![session("cloud://ai-core/not-current")]));
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("cloud-open-session-0").is_none());
        workspace.update(vcx, |w, cx| {
            w.apply_machine_sessions(second, Ok(vec![session("cloud://studio/session-existing")]));
            cx.notify();
        });
        click(vcx, "cloud-open-session-0");
        assert!(
            matches!(commands.try_recv(), Ok(Command::Watch { session_id }) if session_id == "cloud://studio/session-existing")
        );

        click(vcx, "machines-picker-button");
        click(vcx, "machine-connect-3");
        assert!(
            matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: None, .. }) if host == "cloud:studio")
        );
        workspace.update(vcx, |w, cx| w.open_default_draft(None, cx));
        assert!(
            matches!(commands.try_recv(), Ok(Command::CreateRemoteSession { host, working_dir: None, .. }) if host == "cloud:ai-core")
        );
    }

    #[test]
    fn cloud_active_detector_requires_connected_nonclosing_exact_namespace() {
        let id = "ssh://jcode-cloud-alpha/session";
        assert!(live_cloud_panel(false, id, true, false, true));
        assert!(!live_cloud_panel(false, id, true, false, false));
        assert!(live_cloud_panel(false, id, true, false, true));
        // A lost old panel must not keep checks alive once a new live one closes.
        assert!(
            ![(false, false), (true, true)]
                .into_iter()
                .any(|(closing, connected)| live_cloud_panel(closing, id, true, false, connected))
        );
        assert!(!live_cloud_panel(true, id, true, false, true));
        assert!(!live_cloud_panel(false, id, false, false, true));
        assert!(!live_cloud_panel(false, id, true, true, true));
        for id in [
            "local",
            "startup://draft",
            "ssh://other/session",
            "ssh://jcode-cloud-alpha-evil/session",
            "ssh://jcode-cloud-alpha/",
        ] {
            assert!(!live_cloud_panel(false, id, true, false, true));
        }
    }

    #[gpui::test]
    fn managed_and_legacy_cloud_defaults_connect_only_to_managed_cloud(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, commands) = harness::spawn_recording();
        let workspace = cx.new(|cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.bridge = bridge;
            workspace
        });
        workspace.update(cx, |workspace, cx| {
            for host in [MANAGED_CLOUD_HOST, cloud_alpha::HOST] {
                workspace.remotes.default_host = Some(host.into());
                workspace.create_default_session(
                    Some("/local-only".into()),
                    Some(Panel::STARTUP_SESSION_ID.into()),
                    cx,
                );
                assert!(
                    matches!(
                        commands.try_recv(),
                        Ok(Command::CreateRemoteSession { host, working_dir: None, request_id: Some(id) })
                            if host == MANAGED_CLOUD_HOST && id == Panel::STARTUP_SESSION_ID
                    ),
                    "cloud must never fall back to SSH aliases, the AWS alpha, or local"
                );
                assert!(commands.try_recv().is_err());
                assert!(
                    workspace.remotes.cloud.take_updates().is_empty(),
                    "must not invoke the personal AWS helper"
                );
                assert!(!workspace.remotes.startup_failed);
                assert_eq!(
                    workspace.remotes.default_host.as_deref(),
                    Some(MANAGED_CLOUD_HOST)
                );
            }
        });
    }

    #[gpui::test]
    fn managed_and_legacy_cloud_connect_open_a_managed_draft(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = harness::spawn_recording();
        let workspace = cx.new(|cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.bridge = bridge;
            workspace
        });
        workspace.update(cx, |workspace, cx| {
            for host in [MANAGED_CLOUD_HOST, cloud_alpha::HOST] {
                workspace.connect_machine(Some(host.into()), cx);
                let panel = workspace.slots[workspace.active].panel.read(cx);
                let id = panel.session_id.clone();
                assert_eq!(pending::remote_draft_host(&id), Some(MANAGED_CLOUD_HOST));
                assert!(panel.status.contains(MANAGED_CLOUD_STARTING));
                assert!(matches!(
                    commands.try_recv(),
                    Ok(Command::CreateRemoteSession { host, request_id: Some(request), .. })
                        if host == MANAGED_CLOUD_HOST && request == id
                ));
                assert!(workspace.remotes.cloud.take_updates().is_empty());
            }
        });
    }

    #[test]
    fn managed_names_never_rewrite_existing_ssh_session_ids() {
        assert_eq!(new_session_host(cloud_alpha::HOST), MANAGED_CLOUD_HOST);
        assert_eq!(new_session_host("my-server"), "my-server");
        assert!(!managed_destination("jcode-cloud-evil"));
        assert!(!managed_destination("ssh://jcode-cloud-alpha/session"));
    }
}
