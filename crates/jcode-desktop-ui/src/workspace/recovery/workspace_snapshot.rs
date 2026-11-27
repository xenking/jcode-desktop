//! The workspace as a value: what is saved across reloads and restarts, and how it is restored.
use super::*;

impl Workspace {
    pub fn snapshot(&self, window: &Window, cx: &App) -> anyhow::Result<WorkspaceSnapshot> {
        self.snapshot_inner(window, cx, false)
    }

    /// How this window was launched, in its restart-manifest form.
    pub(super) fn launch_snapshot(&self) -> LaunchSnapshot {
        LaunchSnapshot::from(&self.launch)
    }

    /// Repaint every chat transcript, for state that renders from outside
    /// the panels, such as applet placements.
    pub(super) fn repaint_chats(&self, cx: &mut Context<Self>) {
        for slot in &self.slots {
            slot.panel.update_chat(cx, |_, cx| cx.notify());
        }
    }

    pub(super) fn snapshot_inner(
        &self,
        window: &Window,
        cx: &App,
        for_reload: bool,
    ) -> anyhow::Result<WorkspaceSnapshot> {
        // Closing slots and transient tool snapshots must not be restored as
        // server sessions. All saved indices refer to persisted slots only.
        let slots: Vec<_> = self
            .slots
            .iter()
            .filter(|slot| {
                !slot.closing
                    && !slot.panel.profile().is_changelog()
                    && slot.panel.read_chat(cx).is_none_or(|panel| {
                        !panel.is_change_review()
                            && !panel.is_accounts_panel()
                            && !panel.is_preview()
                    })
            })
            .collect();
        let index_for_id =
            |id: gpui::EntityId| slots.iter().position(|slot| slot.panel.entity_id() == id);
        let review_source = self.slots.get(self.active).and_then(|slot| {
            if slot.panel.profile().is_changelog() {
                return self.previous.and_then(index_for_id);
            }
            let panel = slot.panel.read_chat(cx)?;
            let source_id = panel.review_source_id()?;
            slots
                .iter()
                .position(|slot| slot.panel.session_id(cx) == source_id)
        });
        let active = self
            .slots
            .get(self.active)
            .and_then(|slot| index_for_id(slot.panel.entity_id()))
            .or(review_source)
            .unwrap_or(0);
        let picker = self.folder_picker(cx);
        let focus = if picker
            .as_ref()
            .is_some_and(|picker| picker.focus_handle(cx).is_focused(window))
        {
            FocusSnapshot::FolderSearch
        } else if let Some(index) = slots
            .iter()
            .position(|slot| slot.panel.surface().input_focus_handle(cx).is_focused(window))
        {
            FocusSnapshot::Panel(index)
        } else if let Some(index) = review_source {
            FocusSnapshot::Panel(index)
        } else {
            FocusSnapshot::Workspace
        };
        Ok(WorkspaceSnapshot {
            launch: Some((&self.launch).into()),
            format_version: SNAPSHOT_FORMAT_VERSION,
            resume: self.resume_snapshot(cx),
            side_panel_snapshots: self.side_panel_snapshots.clone(),
            layout_mode: self.layout_mode,
            recent_accounts: self.recent_accounts.clone(),
            sidebar_closed_sessions: if for_reload {
                self.sidebar_closed_sessions.clone()
            } else {
                HashSet::new()
            },
            sidebar_view: self.sidebar_view,
            session_catalog: self.session_catalog,
            tutorial_page: self.tutorial_page,
            slots: slots
                .iter()
                .map(|slot| SlotSnapshot {
                    panel: slot.panel.profile().snapshot(for_reload, cx),
                    row: slot.row,
                    width_fraction: slot.width_fraction,
                    restore_fraction: slot.restore_fraction,
                })
                .collect(),
            active,
            space_hosts: self.space_hosts.clone(),
            active_row: if review_source.is_some() {
                slots[active].row
            } else {
                self.active_row
            },
            row_focus: self.row_focus.iter().map(|id| id.and_then(index_for_id)).collect(),
            previous: self.previous.and_then(index_for_id),
            last_voice_chat: self.last_voice_chat.and_then(index_for_id),
            camera_x: self.camera_x.clone(),
            camera_target: self.camera_target.clone(),
            overview: self.overview,
            hints_overlay: self.hints_overlay,
            folder_picker_sets_default: picker.as_ref().is_some_and(|p| p.read(cx).sets_default),
            folder_picker_dir: picker.as_ref().map(|p| p.read(cx).dir.clone()),
            folder_picker_error: picker.as_ref().and_then(|p| p.read(cx).checkpoint(cx).0),
            folder_search: picker
                .as_ref()
                .map(|p| p.read(cx).checkpoint(cx).1),
            focus,
        })
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        self.side_panel_snapshots = snapshot.side_panel_snapshots;
        self.layout_mode = snapshot.layout_mode;
        self.recent_accounts = snapshot.recent_accounts;
        self.sidebar_closed_sessions = snapshot.sidebar_closed_sessions;
        self.sidebar_view = snapshot.sidebar_view;
        self.session_catalog = snapshot.session_catalog;
        self.tutorial_page = snapshot.tutorial_page.min(2);
        self.slots.clear();
        // Rows past this machine's own belong to the saved machines, in saved
        // order; machines paired since are appended after them.
        self.space_hosts = snapshot.space_hosts.clone();
        self.add_paired_spaces();
        let flat = snapshot.format_version <= LAST_FLAT_SNAPSHOT_VERSION;
        // Saved indices refer to the saved slot list. Some saved slots are
        // dropped below, so translate every index to the restored slot that
        // holds the same panel rather than whatever now sits at that index.
        let mut restored_index: Vec<Option<usize>> = Vec::with_capacity(snapshot.slots.len());
        for saved in snapshot.slots {
            restored_index.push(None);
            let mut panel_state = saved.panel;
            // Version 1 put every panel in this machine's four rows. A panel of
            // a paired machine moves to the same-numbered workspace of that
            // machine's space.
            let row = if flat {
                crate::harness::remote_host(&panel_state.session_id)
                    .or_else(|| {
                        pending::remote_draft_host(&panel_state.session_id).map(str::to_owned)
                    })
                    .map_or(saved.row, |host| {
                        self.bound_row(Some(&host), saved.row % ROWS_PER_SPACE)
                    })
            } else {
                saved.row
            };
            // Some(needs a session now) for a local draft from the last run.
            let mut restored_local_draft = None;
            // Preview fixture IDs are never runtime sessions, including old snapshots.
            if panel_state.session_id.starts_with("preview://")
                || panel_state.session_id.starts_with("accounts://")
                || panel_state.session_id == ChangelogPanel::SESSION_ID
                // The retired native inbox. Gmail is now an applet panel.
                || panel_state.session_id == "gmail://inbox"
            {
                continue;
            }
            if panel_state.session_id.starts_with("startup://draft/") {
                // The previous bridge generation cannot deliver its in-flight
                // result to this workspace. Restart with a fresh correlation ID
                // and original destination, not today's machine default.
                let help = panel_state.session_id.starts_with("startup://draft/help/");
                let publish = crate::publish::is_publish_draft(&panel_state.session_id);
                let remote_host =
                    pending::remote_draft_host(&panel_state.session_id).map(str::to_owned);
                if let Some(host) = remote_host {
                    let host = machines::new_session_host(&host).to_owned();
                    panel_state.session_id = pending::next_remote_draft_id(&host);
                    self.create_remote_draft_session(host, panel_state.session_id.clone(), cx);
                } else if !panel_state
                    .session_id
                    .starts_with("startup://draft/remote/")
                {
                    panel_state.session_id = pending::next_draft_id();
                    if help {
                        panel_state.session_id = panel_state.session_id.replacen(
                            "startup://draft/",
                            "startup://draft/help/",
                            1,
                        );
                    } else if publish {
                        panel_state.session_id = panel_state.session_id.replacen(
                            "startup://draft/",
                            crate::publish::DRAFT_PREFIX,
                            1,
                        );
                    }
                    restored_local_draft = Some(help || publish);
                }
            }
            if panel_state.terminal_resource_id.is_some()
                || panel_state.session_id == PanelItem::TERMINAL_SESSION_ID
            {
                let host = self.host;
                let terminal = cx.new(|cx| {
                    TerminalPanel::new(
                        panel_state.working_dir,
                        panel_state.terminal_resource_id,
                        panel_state.terminal_output_cursor,
                        host,
                        cx,
                    )
                });
                *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
                let terminal = PanelItem::Terminal(terminal);
                self.slots.push(restored_slot(
                    terminal,
                    row,
                    saved.width_fraction,
                    saved.restore_fraction,
                ));
                continue;
            }
            if let Some(path) = panel_state.session_id.strip_prefix("file://") {
                let file = cx.new(|cx| FilePanel::new(PathBuf::from(path), cx));
                *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
                self.slots.push(restored_slot(
                    PanelItem::File(file),
                    row,
                    saved.width_fraction,
                    saved.restore_fraction,
                ));
                continue;
            }
            let panel = if panel_state.session_id == "unfinished-work" {
                let workspace = cx.weak_entity();
                cx.new(|cx| {
                    Panel::new_unfinished_work(
                        self.unfinished_sessions.clone(),
                        std::sync::Arc::new(move |session, window, cx| {
                            workspace
                                .update(cx, |workspace, cx| {
                                    workspace.activate_unfinished_session(session, window, cx);
                                })
                                .expect("unfinished-work workspace is still available");
                        }),
                        self.bridge.clone(),
                        cx,
                    )
                })
            } else if panel_state.side_document.is_some() {
                cx.new(|cx| {
                    Panel::side_document_from_snapshot(&panel_state, self.bridge.clone(), cx)
                        .expect("checked side document snapshot")
                })
            } else if let Some(instance) = panel_state
                .session_id
                .strip_prefix(crate::panel::applet_panel::APPLET_PREFIX)
            {
                cx.new(|cx| Panel::new_applet(instance.to_owned(), self.bridge.clone(), cx))
            } else if panel_state.session_id == "todoist://tasks" {
                cx.new(|cx| Panel::new_todoist(self.bridge.clone(), cx))
            } else if panel_state.session_id == crate::panel::openrig::SESSION_ID {
                cx.new(|cx| Panel::new_openrig(self.bridge.clone(), cx))
            } else if panel_state.session_id == crate::panel::orchestration::SESSION_ID {
                let opener = self.orchestration_opener(cx);
                cx.new(|cx| Panel::new_orchestration(opener, self.bridge.clone(), cx))
            } else {
                let session_id = panel_state.session_id.clone();
                let title = Some(panel_state.title.clone());
                let working_dir = panel_state.working_dir.clone();
                let panel = cx.new(|cx| {
                    Panel::new(
                        session_id.clone(),
                        title,
                        working_dir,
                        self.bridge.clone(),
                        cx,
                    )
                });
                if !Panel::is_pending_session_id(&session_id)
                    && session_id != Panel::DEFAULT_DIRECTORY_SESSION_ID
                    && session_id != Panel::MACHINES_SESSION_ID
                {
                    self.bridge.send(Command::Watch { session_id });
                }
                panel
            };
            if panel.read(cx).is_machines() {
                let input = self.create_machine_input(cx);
                panel.update(cx, |panel, cx| panel.replace_input(input.clone(), cx));
                self.remotes.input = Some(input);
            } else if !panel.read(cx).is_default_directory() {
                Self::wire_session_panel(&panel, cx);
            }
            panel.update(cx, |panel, cx| {
                panel.restore_snapshot(panel_state, cx);
                panel.prefetch_persisted_history(cx);
                if let Some(needs_session) = restored_local_draft {
                    panel.resume_draft_session(needs_session, cx);
                }
            });
            if panel.read(cx).is_remote_draft() {
                panel.update(cx, |panel, cx| {
                    let host = pending::remote_draft_host(&panel.session_id).map(str::to_owned);
                    let status = match host.as_deref() {
                        Some(host) if machines::managed_destination(host) => {
                            panel.set_title(machines::machine_label(host).to_owned(), cx);
                            format!("{} Your queued prompts are preserved.", machines::MANAGED_CLOUD_STARTING)
                        }
                        Some(host) => format!("Reconnecting to {}… Your queued prompts are preserved.", machines::machine_label(host)),
                        None => "Session creation failed: invalid saved remote destination. No local session was opened.".into(),
                    };
                    panel.set_status(status, cx);
                });
            }
            *restored_index.last_mut().expect("pushed for this slot") = Some(self.slots.len());
            self.slots.push(restored_slot(
                PanelItem::Chat(panel),
                row,
                saved.width_fraction,
                saved.restore_fraction,
            ));
        }
        let restored = |index: usize| restored_index.get(index).copied().flatten();
        self.active_row = snapshot.active_row;
        // Stay on the saved panel. If it was not restorable, stay on the same
        // row rather than jumping to the first panel of the workspace.
        self.active = restored(snapshot.active)
            .or_else(|| snapshot.row_focus[self.active_row].and_then(restored))
            .or_else(|| {
                self.slots
                    .iter()
                    .position(|slot| slot.row == self.active_row)
            })
            .unwrap_or(0);
        if flat && let Some(slot) = self.slots.get(self.active) {
            self.active_row = slot.row;
        }
        self.row_focus = snapshot
            .row_focus
            .iter()
            .map(|index| {
                index
                    .and_then(restored)
                    .map(|index| self.slots[index].panel.entity_id())
            })
            .collect();
        self.previous = snapshot
            .previous
            .and_then(restored)
            .map(|index| self.slots[index].panel.entity_id());
        self.last_voice_chat = snapshot
            .last_voice_chat
            .and_then(restored)
            .map(|index| self.slots[index].panel.entity_id());
        self.camera_x = snapshot.camera_x.clone();
        // Resume from what was actually visible. With no restored animation
        // clock, retaining the old destination would snap there on first paint.
        // The dirty camera resolves a fresh smooth transition if focus needs it.
        self.camera_target = snapshot.camera_x.clone();
        self.camera_from = snapshot.camera_x;
        self.camera_started = vec![None; self.row_count()];
        self.camera_touch_pan = vec![false; self.row_count()];
        self.camera_dirty = vec![true; self.row_count()];
        self.camera_snap_pending = vec![false; self.row_count()];
        // Saved per-workspace state covers the saved rows; give machines paired
        // since their empty ones.
        self.sync_spaces();
        self.overview = snapshot.overview;
        self.overview_progress = AnimatedValue::new(
            if snapshot.overview { 1.0 } else { 0.0 },
            transition::policy(Transition::Overview).duration,
        );
        self.hints_overlay = snapshot.hints_overlay;
        self.hints_progress = AnimatedValue::new(
            if snapshot.hints_overlay { 1.0 } else { 0.0 },
            transition::policy(Transition::Hints).duration,
        );
        self.focus_restore = match snapshot.focus {
            FocusSnapshot::Panel(index) => restored(index)
                .map(FocusSnapshot::Panel)
                .unwrap_or(FocusSnapshot::Workspace),
            focus => focus,
        };
        if let Some(dir) = snapshot.folder_picker_dir {
            let sets_default = snapshot.folder_picker_sets_default;
            let workspace = cx.weak_entity();
            let picker = cx.new(|cx| {
                let mut picker = Self::new_folder_picker(workspace, sets_default, dir, None, cx);
                picker.restore_checkpoint(snapshot.folder_picker_error, snapshot.folder_search, cx);
                picker
            });
            if sets_default {
                if let Some(index) = self.default_directory_panel_index(cx) {
                    let search = picker.read(cx).search.clone();
                    self.slots[index]
                        .panel
                        .update_chat(cx, |panel, cx| panel.replace_input(search, cx));
                }
                self.adopt_default_directory_picker(picker, cx);
            } else {
                self.modal_layer
                    .update(cx, |layer, cx| layer.restore(picker, cx));
            }
        }
        self.restore_workspace_live_views(cx);
        if self.machines_panel_index(cx).is_some() {
            self.refresh_cloud_registry(cx);
        }
    }

