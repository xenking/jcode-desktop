//! One space per machine in one window: this machine's workspaces first, then
//! each paired machine's own. A workspace belongs to its machine; machines are
//! discovered from the paired registry, never created from here.
use super::*;

impl Workspace {
    /// The machine a workspace belongs to, `None` for this one.
    pub(super) fn row_host(&self, row: usize) -> Option<&str> {
        let space = (row / ROWS_PER_SPACE).checked_sub(1)?;
        self.space_hosts.get(space).map(String::as_str)
    }

    /// First workspace of a machine's space.
    pub(super) fn space_first_row(&self, host: &str) -> Option<usize> {
        let space = self.space_hosts.iter().position(|known| known == host)?;
        Some((space + 1) * ROWS_PER_SPACE)
    }

    /// Where a new session starts: the machine of the active workspace. On this
    /// machine's workspaces it is the default destination, unless that is a
    /// paired machine: those have workspaces of their own.
    pub(super) fn new_session_destination(&self) -> Option<String> {
        match self.row_host(self.active_row) {
            Some(host) => Some(host.to_owned()),
            None => self
                .remotes
                .default_host
                .clone()
                .filter(|host| self.space_first_row(host).is_none()),
        }
    }

    /// The workspace a panel running on `host` belongs in: the same-numbered
    /// one of that machine's space. A paired machine's sessions never sit in
    /// another machine's workspaces, and a session on any other host (this
    /// machine, SSH, Cloud) never in a paired machine's.
    pub(super) fn bound_row(&self, host: Option<&str>, row: usize) -> usize {
        let first = host.and_then(|host| self.space_first_row(host)).unwrap_or(0);
        if row / ROWS_PER_SPACE * ROWS_PER_SPACE == first {
            row
        } else {
            first + row % ROWS_PER_SPACE
        }
    }

    /// Workspaces (rows) on screen: every machine's space contributes its own.
    pub(super) fn row_count(&self) -> usize {
        self.row_focus.len()
    }

    /// Give every paired machine its space, in the order they were paired. A
    /// machine unpaired since keeps its space while it holds panels, so
    /// nothing open disappears; an empty trailing one is dropped.
    pub(super) fn sync_spaces(&mut self) {
        self.add_paired_spaces();
        while let Some(last) = self.space_hosts.last() {
            let first = (self.space_hosts.len()) * ROWS_PER_SPACE;
            let paired = self.remotes.paired.iter().any(|m| &m.host() == last);
            let used = self.slots.iter().any(|slot| slot.row >= first);
            if paired || used {
                break;
            }
            self.space_hosts.pop();
        }
        self.resize_rows((1 + self.space_hosts.len()) * ROWS_PER_SPACE);
    }

    /// Append a space for every paired machine that has none, in the order
    /// they were paired; never removes one.
    pub(super) fn add_paired_spaces(&mut self) {
        for machine in &self.remotes.paired {
            let host = machine.host();
            if !self.space_hosts.contains(&host) {
                self.space_hosts.push(host);
            }
        }
        self.resize_rows((1 + self.space_hosts.len()) * ROWS_PER_SPACE);
    }

    /// Resize the per-workspace state; new workspaces start empty.
    pub(super) fn resize_rows(&mut self, rows: usize) {
        self.row_focus.resize(rows, None);
        self.camera_x.resize(rows, 0.0);
        self.camera_target.resize(rows, 0.0);
        self.camera_from.resize(rows, 0.0);
        self.camera_started.resize(rows, None);
        self.camera_touch_pan.resize(rows, false);
        self.camera_dirty.resize(rows, true);
        self.camera_snap_pending.resize(rows, false);
        self.active_row = self.active_row.min(rows - 1);
    }

    /// The short label on a workspace's tab group: "3" on this machine, "W2" for
    /// the second workspace of the machine whose name starts with W.
    pub(super) fn workspace_tab_label(&self, row: usize) -> String {
        match self.row_host(row) {
            None => (row + 1).to_string(),
            Some(host) => {
                let initial = machines::machine_label(host)
                    .chars()
                    .find(|c| c.is_alphanumeric())
                    .map_or('·', |c| c.to_ascii_uppercase());
                format!("{initial}{}", row % ROWS_PER_SPACE + 1)
            }
        }
    }

