//! Each machine's Desktop state lives in that machine's daemon: the open
//! panels, their layout, where focus was and unsent drafts. This Desktop reads
//! it when it attaches to a machine, writes its own changes back, and shows
//! what another Desktop attached to the same machine changes. The machine in
//! question may be this one: its own daemon keeps this Desktop's state too.
//!
//! The crash-recovery snapshot stays the offline cache: it restores the window
//! before a machine answers, and a machine that cannot sync (lost, or too old
//! to know about Desktop state) leaves everything working as before.
use super::*;
use jcode_desktop_harness::harness::{LinkStatus, MachineStateEvent};
use jcode_desktop_model::machine_state::{self as shared, Entries, Facts, Fingerprints, PanelFacts};
use super::workspace_machine_sync_store as store;
use jcode_session_types::desktop_state::PanelKind;
use jcode_sdk::api::DesktopStateChange;
use jcode_sdk::desktop_state::{DesktopStateMirror, DesktopStatePut};
use std::collections::{BTreeMap, BTreeSet};

/// How often a quiet connected machine is compared with what this Desktop shows.
pub(super) const SYNC_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Default)]
pub(super) struct MachineSyncState {
    machines: HashMap<Option<String>, MachineSync>,
    requested: HashSet<Option<String>>,
    next_token: u64,
    /// A composer shown from another Desktop's state keeps the name it came with.
    adopted_ids: HashMap<gpui::EntityId, String>,
    /// Draft keys of the composer the user is typing in. Never overwritten.
    pub(super) typing: BTreeSet<String>,
    /// What each machine last agreed on, kept across restarts; `None` where
    /// nothing is kept (offline fixtures, tests).
    store: Option<store::Store>,
}