    pub fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &self.resume {
            window.focus(&state.search.focus_handle(cx), cx);
            return;
        }
        if self.account_sign_in_visible || self.onboarding_launch.error.is_some() {
            window.focus(&self.focus_handle, cx);
            return;
        }
        match self.focus_restore.clone() {
            FocusSnapshot::Panel(_) if self.overview || self.overview_progress.is_animating() => {
                self.focus_active(window, cx);
            }
            FocusSnapshot::Panel(index) => {
                if let Some(slot) = self.slots.get(index) {
                    let handle = slot.panel.surface().input_focus_handle(cx);
                    window.focus(&handle, cx);
                } else {
                    window.focus(&self.focus_handle, cx);
                }
            }
            FocusSnapshot::FolderSearch => {
                if let Some(picker) = self.folder_picker(cx) {
                    let handle = picker.focus_handle(cx);
                    window.focus(&handle, cx);
                } else {
                    self.focus_active(window, cx);
                }
            }
            FocusSnapshot::Workspace => self.focus_active(window, cx),
        }
    }
}

/// Version 2 adds the machine spaces: rows past this machine's four belong to
/// the paired machines in `space_hosts`. Version 1 files still load; their
/// remote panels move to their machine's space.
pub(super) const SNAPSHOT_FORMAT_VERSION: u32 = 2;
const LAST_FLAT_SNAPSHOT_VERSION: u32 = 1;

