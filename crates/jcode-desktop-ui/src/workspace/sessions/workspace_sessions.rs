//! Opening, wiring and raising session panels.
use super::*;

impl Workspace {
    pub(super) fn wire_session_panel(panel: &Entity<Panel>, cx: &mut Context<Self>) {
        Panel::connect_input(panel, cx);
        if !cfg!(test) {
            panel.update(cx, |panel, cx| panel.refresh_local_pending_awaits(cx));
        }
        cx.subscribe(
            panel,
            |this, _, event: &crate::panel::OpenSwarmWorker, cx| {
                this.open_swarm_worker(&event.0, cx);
            },
        )
        .detach();
    }

    /// Open `session` as a panel immediately to the right of the focused panel.
    /// Returns the index of the new slot.
    pub(super) fn open_session(&mut self, session: jcode_sdk::SessionInfo, cx: &mut Context<Self>) -> usize {
        self.open_session_in_row(session, self.active_row, cx)
    }

    /// Open `session` as a panel in `row`. A row other than the active one
    /// receives the panel at its end and stays hidden: focus, the active row
    /// and the camera are untouched.
    pub(super) fn open_session_in_row(
        &mut self,
        session: jcode_sdk::SessionInfo,
        row: usize,
        cx: &mut Context<Self>,
    ) -> usize {
        self.open_session_with_identity(
            session.session_id,
            session.title,
            session.working_dir,
            row,
            cx,
        )
    }