    /// "Workspace 3" on this machine; "work · Workspace 2" on a paired one,
    /// numbered within its own space.
    pub(super) fn workspace_title(&self, row: usize) -> String {
        match self.row_host(row) {
            None => format!("Workspace {}", row + 1),
            Some(host) => format!(
                "{} · Workspace {}",
                machines::machine_label(host),
                row % ROWS_PER_SPACE + 1
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_desktop_harness::paired;

    fn paired_machine(id: &str, label: &str) -> paired::Destination {
        paired::Destination {
            id: id.repeat(32 / id.len()),
            endpoint: "https://machine.example".into(),
            label: label.into(),
            working_dir: "/work".into(),
        }
    }

    #[gpui::test]
    fn each_paired_machine_gets_its_own_four_workspaces_after_this_machines(
        cx: &mut gpui::TestAppContext,
    ) {
        let workspace = cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx)).0;
        workspace.update(cx, |w, _| {
            assert_eq!(w.row_count(), ROWS_PER_SPACE);
            w.remotes.paired = vec![paired_machine("aa", "work"), paired_machine("bb", "lab")];
            w.sync_spaces();
            assert_eq!(w.row_count(), 3 * ROWS_PER_SPACE);
            assert_eq!(w.row_host(0), None);
            assert_eq!(w.row_host(ROWS_PER_SPACE - 1), None);
            assert_eq!(w.row_host(ROWS_PER_SPACE), Some(w.remotes.paired[0].host().as_str()));
            assert_eq!(w.row_host(3 * ROWS_PER_SPACE - 1), Some(w.remotes.paired[1].host().as_str()));
            assert_eq!(w.space_first_row(&w.remotes.paired[1].host()), Some(2 * ROWS_PER_SPACE));
            paired::publish_labels(&w.remotes.paired);
            assert_eq!(w.workspace_title(2), "Workspace 3");
            assert_eq!(w.workspace_tab_label(2), "3");
            assert_eq!(w.workspace_tab_label(ROWS_PER_SPACE + 1), "W2");
            assert_eq!(w.workspace_tab_label(2 * ROWS_PER_SPACE), "L1");
            assert_eq!(w.workspace_title(ROWS_PER_SPACE + 1), "work · Workspace 2");
            // Pairing again, or a second sync, changes nothing.
            w.sync_spaces();
            assert_eq!(w.row_count(), 3 * ROWS_PER_SPACE);
        });
    }

    #[gpui::test]
    fn an_unpaired_machine_keeps_its_workspaces_only_while_they_hold_panels(
        cx: &mut gpui::TestAppContext,
    ) {
        let workspace = cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx)).0;
        workspace.update(cx, |w, cx| {
            w.remotes.paired = vec![paired_machine("aa", "work")];
            w.sync_spaces();
            w.push_test_panel("panel", cx);
            w.slots[0].row = ROWS_PER_SPACE;
            w.remotes.paired.clear();
            w.sync_spaces();
            assert_eq!(w.row_count(), 2 * ROWS_PER_SPACE, "an open panel keeps its space");
            w.slots[0].row = 0;
            w.sync_spaces();
            assert_eq!(w.row_count(), ROWS_PER_SPACE);
        });
    }

    #[gpui::test]
    fn a_new_session_starts_on_the_machine_of_the_active_workspace(cx: &mut gpui::TestAppContext) {
        let workspace = cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx)).0;
        workspace.update(cx, |w, _| {
            w.remotes.paired = vec![paired_machine("aa", "work")];
            w.sync_spaces();
            assert_eq!(w.new_session_destination(), None);
            w.active_row = ROWS_PER_SPACE + 1;
            assert_eq!(w.new_session_destination(), Some(w.remotes.paired[0].host()));
            // This machine's workspaces still honour a default that has no
            // workspaces of its own, and never a paired one.
            w.active_row = 0;
            w.remotes.default_host = Some("desktop".into());
            assert_eq!(w.new_session_destination().as_deref(), Some("desktop"));
            w.remotes.default_host = Some(w.remotes.paired[0].host());
            assert_eq!(w.new_session_destination(), None);
        });
    }

    fn session_rows(w: &Workspace, cx: &App) -> Vec<(String, usize)> {
        let mut rows: Vec<_> = w
            .slots
            .iter()
            .map(|slot| (slot.panel.chat().unwrap().read(cx).session_id.clone(), slot.row))
            .collect();
        rows.sort();
        rows
    }

    #[gpui::test]
    fn a_version_1_snapshot_moves_paired_panels_into_their_machines_space(
        cx: &mut gpui::TestAppContext,
    ) {
        let machine = paired_machine("aa", "work");
        let remote = format!("paired://{}/session_remote_1", "aa".repeat(16));
        let ssh = "ssh://build/session_ssh_1";
        let (old, old_cx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w
        });
        let snapshot = old.update_in(old_cx, |w, window, cx| {
            w.push_test_panel("session_local_1", cx);
            w.active_row = 1;
            w.push_test_panel(&remote, cx);
            w.active_row = 2;
            w.push_test_panel(ssh, cx);
            w.snapshot_inner(window, cx, true).unwrap()
        });
        // What a version 1 file holds: the same slots, no machine spaces.
        let mut v1 = serde_json::to_value(&snapshot).unwrap();
        v1["format_version"] = 1.into();
        v1.as_object_mut().unwrap().remove("space_hosts");
        let v1 = WorkspaceSnapshot::decode(&serde_json::to_vec(&v1).unwrap()).unwrap();

        let (new, new_cx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w.remotes.paired = vec![machine.clone()];
            w.sync_spaces();
            w
        });
        new.update(new_cx, |w, cx| w.apply_snapshot(v1, cx));
        new.read_with(new_cx, |w, cx| {
            assert_eq!(
                session_rows(w, cx),
                [
                    // Its workspace 2 on that machine, not this machine's.
                    (remote.clone(), ROWS_PER_SPACE + 1),
                    ("session_local_1".to_owned(), 0),
                    // An SSH host is not a paired machine: it stays where it was.
                    (ssh.to_owned(), 2),
                ]
            );
            assert_eq!(w.space_hosts, [machine.host()]);
            assert_eq!(w.row_count(), 2 * ROWS_PER_SPACE);
        });

        // Saved again, it is a version 2 file that restores to the same rows.
        let v2 = new.update_in(new_cx, |w, window, cx| w.snapshot_inner(window, cx, true).unwrap());
        assert_eq!(v2.format_version, SNAPSHOT_FORMAT_VERSION);
        assert_eq!(v2.space_hosts, [machine.host()]);
        let (again, again_cx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w
        });
        again.update(again_cx, |w, cx| w.apply_snapshot(v2, cx));
        again.read_with(again_cx, |w, cx| {
            assert_eq!(session_rows(w, cx)[0], (remote.clone(), ROWS_PER_SPACE + 1));
            assert_eq!(w.row_count(), 2 * ROWS_PER_SPACE);
        });
    }

    #[gpui::test]
    fn a_session_is_opened_in_the_workspace_of_its_machine(cx: &mut gpui::TestAppContext) {
        let machine = paired_machine("aa", "work");
        let remote = format!("paired://{}/session_remote_1", "aa".repeat(16));
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w.remotes.paired = vec![machine.clone()];
            w.sync_spaces();
            w
        });
        workspace.update(vcx, |w, cx| {
            // Opened from this machine's workspace 3, it lands in the same
            // numbered workspace of the machine it runs on.
            w.active_row = 2;
            let remote_slot = w.open_session_with_identity(remote.clone(), None, None, 2, cx);
            assert_eq!(w.slots[remote_slot].row, ROWS_PER_SPACE + 2);
            // And a local session opened from that machine's space comes home.
            let local_slot =
                w.open_session_with_identity("session_local_1".into(), None, None, ROWS_PER_SPACE + 1, cx);
            assert_eq!(w.slots[local_slot].row, 1);
        });
    }

    #[gpui::test]
    fn a_panel_cannot_be_moved_into_another_machines_workspaces(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w.remotes.paired = vec![paired_machine("aa", "work")];
            w.sync_spaces();
            w.active_row = ROWS_PER_SPACE - 1;
            w.push_test_panel("session_local_1", cx);
            w
        });
        workspace.update_in(vcx, |w, window, cx| {
            w.move_panel_to_row(1, window, cx);
            assert_eq!((w.slots[0].row, w.active_row), (ROWS_PER_SPACE - 1, ROWS_PER_SPACE - 1));
            w.move_panel_to_row(-1, window, cx);
            assert_eq!(w.slots[0].row, ROWS_PER_SPACE - 2);
        });
    }

    /// Selector text is `'static`; a test leaks the few it builds.
    fn bounds(vcx: &mut gpui::VisualTestContext, selector: String) -> Option<gpui::Bounds<gpui::Pixels>> {
        vcx.debug_bounds(Box::leak(selector.into_boxed_str()))
    }

    fn sectioned_workspace<'a>(
        cx: &'a mut gpui::TestAppContext,
        labels: &[&str],
    ) -> (Entity<Workspace>, &'a mut gpui::VisualTestContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.bridge = harness::spawn_inert();
            w.remotes.paired = labels
                .iter()
                .enumerate()
                .map(|(n, label)| paired_machine(&format!("{n:x}a"), label))
                .collect();
            paired::publish_labels(&w.remotes.paired);
            w.sync_spaces();
            w.push_test_panel("local session", cx);
            w
        });
        vcx.run_until_parked();
        (workspace, vcx)
    }

    #[gpui::test]
    fn the_sidebar_has_a_labelled_section_per_machine_with_its_four_workspaces(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = sectioned_workspace(cx, &["work", "lab"]);
        for space in 0..3 {
            assert!(bounds(vcx, format!("space-section-{space}")).is_some(), "section {space}");
        }
        for row in 0..3 * ROWS_PER_SPACE {
            assert!(bounds(vcx, format!("space-workspace-{row}")).is_some(), "workspace {row}");
        }
        // The session open on this machine is listed under its section only.
        workspace.read_with(vcx, |w, _| assert_eq!(w.slots.len(), 1));
        assert!(vcx.debug_bounds("space-session-0").is_some());
        // Picking a workspace of a machine switches to it.
        let target = bounds(vcx, format!("space-workspace-{}", ROWS_PER_SPACE + 1)).unwrap();
        vcx.simulate_click(target.center(), gpui::Modifiers::default());
        workspace.read_with(vcx, |w, _| assert_eq!(w.active_row, ROWS_PER_SPACE + 1));
    }

    #[gpui::test]
    fn without_a_paired_machine_the_sidebar_is_unchanged(cx: &mut gpui::TestAppContext) {
        let (_, vcx) = sectioned_workspace(cx, &[]);
        assert!(vcx.debug_bounds("space-sections").is_none());
    }

    #[gpui::test]
    fn section_chrome_stays_on_one_line_however_long_the_label(cx: &mut gpui::TestAppContext) {
        let long = "a very long machine label that cannot possibly fit a narrow sidebar";
        let (workspace, vcx) = sectioned_workspace(cx, &[long]);
        let host = workspace.read_with(vcx, |w, _| w.remotes.paired[0].host());
        workspace.update(vcx, |w, cx| {
            w.remotes.machine_status.insert(host, machines::MachineStatus::NeedsUpdate);
            cx.notify();
        });
        vcx.run_until_parked();
        let name = bounds(vcx, "space-name-1".into()).unwrap();
        let status = bounds(vcx, "space-status-1".into()).unwrap();
        let section = vcx.debug_bounds("space-sections").unwrap();
        assert!(name.size.height < gpui::px(20.0), "name wrapped: {:?}", name.size);
        assert!(status.size.height < gpui::px(20.0), "status wrapped: {:?}", status.size);
        assert!(name.right() <= section.right() && status.right() <= section.right());
    }

    #[gpui::test]
    fn link_results_set_the_machine_status_shown_in_its_header(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = sectioned_workspace(cx, &["work"]);
        let host = workspace.read_with(vcx, |w, _| w.remotes.paired[0].host());
        let status = |vcx: &mut gpui::VisualTestContext| {
            workspace.read_with(vcx, |w, _| w.remotes.machine_status.get(&host).copied())
        };
        let report = |vcx: &mut gpui::VisualTestContext, message: &str, failure: Option<jcode_sdk::PairedFailure>| {
            workspace.update(vcx, |w, cx| {
                w.apply(
                    Update::RemoteStatus {
                        host: host.clone(),
                        message: message.into(),
                        request_id: None,
                        failed: failure.is_some(),
                        failure: failure.map(|kind| {
                            crate::connect_failure::ConnectFailure::from_error(&jcode_sdk::Error::new(
                                jcode_sdk::ErrorKind::Paired(kind),
                                "fixture",
                            ))
                        }),
                    },
                    cx,
                );
            });
        };
        assert_eq!(status(vcx), None);
        report(vcx, "Connected to paired machine", None);
        assert_eq!(status(vcx), Some(machines::MachineStatus::Connected));
        report(vcx, "Remote session failed: timeout", Some(jcode_sdk::PairedFailure::TimedOut));
        assert_eq!(status(vcx), Some(machines::MachineStatus::Lost));
        report(vcx, "Remote session failed: too old", Some(jcode_sdk::PairedFailure::TooOld));
        assert_eq!(status(vcx), Some(machines::MachineStatus::NeedsUpdate));
        assert_eq!(machines::MachineStatus::NeedsUpdate.label("work"), "Update Jcode on work");
    }

    #[gpui::test]
    fn keyboard_workspace_switching_crosses_from_one_machine_to_the_next(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = sectioned_workspace(cx, &["work"]);
        workspace.update_in(vcx, |w, window, cx| {
            w.active_row = ROWS_PER_SPACE - 1;
            w.focus_down(&FocusDown, window, cx);
            assert_eq!(w.active_row, ROWS_PER_SPACE, "down from this machine's last workspace");
            w.focus_up(&FocusUp, window, cx);
            assert_eq!(w.active_row, ROWS_PER_SPACE - 1);
        });
    }
}
