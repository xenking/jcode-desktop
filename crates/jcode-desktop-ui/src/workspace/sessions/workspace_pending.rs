//! Session creation never holds up the native editor. Draft IDs also
//! identify replies after concurrent creates, closure, and hot reload.
use super::machines::{self, cloud_alpha, *};
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn next_draft_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Time distinguishes reloaded cdylibs, whose statics start over, while the
    // counter distinguishes requests even on a coarse system clock.
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "startup://draft/{epoch}-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn next_remote_draft_id(host: &str) -> String {
    next_draft_id().replacen(
        "startup://draft/",
        &format!("startup://draft/remote/{host}/"),
        1,
    )
}

/// The destination travels with the draft through reloads and default-machine
/// changes. Valid destinations cannot contain '/', so no escaping is needed.
pub(super) fn remote_draft_host(id: &str) -> Option<&str> {
    let (host, request) = id
        .strip_prefix("startup://draft/remote/")?
        .split_once('/')?;
    (!request.is_empty() && crate::remote_targets::validate_destination(host).is_ok())
        .then_some(host)
}

impl Workspace {
    pub(super) fn open_default_draft(&mut self, directory: Option<String>, cx: &mut Context<Self>) {
        if let Some(host) = self.remotes.default_host.clone() {
            self.open_remote_draft(host, cx);
        } else {
            self.open_local_draft(directory, cx);
        }
    }

    pub(super) fn open_local_draft(&mut self, directory: Option<String>, cx: &mut Context<Self>) {
        self.open_local_draft_kind(directory, false, cx);
    }

    pub(super) fn open_local_draft_kind(
        &mut self,
        directory: Option<String>,
        help: bool,
        cx: &mut Context<Self>,
    ) {
        let mut request_id = next_draft_id();
        if help {
            request_id = request_id.replacen("startup://draft/", "startup://draft/help/", 1);
        }
        self.mount_session_draft(
            request_id.clone(),
            directory.clone(),
            "New session".into(),
            cx,
        );
        let panel = self.slots[self.active].panel.clone();
        if help {
            // Help sends its own first prompt as soon as the session exists.
            panel.update_chat(cx, |panel, _| panel.mark_session_requested());
            self.bridge.send(Command::CreateSession {
                working_dir: directory,
                request_id: Some(request_id),
            });
        } else {
            panel.update_chat(cx, |panel, cx| panel.defer_session(cx));
        }
    }

    /// Stand in for the first prompt of every draft that is waiting for one.
    #[cfg(test)]
    pub(super) fn request_deferred_sessions(&mut self, cx: &mut Context<Self>) -> usize {
        let mut requested = 0;
        for slot in &self.slots {
            if slot
                .panel
                .read_chat(cx)
                .is_some_and(Panel::session_deferred)
            {
                slot.panel
                    .update_chat(cx, |panel, cx| panel.request_deferred_session(cx));
                requested += 1;
            }
        }
        requested
    }

    pub(super) fn open_remote_draft(&mut self, host: String, cx: &mut Context<Self>) {
        self.open_remote_draft_in(host, None, cx);
    }

    pub(super) fn open_remote_draft_in(
        &mut self,
        host: String,
        directory: Option<String>,
        cx: &mut Context<Self>,
    ) {
        // The old Cloud button stored a personal AWS alias. New drafts must
        // never turn that legacy preference into an implicit AWS operation.
        // Existing live SSH sessions keep their original destination.
        let host = if host == "jcode-cloud-alpha" {
            if self.remotes.default_host.as_deref() == Some("jcode-cloud-alpha") {
                self.remotes.default_host = Some(machines::MANAGED_CLOUD_HOST.into());
            }
            machines::MANAGED_CLOUD_HOST.to_owned()
        } else {
            host
        };
        let request_id = next_remote_draft_id(&host);
        self.remotes.failed = false;
        self.remotes.status = Some(format!(
            "Preparing connection to {}…",
            machines::machine_label(&host)
        ));
        self.mount_session_draft(
            request_id.clone(),
            directory,
            if machines::named_destination(&host) {
                machines::machine_label(&host).into()
            } else {
                format!("Connecting to {host}…")
            },
            cx,
        );
        let panel = self.slots[self.active].panel.clone();
        panel.update_chat(cx, |panel, cx| {
            panel.set_status(format!("Preparing connection to {}…", machines::machine_label(&host)), cx);
        });
        self.create_remote_draft_session(host, request_id, cx);
    }