impl MachineSyncState {
    /// `persist`: keep what was agreed beside the crash-recovery snapshot.
    pub(super) fn new(persist: bool) -> Self {
        let path = persist
            .then(crate::learning::state_path)
            .flatten()
            .and_then(|state| Some(state.parent()?.join("machine-sync.json")));
        Self {
            store: path.map(store::Store::open),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn keep_at(&mut self, path: std::path::PathBuf) {
        self.store = Some(store::Store::open(path));
    }

    /// A composer shown from another Desktop's state, or restored from the
    /// snapshot, keeps the name its panel had in the machine's state.
    pub(in crate::workspace) fn keep_shared_id(&mut self, panel: gpui::EntityId, id: String) {
        self.adopted_ids.insert(panel, id);
    }

    pub(in crate::workspace) fn shared_id(&self, panel: gpui::EntityId) -> Option<&String> {
        self.adopted_ids.get(&panel)
    }
}

struct MachineSync {
    /// What the daemon holds.
    mirror: DesktopStateMirror,
    /// What this Desktop and the daemon last agreed on, key by key.
    applied: Fingerprints,
    /// Not rejected by the daemon: a daemon older than the compose directory
    /// field refuses it, and then it is left out for that machine.
    dirs: bool,
    /// This Desktop has not yet seen the daemon's state.
    first: bool,
    connected: bool,
    in_flight: Option<InFlight>,
}

impl MachineSync {
    fn new(agreed: Fingerprints) -> Self {
        Self {
            mirror: DesktopStateMirror::default(),
            applied: agreed,
            dirs: true,
            first: true,
            connected: false,
            in_flight: None,
        }
    }

    fn remote(&self) -> Entries {
        let held = self
            .mirror
            .keys()
            .filter_map(|key| Some((key.to_owned(), self.mirror.get(key)?.clone())))
            .collect();
        shared::canonical(&held)
    }
}

struct InFlight {
    token: u64,
    keys: Vec<(String, Option<serde_json::Value>)>,
}

impl machines::MachineStatus {
    fn of(link: LinkStatus) -> Self {
        match link {
            LinkStatus::Connecting => Self::Connecting,
            LinkStatus::Connected => Self::Connected,
            LinkStatus::Lost => Self::Lost,
            LinkStatus::NeedsUpdate => Self::NeedsUpdate,
        }
    }
}

impl Workspace {
    /// The space of a machine: 0 is this one.
    fn machine_space(&self, host: Option<&str>) -> Option<usize> {
        match host {
            None => Some(0),
            Some(host) => self.space_hosts.iter().position(|known| known == host).map(|n| n + 1),
        }
    }

    /// The panel's name in the machine's shared state, if it takes part.
    fn shared_panel_id(
        &self,
        slot: &Slot,
        host: Option<&str>,
        cx: &App,
    ) -> Option<(String, crate::panel::SharedIdentity)> {
        use crate::panel::SharedIdentity;
        let identity = slot.panel.read_chat(cx)?.shared_identity()?;
        let id = match &identity {
            SharedIdentity::Session { host: on, session } => {
                (on.as_deref() == host).then_some(())?;
                session.clone()
            }
            SharedIdentity::Compose(draft) => {
                if host.is_some() {
                    return None;
                }
                self.machine_sync
                    .adopted_ids
                    .get(&slot.panel.entity_id())
                    .cloned()
                    .unwrap_or_else(|| {
                        let tail: String = draft
                            .chars()
                            .rev()
                            .take(60)
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        format!("c-{tail}")
                    })
            }
        };
        shared::valid_id(&id, 64).then_some((id, identity))
    }

    /// Panels of a machine's space by shared name, with their slot index.
    fn shared_panels(&self, host: Option<&str>, cx: &App) -> BTreeMap<String, usize> {
        let Some(space) = self.machine_space(host) else {
            return BTreeMap::new();
        };
        let rows = space * ROWS_PER_SPACE..(space + 1) * ROWS_PER_SPACE;
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| !slot.closing && rows.contains(&slot.row))
            .filter_map(|(index, slot)| Some((self.shared_panel_id(slot, host, cx)?.0, index)))
            .collect()
    }

    fn machine_facts(&self, host: Option<&str>, cx: &App) -> Facts {
        let Some(space) = self.machine_space(host) else {
            return Facts::default();
        };
        let base = space * ROWS_PER_SPACE;
        let mut facts = Facts::default();
        for row in base..base + ROWS_PER_SPACE {
            for index in self.row_indices(row) {
                let slot = &self.slots[index];
                if slot.closing {
                    continue;
                }
                let Some((id, identity)) = self.shared_panel_id(slot, host, cx) else {
                    continue;
                };
                let Some(panel) = slot.panel.read_chat(cx) else {
                    continue;
                };
                let (text, cursor, attachments) = panel.shared_draft(cx);
                let compose = matches!(identity, crate::panel::SharedIdentity::Compose(_));
                facts.panels.push(PanelFacts {
                    id,
                    kind: match identity {
                        crate::panel::SharedIdentity::Session { .. } => PanelKind::Session,
                        crate::panel::SharedIdentity::Compose(_) => PanelKind::Compose,
                    },
                    session_id: match identity {
                        crate::panel::SharedIdentity::Session { session, .. } => Some(session),
                        crate::panel::SharedIdentity::Compose(_) => None,
                    },
                    title: Some(panel.title_text()),
                    working_dir: compose.then(|| panel.working_dir.clone()).flatten(),
                    row: row - base,
                    width: slot.width_fraction,
                    draft: Some(shared::DraftFacts {
                        text,
                        cursor,
                        attachments: attachments
                            .into_iter()
                            .map(|a| jcode_session_types::desktop_state::DraftAttachment {
                                media_type: a.media_type,
                                label: Some(a.label).filter(|label| !label.is_empty()),
                                data: a.encoded,
                            })
                            .collect(),
                    }),
                });
            }
        }
        if (base..base + ROWS_PER_SPACE).contains(&self.active_row) {
            let panel = self
                .slots
                .get(self.active)
                .filter(|slot| slot.row == self.active_row && !slot.closing)
                .and_then(|slot| Some(self.shared_panel_id(slot, host, cx)?.0));
            facts.focus = Some((self.active_row - base, panel));
        }
        facts
    }

    pub(super) fn machine_entries(&self, host: Option<&str>, held_focus: Option<&serde_json::Value>, cx: &App) -> Entries {
        let mut facts = self.machine_facts(host, cx);
        let dirs = self.machine_sync.machines.get(&host.map(str::to_owned)).is_none_or(|m| m.dirs);
        if !dirs {
            for panel in &mut facts.panels {
                panel.working_dir = None;
            }
        }
        shared::canonical(&shared::entries(&facts, held_focus))
    }

    /// What was agreed with a machine before this run, if anything is kept.
    fn agreed(&self, host: Option<&str>) -> Fingerprints {
        self.machine_sync.store.as_ref().map(|store| store.agreed(host)).unwrap_or_default()
    }

    /// Ask for the link to each machine the window needs: this one always, a
    /// paired one once the user is in its space or panels of it are open.
    fn attach_needed_machines(&mut self) {
        for space in 0..=self.space_hosts.len() {
            let host = space.checked_sub(1).map(|n| self.space_hosts[n].clone());
            let rows = space * ROWS_PER_SPACE..(space + 1) * ROWS_PER_SPACE;
            let needed = space == 0
                || rows.contains(&self.active_row)
                || self.slots.iter().any(|slot| rows.contains(&slot.row) && !slot.closing);
            if needed && self.machine_sync.requested.insert(host.clone()) {
                self.bridge.send(Command::AttachMachine { host });
            }
        }
    }

    /// Called every [`SYNC_INTERVAL`]: attach what is needed, note which
    /// composer the user is in, and bring each connected machine up to date.
    pub(super) fn machine_sync_tick(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.single_panel {
            return;
        }
        self.attach_needed_machines();
        // A new composer's name is fixed the first time it is seen, so it
        // outlives the panel's own draft id, which a restart replaces.
        let unnamed: Vec<_> = self
            .slots
            .iter()
            .filter(|slot| !slot.closing && self.machine_sync.shared_id(slot.panel.entity_id()).is_none())
            .filter(|slot| {
                slot.panel
                    .read_chat(cx)
                    .and_then(|panel| panel.shared_identity())
                    .is_some_and(|identity| matches!(identity, crate::panel::SharedIdentity::Compose(_)))
            })
            .filter_map(|slot| Some((slot.panel.entity_id(), self.shared_panel_id(slot, None, cx)?.0)))
            .collect();
        for (panel, id) in unnamed {
            self.machine_sync.keep_shared_id(panel, id);
        }
        let mut typing = BTreeSet::new();
        for slot in &self.slots {
            if slot.closing || !slot.panel.surface().input_focus_handle(cx).is_focused(window) {
                continue;
            }
            for host in std::iter::once(None).chain(self.space_hosts.iter().map(|h| Some(h.as_str()))) {
                if let Some((id, _)) = self.shared_panel_id(slot, host, cx) {
                    typing.insert(format!("draft/{id}"));
                }
            }
        }
        self.machine_sync.typing = typing;
        let connected: Vec<Option<String>> = self
            .machine_sync
            .machines
            .iter()
            .filter(|(_, machine)| machine.connected)
            .map(|(host, _)| host.clone())
            .collect();
        for host in connected {
            self.sync_machine(host.as_deref(), cx);
        }
        if let Some(store) = &mut self.machine_sync.store {
            store.flush();
        }
    }

    /// The name a composer panel carries in this machine's state, to keep in
    /// the crash-recovery snapshot so a restart finds its entry again.
    pub(in crate::workspace) fn compose_shared_id(&self, slot: &Slot, cx: &App) -> Option<String> {
        matches!(
            slot.panel.read_chat(cx)?.shared_identity()?,
            crate::panel::SharedIdentity::Compose(_)
        )
        .then(|| self.machine_sync.shared_id(slot.panel.entity_id()).cloned())
        .flatten()
    }

    pub(super) fn apply_machine_link(&mut self, host: Option<String>, status: LinkStatus, cx: &mut Context<Self>) {
        let shown = machines::MachineStatus::of(status);
        match &host {
            Some(host) => {
                self.remotes.machine_status.insert(host.clone(), shown);
            }
            None => self.remotes.local_link = Some(shown),
        }
        let agreed = self.agreed(host.as_deref());
        let machine = self.machine_sync.machines.entry(host).or_insert_with(|| MachineSync::new(agreed));
        if status != LinkStatus::Connected {
            machine.connected = false;
            machine.in_flight = None;
        }
        cx.notify();
    }

    pub(super) fn apply_machine_state(&mut self, host: Option<String>, event: MachineStateEvent, cx: &mut Context<Self>) {
        let agreed = self.agreed(host.as_deref());
        let machine = self.machine_sync.machines.entry(host.clone()).or_insert_with(|| MachineSync::new(agreed));
        match event {
            MachineStateEvent::Snapshot(snapshot) => {
                machine.mirror.replace(&snapshot);
                machine.connected = true;
                machine.in_flight = None;
            }
            MachineStateEvent::Changed { rev, entries, .. } => {
                machine.mirror.merge(rev, &entries);
            }
        }
        self.sync_machine(host.as_deref(), cx);
    }

    pub(super) fn apply_machine_put(
        &mut self,
        host: Option<String>,
        token: u64,
        result: Result<DesktopStatePut, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(machine) = self.machine_sync.machines.get_mut(&host) else {
            return;
        };
        if machine.in_flight.as_ref().is_none_or(|flight| flight.token != token) {
            return;
        }
        let flight = machine.in_flight.take().expect("checked above");
        match &result {
            Ok(DesktopStatePut::Applied { .. }) => {
                for (key, value) in flight.keys {
                    record(&mut machine.applied, key, value.as_ref());
                }
            }
            Ok(DesktopStatePut::Conflict { .. }) | Err(_) => {}
            Ok(DesktopStatePut::Rejected { reason, message }) => {
                eprintln!("desktop state rejected ({reason:?}): {message}");
                let with_dir = flight.keys.iter().any(|(key, value)| {
                    key.starts_with("panel/") && value.as_ref().is_some_and(|v| v.get("working_dir").is_some())
                });
                if with_dir && machine.dirs {
                    // A daemon from before compose directories refuses the
                    // field: leave it out for this machine and write again.
                    machine.dirs = false;
                } else {
                    // Nothing was stored and the same write cannot succeed:
                    // stop asking until the entry changes again.
                    for (key, value) in flight.keys {
                        record(&mut machine.applied, key, value.as_ref());
                    }
                }
            }
        }
        if let Ok(outcome) = &result {
            machine.mirror.apply_put(outcome);
        }
        self.sync_machine(host.as_deref(), cx);
    }

    /// Compare this Desktop with the machine's state: show what changed there,
    /// write what changed here.
    fn sync_machine(&mut self, host: Option<&str>, cx: &mut Context<Self>) {
        if self.single_panel {
            return;
        }
        let key = host.map(str::to_owned);
        let Some(machine) = self.machine_sync.machines.get(&key).filter(|m| m.connected) else {
            return;
        };
        let remote = machine.remote();
        let first = machine.first;
        let typing = self.machine_sync.typing.clone();
        let local = self.machine_entries(host, remote.get(shared::FOCUS), cx);
        let machine = self.machine_sync.machines.get_mut(&key).expect("present above");
        // Where both already agree there is nothing to decide later.
        for k in remote.keys().chain(local.keys()).cloned().collect::<BTreeSet<_>>() {
            if remote.get(&k) == local.get(&k) {
                record(&mut machine.applied, k.clone(), remote.get(&k));
            }
        }
        let plan = shared::reconcile(&machine.applied, &remote, &local, &typing, first);
        machine.first = false;
        let may_push = machine.in_flight.is_none();
        let token = {
            self.machine_sync.next_token += 1;
            self.machine_sync.next_token
        };
        if !plan.adopt.is_empty() {
            self.adopt_machine_state(host, &remote, &plan.adopt, first, cx);
            let now = self.machine_entries(host, remote.get(shared::FOCUS), cx);
            let machine = self.machine_sync.machines.get_mut(&key).expect("present above");
            for k in &plan.adopt {
                record(&mut machine.applied, k.clone(), now.get(k));
            }
        }
        for key in &plan.stash {
            let Some(id) = key.strip_prefix("draft/") else { continue };
            let Some(text) = remote.get(key).and_then(|doc| doc.get("text")).and_then(|t| t.as_str()) else {
                continue;
            };
            if let Some(&index) = self.shared_panels(host, cx).get(id) {
                self.slots[index].panel.update_chat(cx, |panel, cx| panel.keep_in_history(text, cx));
            }
        }
        if let Some(store) = &mut self.machine_sync.store
            && let Some(machine) = self.machine_sync.machines.get(&key)
        {
            store.record(host, &machine.applied);
        }
        if may_push && !plan.push.is_empty() {
            let machine = self.machine_sync.machines.get_mut(&key).expect("present above");
            let expected_rev = machine.mirror.rev();
            machine.in_flight = Some(InFlight {
                token,
                keys: plan.push.clone(),
            });
            self.bridge.send(Command::PutMachineState {
                host: key,
                token,
                expected_rev,
                changes: plan
                    .push
                    .into_iter()
                    .map(|(key, value)| DesktopStateChange { key, value })
                    .collect(),
            });
        }
    }

    /// Show what the machine's daemon holds for `adopt`: open and close
    /// panels, arrange them, set drafts, and (on first sight) restore focus.
    fn adopt_machine_state(
        &mut self,
        host: Option<&str>,
        remote: &Entries,
        adopt: &BTreeSet<String>,
        first: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(space) = self.machine_space(host) else {
            return;
        };
        let base = space * ROWS_PER_SPACE;
        let state = shared::parse(remote);
        let row_of = |id: &str| {
            state
                .rows
                .iter()
                .position(|row| row.iter().any(|(panel, _)| panel == id))
                .unwrap_or(0)
        };
        let present = self.shared_panels(host, cx);
        for id in adopt.iter().filter_map(|key| key.strip_prefix("panel/")) {
            match (state.panels.get(id), present.get(id)) {
                (Some(panel), None) => self.open_shared_panel(host, id, panel, base + row_of(id), cx),
                (None, Some(&index)) => self.close_shared_panel(index, cx),
                _ => {}
            }
        }
        if adopt.contains(shared::LAYOUT) {
            self.arrange_shared_panels(host, &state, base, cx);
        }
        let present = self.shared_panels(host, cx);
        for key in adopt.iter().filter(|key| key.starts_with("draft/")) {
            let id = &key["draft/".len()..];
            let Some(&index) = present.get(id) else {
                continue;
            };
            let (text, cursor, attachments) = match state.drafts.get(id) {
                Some(draft) => (
                    draft.text.clone(),
                    draft.cursor as usize,
                    draft
                        .attachments
                        .iter()
                        .map(|a| crate::input::AttachmentSnapshot {
                            media_type: a.media_type.clone(),
                            encoded: a.data.clone(),
                            label: a.label.clone().unwrap_or_default(),
                        })
                        .collect(),
                ),
                None => (String::new(), 0, Vec::new()),
            };
            self.slots[index]
                .panel
                .update_chat(cx, |panel, cx| panel.apply_shared_draft(text, cursor, attachments, cx));
        }
        if first && adopt.contains(shared::FOCUS) {
            self.restore_shared_focus(host, &state, base, cx);
        }
        cx.notify();
    }

    fn open_shared_panel(
        &mut self,
        host: Option<&str>,
        id: &str,
        panel: &jcode_session_types::desktop_state::PanelDoc,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        use jcode_session_types::desktop_state::PanelKind;
        match (panel.kind, host, panel.session_id.as_deref()) {
            (PanelKind::Session, host, Some(session)) => {
                let session_id = match host {
                    Some(host) => crate::harness::remote_session_id(host, session),
                    None => session.to_owned(),
                };
                self.open_session_with_identity(session_id, panel.title.clone(), None, row, cx);
            }
            (PanelKind::Compose, None, _) => {
                let index = self.open_session_with_identity(
                    pending::next_draft_id(),
                    Some(panel.title.clone().unwrap_or_else(|| "New session".into())),
                    panel.working_dir.clone(),
                    row,
                    cx,
                );
                self.machine_sync
                    .keep_shared_id(self.slots[index].panel.entity_id(), id.to_owned());
                self.slots[index]
                    .panel
                    .update_chat(cx, |panel, cx| panel.defer_until_first_prompt(cx));
            }
            _ => {}
        }
    }

    /// Close a panel another Desktop closed, the way the user closing it would.
    fn close_shared_panel(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(session_id) = self.slots[index].panel.read_chat(cx).map(|p| p.session_id.clone()) else {
            return;
        };
        self.sidebar_closed_sessions.insert(session_id.clone());
        if !self.gone_sessions.contains(&session_id) {
            self.gone_sessions.push(session_id);
        }
    }

    /// Put panels in the rows, widths and order the machine's layout names.
    fn arrange_shared_panels(&mut self, host: Option<&str>, state: &shared::Remote, base: usize, cx: &mut Context<Self>) {
        let index = self.shared_panels(host, cx);
        let now = Instant::now();
        for (rel, row) in state.rows.iter().enumerate() {
            for (id, width) in row {
                let Some(&i) = index.get(id) else { continue };
                let slot = &mut self.slots[i];
                slot.row = base + rel;
                if (slot.width_fraction - width).abs() > f32::EPSILON {
                    slot.width_fraction = *width;
                    slot.animated_width.set(*width, now);
                    slot.restore_fraction = None;
                }
            }
        }
        let mut order: Vec<usize> = (0..self.slots.len()).collect();
        for rel in 0..shared::MAX_ROWS {
            let positions: Vec<usize> = self.row_indices(base + rel).collect();
            let mut want: Vec<usize> = state
                .rows
                .get(rel)
                .into_iter()
                .flatten()
                .filter_map(|(id, _)| index.get(id).copied())
                .filter(|i| positions.contains(i))
                .collect();
            for &position in &positions {
                if !want.contains(&position) {
                    want.push(position);
                }
            }
            for (&position, &from) in positions.iter().zip(&want) {
                order[position] = from;
            }
        }
        if order.iter().enumerate().any(|(to, from)| to != *from) {
            let active = self.slots.get(self.active).map(|slot| slot.panel.entity_id());
            let mut old: Vec<Option<Slot>> = std::mem::take(&mut self.slots).into_iter().map(Some).collect();
            self.slots = order.iter().map(|&from| old[from].take().expect("each slot once")).collect();
            if let Some(active) = active {
                self.active = self
                    .slots
                    .iter()
                    .position(|slot| slot.panel.entity_id() == active)
                    .unwrap_or(0);
            }
        }
        self.retarget_camera();
    }

    /// Return to the panel the machine's state says the user last had, when the
    /// user is in that machine's space.
    fn restore_shared_focus(&mut self, host: Option<&str>, state: &shared::Remote, base: usize, cx: &mut Context<Self>) {
        let Some((row, panel)) = &state.focus else {
            return;
        };
        if !(base..base + ROWS_PER_SPACE).contains(&self.active_row) {
            return;
        }
        let index = panel
            .as_ref()
            .and_then(|id| self.shared_panels(host, cx).get(id).copied());
        match index {
            Some(index) => self.set_active(index, cx),
            None => self.select_row(base + row, 0),
        }
        self.focus_pending = true;
    }

    #[cfg(test)]
    pub(super) fn machine_state_applied(&self, host: Option<&str>) -> Option<Fingerprints> {
        self.machine_sync.machines.get(&host.map(str::to_owned)).map(|m| m.applied.clone())
    }
}

/// Note what `key` now holds on both sides.
fn record(applied: &mut Fingerprints, key: String, value: Option<&serde_json::Value>) {
    match value {
        Some(value) => applied.insert(key, shared::fingerprint(value)),
        None => applied.remove(&key),
    };
}