/// A slot restored from a snapshot, at its saved place and width.
pub(super) fn restored_slot(
    panel: PanelItem,
    row: usize,
    width_fraction: f32,
    restore_fraction: Option<f32>,
) -> Slot {
    Slot {
        panel,
        row,
        width_fraction,
        animated_width: AnimatedValue::new(
            width_fraction,
            transition::policy(Transition::PanelWidth).duration,
        ),
        order_offset: AnimatedValue::new(0.0, transition::policy(Transition::PanelOrder).duration),
        order_distance_fraction: width_fraction,
        close_progress: AnimatedValue::new(
            1.0,
            transition::policy(Transition::PanelClose).duration,
        ),
        closing: false,
        restore_fraction,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct SlotSnapshot {
    pub(super) panel: PanelSnapshot,
    pub(super) row: usize,
    pub(super) width_fraction: f32,
    pub(super) restore_fraction: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) enum FocusSnapshot {
    Panel(usize),
    FolderSearch,
    #[default]
    Workspace,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkspaceSnapshot {
    pub(super) format_version: u32,
    #[serde(default)]
    pub(super) resume: screens::Snapshot,
    #[serde(default)]
    pub(super) side_panel_snapshots: HashMap<String, side_panel::SidePanelRoutingState>,
    #[serde(default)]
    pub(super) layout_mode: crate::config::LayoutMode,
    #[serde(default)]
    pub(super) recent_accounts: Vec<String>,
    #[serde(default)]
    pub(super) sidebar_closed_sessions: HashSet<String>,
    #[serde(default)]
    pub(super) sidebar_view: SidebarView,
    #[serde(default)]
    pub(super) session_catalog: sidebar_session_catalog::Preferences,
    #[serde(default)]
    pub(super) tutorial_page: usize,
    pub(super) slots: Vec<SlotSnapshot>,
    pub(super) active: usize,
    pub(super) active_row: usize,
    /// Paired machines in space order; absent in version 1.
    #[serde(default)]
    pub(super) space_hosts: Vec<String>,
    pub(super) row_focus: Vec<Option<usize>>,
    pub(super) previous: Option<usize>,
    #[serde(default)]
    pub(super) last_voice_chat: Option<usize>,
    pub(super) camera_x: Vec<f32>,
    pub(super) camera_target: Vec<f32>,
    pub(super) overview: bool,
    pub(super) hints_overlay: bool,
    #[serde(default)]
    pub(super) folder_picker_sets_default: bool,
    pub(super) folder_picker_dir: Option<PathBuf>,
    pub(super) folder_picker_error: Option<String>,
    pub(super) folder_search: Option<PromptInputSnapshot>,
    pub(super) focus: FocusSnapshot,
    /// The window's own launch, kept across Ctrl+R in a shared host. Older
    /// snapshots omit it and fall back to the process launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) launch: Option<LaunchSnapshot>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(super) struct LaunchSnapshot {
    pub(super) args: Vec<String>,
    pub(super) env: Vec<(String, String)>,
}

impl From<&jcode_desktop_api::WindowLaunch> for LaunchSnapshot {
    fn from(launch: &jcode_desktop_api::WindowLaunch) -> Self {
        Self {
            args: launch
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            env: launch.env.clone(),
        }
    }
}

impl From<LaunchSnapshot> for jcode_desktop_api::WindowLaunch {
    fn from(launch: LaunchSnapshot) -> Self {
        Self::from_parts(launch.args, launch.env)
    }
}

impl WorkspaceSnapshot {
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(Into::into)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let snapshot: Self = serde_json::from_slice(bytes)?;
        anyhow::ensure!(
            (LAST_FLAT_SNAPSHOT_VERSION..=SNAPSHOT_FORMAT_VERSION).contains(&snapshot.format_version),
            "unsupported workspace snapshot format {}",
            snapshot.format_version
        );
        anyhow::ensure!(
            snapshot.active_row < snapshot.row_focus.len(),
            "active row is out of range"
        );
        anyhow::ensure!(
            snapshot.row_focus.len() == snapshot.camera_x.len()
                && snapshot.row_focus.len() == snapshot.camera_target.len()
                && !snapshot.row_focus.is_empty(),
            "per-row state does not match the row count"
        );
        anyhow::ensure!(
            snapshot.slots.iter().all(|slot| {
                slot.row < snapshot.row_focus.len()
                    && slot.width_fraction.is_finite()
                    && (0.1..=1.0).contains(&slot.width_fraction)
                    && slot
                        .restore_fraction
                        .is_none_or(|width| width.is_finite() && (0.1..=1.0).contains(&width))
            }),
            "snapshot contains an invalid panel layout"
        );
        anyhow::ensure!(
            snapshot.slots.is_empty() || snapshot.active < snapshot.slots.len(),
            "active panel is out of range"
        );
        Ok(snapshot)
    }
}