    pub(super) fn mount_session_draft(
        &mut self,
        request_id: String,
        directory: Option<String>,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let inserted = self.open_session(
            jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: request_id.clone(),
                title: Some(title),
                working_dir: directory.clone(),
                status: "starting".into(),
                transcript_bytes: None,
                saved: false,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                save_label: None,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                edit_stats: None,
                context: None,
            },
            cx,
        );
        // The editor must be hit-testable and visible in the very next frame,
        // rather than waiting for either backend creation or a width tween.
        // Snap sibling demotions too. Otherwise the old full-width panel can
        // push this full-width editor beyond the clip until its tween finishes.
        for slot in self
            .slots
            .iter_mut()
            .filter(|slot| slot.row == self.active_row)
        {
            slot.animated_width = AnimatedValue::new(
                slot.width_fraction,
                transition::policy(Transition::PanelOpen).duration,
            );
        }
        // Keep a spatial entrance without shrinking the native editor into a
        // sliver. Only offset the newcomer, so rapid spawns preserve earlier
        // entrances. The existing order tween also drives animation frames and
        // hit testing, and respects the reduced-motion PanelOpen policy.
        let slot = &mut self.slots[inserted];
        slot.order_offset =
            AnimatedValue::new(0.12, transition::policy(Transition::PanelOpen).duration);
        slot.order_offset.set(0.0, Instant::now());
        self.set_active(inserted, cx);
        // The viewport width is only known in render_strip. Resolve and snap
        // its camera there instead of interpolating an offscreen editor in.
        self.camera_snap_pending[self.active_row] = true;
        self.focus_pending = true;
        cx.notify();
    }
}

impl Workspace {
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
                slot.panel
                    .update_chat(cx, |panel, cx| panel.set_status(message, cx));
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
                    // Helper phases and SSH results use separate channels.
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.note_connection_phase(message, failed, cx)
                    });
                }
            }
        }
    }

    pub(super) fn retry_pending_session(&mut self, index: usize, local: bool, cx: &mut Context<Self>) {
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
                panel.restart_session_request(
                    request_id.clone(),
                    format!("Retrying connection to {}…", machines::machine_label(&host)),
                    cx,
                );
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
            if panel.read(cx).is_help_draft() {
                request_id = request_id.replacen("startup://draft/", "startup://draft/help/", 1);
            } else if crate::publish::is_publish_draft(&panel.read(cx).session_id) {
                request_id =
                    request_id.replacen("startup://draft/", crate::publish::DRAFT_PREFIX, 1);
            }
            panel.update(cx, |panel, cx| {
                panel.reconnect_locally(request_id.clone(), directory.clone(), cx);
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
            return self.slots[index].panel.surface().view().into_any_element();
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
            panel.read(cx).creation_failed()
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
                                        .data(include_bytes!("../../../../../assets/icons/cloud.svg"))
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
                                        .data(include_bytes!("../../../../../assets/icons/cloud.svg"))
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
                        slot.panel
                            .update_chat(cx, |panel, cx| panel.adopt_request(managed_id.clone(), cx));
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
                    .and_then(|slot| slot.panel.profile().working_dir(cx).map(str::to_owned))
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
                        panel.set_title(machine_label(&host).to_owned(), cx)
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
                        let cloud = this.remotes.cloud.clone();
                        let changed = cloud.drain_updates(|message, failed, request_id| {
                            this.update_pending_remote_status(
                                request_id.as_deref(),
                                &message,
                                failed,
                                cx,
                            );
                            this.remotes.status = Some(message);
                            this.remotes.failed = failed;
                        });
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
}