    pub(super) fn open_session_with_identity(
        &mut self,
        session_id: String,
        title: Option<String>,
        working_dir: Option<String>,
        row: usize,
        cx: &mut Context<Self>,
    ) -> usize {
        let row = self.bound_row(crate::harness::remote_host(&session_id).as_deref(), row);
        debug_assert!(row < self.row_count());
        let width_fraction = spawned_panel_width(
            self.row_indices(row)
                .filter(|&index| !self.slots[index].closing)
                .count(),
        );
        let bridge = self.bridge.clone();
        let is_startup = session_id == Panel::STARTUP_SESSION_ID;
        let panel = cx.new(|cx| Panel::new(session_id.clone(), title, working_dir, bridge, cx));
        Self::wire_session_panel(&panel, cx);
        if !Panel::is_pending_session_id(&session_id) {
            panel.update(cx, |panel, cx| panel.prefetch_persisted_history(cx));
            self.bridge.send(Command::Watch { session_id });
        }
        let slot = Slot {
            panel: PanelItem::Chat(panel),
            row,
            width_fraction,
            animated_width: AnimatedValue::new(
                0.0,
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
        };
        let active_is_on_strip = self
            .slots
            .get(self.active)
            .is_some_and(|slot| slot.row == row);
        let row_last = self.row_indices(row).last();
        let insert_at = insert_index(self.active, active_is_on_strip, row_last, self.slots.len());
        self.slots.insert(insert_at, slot);
        self.slots[insert_at]
            .animated_width
            .set(width_fraction, Instant::now());
        // The lone panel that had the whole viewport steps back to the default
        // width now that it has company, leaving two equal halves.
        let live_indices = self
            .row_indices(row)
            .filter(|&index| !self.slots[index].closing)
            .collect::<Vec<_>>();
        let strip_panels = live_indices.len();
        let now = Instant::now();
        for index in live_indices {
            if index == insert_at {
                continue;
            }
            let slot = &mut self.slots[index];
            let demoted = demoted_width(slot.width_fraction, strip_panels);
            if demoted != slot.width_fraction {
                slot.width_fraction = demoted;
                slot.animated_width.set(demoted, now);
                slot.restore_fraction = None;
            }
        }
        // Inserting shifts every later index, including the focused one.
        if self.active >= insert_at {
            self.active += 1;
        }
        if !is_startup && row == self.active_row {
            crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        }
        insert_at
    }

    /// Push each coordinator panel its swarm workers for the composer
    /// indicator. Non-coordinators get none.
    pub(super) fn sync_swarm_strips(&mut self, cx: &mut Context<Self>) {
        let now = swarm_now_ms();
        let workers =
            crate::swarm_view::workers_by_coordinator(&self.sessions, &self.worker_meta, now);
        let mut changed = false;
        for slot in self.slots.iter().filter(|slot| !slot.closing) {
            let Some(panel) = slot.panel.read_chat(cx) else {
                continue;
            };
            let list = workers
                .get(&panel.session_id)
                .map_or(&[][..], Vec::as_slice);
            let record = self.worker_meta.get(&panel.session_id);
            if !panel.swarm_strip_matches(list) || !panel.worker_record_matches(record) {
                changed = true;
                slot.panel
                    .update_chat(cx, |panel, cx| panel.sync_worker_strip(list, record, cx));
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn seed_tab_status_fixture(&self, cx: &mut Context<Self>) {
        use jcode_sdk::ApiEvent;
        for (index, slot) in self.slots.iter().take(4).enumerate() {
            slot.panel.update_chat(cx, |panel, cx| {
                let session_id = panel.session_id.clone();
                let status = |status: &str| ApiEvent::SessionStatus {
                    session_id: session_id.clone(),
                    status: status.into(),
                    pending_soft_interrupts: None,
                };
                match index {
                    0 => panel.apply(&status("running"), cx),
                    1 => panel.set_tab_waiting(true, cx),
                    2 => {
                        let event = serde_json::from_value(serde_json::json!({
                            "ev":"ask_request", "session_id":session_id,
                            "request_id":"fixture-ask", "tool_call_id":"fixture-tool",
                            "questions":[{"header":"Choice", "question":"Continue?", "options":[
                                {"label":"Yes", "description":"Continue the work"},
                                {"label":"No", "description":"Stop here"}
                            ]}]
                        }))
                        .expect("valid ask fixture");
                        panel.apply(&event, cx);
                    }
                    3 => {
                        panel.set_surface_focused(false, cx);
                        panel.apply(&status("running"), cx);
                        panel.apply(&status("idle"), cx);
                    }
                    _ => unreachable!(),
                }
            });
        }
    }

    /// Select an open session's tab and activate the app, so an agent that
    /// messages the user about another session can raise it. No-op when the
    /// session has no open panel here.
    pub(super) fn raise_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.session_id(cx) == session_id)
        else {
            return false;
        };
        self.set_active(index, cx);
        self.overview = false;
        cx.activate(true);
        true
    }

    /// `JCODE_DESKTOP_SCREENSHOT_SWARM=real`: the user's real coordinator and
    /// its workers, from trimmed copies of their `~/.jcode` files (see
    /// `jcode-desktop-harness/src/fixtures`), so a screenshot shows real data
    /// shapes, not invented ones.
    pub(super) fn load_real_swarm_fixture(&mut self, cx: &mut Context<Self>) {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../jcode-desktop-harness/src/fixtures");
        let home = fixtures.join("microbe_home");
        let mut sessions = harness::merge_persisted_sessions(Vec::new(), Some(&home));
        jcode_sdk::enrich_sessions_from_swarm_state(&mut sessions, home.join("state/swarm"));
        // The rows read each worker's record; the fixture homes already hold
        // trimmed copies of them, so read those rather than the real home. The
        // usage fixture keeps full token records for the workers it covers.
        let ids = jcode_sdk::worker_ids(&sessions);
        let mut meta = jcode_sdk::worker_meta_for(&home, &ids);
        meta.extend(jcode_sdk::worker_meta_for(
            &fixtures.join("usage_home"),
            &ids,
        ));
        self.worker_meta = meta;
        let Some(position) = sessions
            .iter()
            .position(|s| s.session_id.starts_with("session_microbe_"))
        else {
            return;
        };
        let coordinator = sessions.remove(position);
        sessions.insert(0, coordinator.clone());
        self.sessions = sessions;
        self.slots.clear();
        self.active = self.open_session(coordinator, cx);
        let ids = self.slots[self.active]
            .panel
            .read_chat(cx)
            .map(Panel::swarm_card_worker_ids)
            .unwrap_or_default();
        let usage =
            jcode_desktop_harness::swarm_usage::usage_for(&fixtures.join("usage_home"), &ids);
        self.slots[self.active]
            .panel
            .update_chat(cx, |panel, cx| panel.set_swarm_usage(usage, cx));
        self.sync_swarm_strips(cx);
    }

    /// Focus a worker's panel, opening it if needed (strip, sidebar, or DM).
    pub(super) fn open_swarm_worker(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sidebar_closed_sessions.remove(session_id);
        let index = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.session_id(cx) == session_id)
            .unwrap_or_else(|| {
                let session = self
                    .sessions
                    .iter()
                    .find(|s| s.session_id == session_id)
                    .cloned();
                if let Some(session) = session {
                    self.open_session(session, cx)
                } else {
                    // A signed DM may arrive ahead of the next catalog refresh.
                    self.open_session_with_identity(
                        session_id.to_owned(),
                        None,
                        None,
                        self.active_row,
                        cx,
                    )
                }
            });
        self.set_active(index, cx);
        self.sync_swarm_strips(cx);
    }

    pub(super) fn activate_session(
        &mut self,
        session: jcode_sdk::SessionInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == session.session_id)
            .unwrap_or_else(|| self.open_session(session, cx));
        // A sidebar card can be reopened before its close animation finishes.
        // Revive the original entity, including its composer draft, rather than
        // focusing a disappearing view or creating duplicate event recipients.
        if self.slots[index].closing {
            self.slots[index].closing = false;
            self.slots[index].close_progress.set(1.0, Instant::now());
            let session_id = self.slots[index].panel.session_id(cx).to_owned();
            if !Panel::is_pending_session_id(&session_id) {
                self.bridge.send(Command::Watch { session_id });
            }
        }
        self.set_active(index, cx);
        self.overview = false;
        self.overview_progress.set(0.0, Instant::now());
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn activate_unfinished_session(
        &mut self,
        unfinished: harness::UnfinishedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .sessions
            .iter()
            .find(|session| session.session_id == unfinished.session_id)
            .cloned()
            .unwrap_or_else(|| jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: unfinished.session_id,
                working_dir: unfinished.working_dir,
                title: Some(unfinished.title),
                status: "idle".into(),
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
            });
        self.activate_session(session, window, cx);
    }
}
