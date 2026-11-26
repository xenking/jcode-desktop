//! Opening and closing panels: sessions, terminals, files, accounts and applets.
use super::*;

impl Workspace {
    pub(super) fn new_panel(&mut self, _: &NewPanel, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_panel {
            self.open_new_session_window(cx);
            return;
        }
        self.tutorial_cue("Enter", "New session", "new", cx);
        self.learned("new_panel", cx);
        self.open_new_session(cx);
    }

    pub(super) fn fork_panel(&mut self, _: &ForkPanel, _: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.slots.get(self.active).and_then(|slot| {
            let panel = slot.panel.read_chat(cx)?;
            (!panel.is_preview() && panel.can_fork()).then(|| panel.session_id.clone())
        }) else {
            return;
        };
        self.bridge.send(Command::Fork { session_id });
    }

    pub(super) fn new_panel_in_pinned_directory(
        &mut self,
        _: &NewPanelInPinnedDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_panel(&NewPanel, window, cx);
    }

    pub(super) fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        let working_dir = self.default_working_dir();
        if self.single_panel {
            if let Err(error) = panel_window::open_terminal_window(working_dir, self.host, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let host = self.host;
        let terminal = cx.new(|cx| TerminalPanel::new(working_dir, None, None, host, cx));
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Terminal(terminal),
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
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn close_accounts(
        &mut self,
        panel: &Entity<Panel>,
        source: &Entity<Panel>,
        choose_model: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.chat() == Some(panel))
        else {
            return;
        };
        self.set_active(index, cx);
        self.close_panel(&ClosePanel, window, cx);
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.chat() == Some(source))
        {
            self.set_active(index, cx);
            if source.read(cx).can_refresh_account_runtime() {
                self.bridge.send(Command::RefreshRuntime {
                    session_id: source.read(cx).session_id.clone(),
                });
            }
            if choose_model {
                source.update(cx, |panel, cx| panel.choose_account_model(cx));
            }
            self.retarget_camera();
            self.focus_active(window, cx);
        }
    }

    pub(crate) fn install_panel_window_origin(cx: &mut App) {
        cx.set_global(panel_window::OriginRoutes {
            restore: |session, fallback, refresh, choose_model, window, cx| {
                let Some(Some(root)) = window.root::<Self>() else { return false };
                root.update(cx, |workspace, cx| {
                    let source = workspace.slots.iter()
                        .find(|slot| !slot.closing && session == Some(slot.panel.session_id(cx)))
                        .and_then(|slot| slot.panel.chat().cloned())
                        .or(fallback);
                    let Some(source) = source else { return };
                    if refresh && source.read(cx).can_refresh_account_runtime() {
                        workspace.bridge.send(Command::RefreshRuntime {
                            session_id: source.read(cx).session_id.clone(),
                        });
                    }
                    if choose_model {
                        workspace.open_model_window_or_picker(&OpenModelPicker { source: source.entity_id() }, window, cx);
                    }
                    source.update(cx, |panel, cx| panel.focus_input(window, cx));
                    window.activate_window();
                });
                true
            },
            fleet: |action, window, cx| {
                if let Some(Some(root)) = window.root::<Self>() {
                    root.update(cx, |workspace, cx| workspace.open_fleet_session(action, window, cx));
                    window.activate_window();
                }
            },
        });
    }

    pub(super) fn open_model_window_or_picker(
        &mut self,
        request: &OpenModelPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self
            .slots
            .iter()
            .find(|slot| !slot.closing && slot.panel.entity_id() == request.source)
            .and_then(|slot| slot.panel.chat().cloned())
        else {
            return;
        };
        source.update(cx, |panel, cx| panel.toggle_model_picker(window, cx));
    }

    pub(super) fn open_accounts(
        &mut self,
        request: &OpenAccounts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source_index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.entity_id() == request.source)
        else {
            return;
        };
        let Some(source) = self.slots[source_index].panel.chat().cloned() else {
            return;
        };
        if source.read(cx).is_accounts_panel() {
            if let Some(command) = &request.login_command {
                source.update(cx, |panel, cx| panel.login_command(command, cx));
            }
            self.set_active(source_index, cx);
            self.retarget_camera();
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let source_session = source.read(cx).session_id.clone();
        if self.single_panel {
            let preview = source.read(cx).preview_state;
            let panel =
                cx.new(|cx| Panel::new_accounts(&source_session, preview, self.bridge.clone(), cx));
            match panel_window::open_panel_window(panel, Some(source), window, cx) {
                Ok(panel) => {
                    if let Some(command) = &request.login_command {
                        panel.update(cx, |panel, cx| {
                            panel.login_command(command, cx);
                        });
                    }
                }
                Err(error) => eprintln!("could not open Accounts window: {error:#}"),
            }
            return;
        }
        if let Some(index) = self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.session_id(cx) == format!("accounts://{source_session}")
        }) {
            if let Some(command) = &request.login_command {
                self.slots[index].panel.update_chat(cx, |panel, cx| {
                    panel.login_command(command, cx);
                });
            }
            self.set_active(index, cx);
            self.retarget_camera();
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let row = self.slots[source_index].row;
        let preview = source.read(cx).preview_state;
        let panel =
            cx.new(|cx| Panel::new_accounts(&source_session, preview, self.bridge.clone(), cx));
        if let Some(command) = &request.login_command {
            panel.update(cx, |panel, cx| {
                panel.login_command(command, cx);
            });
        }
        let source_for_close = source.clone();
        cx.subscribe_in(
            &panel,
            window,
            move |this, _, event: &crate::panel::AccountsPanelRedeemReset, window, cx| {
                this.open_usage_reset(&event.0, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &panel,
            window,
            move |this, panel, _: &crate::panel::AccountsPanelClosed, window, cx| {
                this.close_accounts(panel, &source_for_close, false, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &panel,
            window,
            move |this, panel, _: &crate::panel::AccountsPanelChooseModel, window, cx| {
                this.close_accounts(panel, &source, true, window, cx);
            },
        )
        .detach();
        let width_fraction = spawned_panel_width(self.slots.len());
        let insert_at = source_index + 1;
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::Chat(panel),
                row,
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
        if self.active >= insert_at {
            self.active += 1;
        }
        let row_count = self
            .slots
            .iter()
            .filter(|slot| slot.row == row && !slot.closing)
            .count();
        let source = &mut self.slots[source_index];
        let width = demoted_width(source.width_fraction, row_count);
        if width != source.width_fraction {
            source.width_fraction = width;
            source.animated_width.set(width, Instant::now());
            source.restore_fraction = None;
        }
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn open_code_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_document_file(
            &crate::ui_actions::OpenDocumentFile {
                path,
                fragment: None,
            },
            window,
            cx,
        );
    }

    pub(super) fn open_document_file(
        &mut self,
        action: &crate::ui_actions::OpenDocumentFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let requested = action.path.clone();
        let fragment = action.fragment.clone();
        cx.spawn_in(window, async move |this, cx| {
            let path = cx
                .background_executor()
                .spawn(async move { requested.canonicalize().unwrap_or(requested) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_resolved_document_file(path, fragment, window, cx);
            });
        })
        .detach();
    }

    pub(super) fn open_resolved_document_file(
        &mut self,
        path: PathBuf,
        fragment: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let show_heading = |file: &Entity<FilePanel>, cx: &mut App| {
            if let Some(fragment) = &fragment {
                file.update(cx, |file, cx| file.show_heading(fragment, cx));
            }
        };
        let session_id = FilePanel::session_id_for(&path);
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == session_id)
        {
            if let Some(file) = self.slots[index].panel.profile().file() {
                show_heading(file, cx);
            }
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }

        let width_fraction = spawned_panel_width(self.slots.len());
        let file = cx.new(|cx| FilePanel::new(path, cx));
        if self.single_panel {
            match panel_window::open_file_window(file, window, cx) {
                Ok(file) => show_heading(&file, cx),
                Err(error) => eprintln!("Could not open panel window: {error:#}"),
            }
            return;
        }
        show_heading(&file, cx);
        let insert_at = if self.slots.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.slots.insert(
            insert_at,
            Slot {
                panel: PanelItem::File(file),
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
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn new_unfinished_work(
        &mut self,
        _: &NewUnfinishedWork,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == "unfinished-work")
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let workspace = cx.weak_entity();
        let panel = cx.new(|cx| {
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
        });
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
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
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn orchestration_opener(&self, cx: &mut Context<Self>) -> crate::panel::SessionOpener {
        let workspace = cx.weak_entity();
        std::sync::Arc::new(move |session, window, cx| {
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.activate_unfinished_session(session, window, cx);
            });
        })
    }

    /// One orchestration panel per workspace: live sessions, whether each is
    /// running, and their todos.
    pub(super) fn open_orchestration(
        &mut self,
        _: &OpenOrchestration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_orchestration(None, window, cx);
    }

    pub(super) fn show_orchestration(
        &mut self,
        target: Option<jcode_sdk::SessionInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = target.map(|session| {
            let source = self
                .slots
                .iter()
                .find(|slot| !slot.closing && slot.panel.session_id(cx) == session.session_id);
            (session, source.and_then(|slot| slot.panel.chat().cloned()))
        });
        if let Some(index) = self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.session_id(cx) == crate::panel::orchestration::SESSION_ID
        }) {
            self.slots[index]
                .panel
                .update_chat(cx, |panel, cx| panel.set_orchestration_focus(focus, cx));
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let opener = self.orchestration_opener(cx);
        let panel = cx.new(|cx| Panel::new_orchestration(opener, self.bridge.clone(), cx));
        if self.single_panel {
            match panel_window::open_panel_window(panel, None, window, cx) {
                Ok(panel) => panel.update(cx, |panel, cx| panel.set_orchestration_focus(focus, cx)),
                Err(error) => eprintln!("Could not open orchestration window: {error:#}"),
            }
        } else {
            panel.update(cx, |panel, cx| panel.set_orchestration_focus(focus, cx));
            self.insert_spawned_panel(panel, window, cx);
        }
    }

    pub(super) fn open_openrig(&mut self, _: &OpenOpenrig, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == crate::panel::openrig::SESSION_ID)
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let panel = cx.new(|cx| Panel::new_openrig(self.bridge.clone(), cx));
        self.insert_spawned_panel(panel, window, cx);
    }

    pub(super) fn insert_spawned_panel(
        &mut self,
        panel: Entity<Panel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
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
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn open_todoist(&mut self, _: &OpenTodoist, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.panel.session_id(cx) == "todoist://tasks")
        {
            self.set_active(index, cx);
            self.focus_active(window, cx);
            return;
        }
        let width_fraction = spawned_panel_width(self.slots.len());
        let panel = cx.new(|cx| Panel::new_todoist(self.bridge.clone(), cx));
        if self.single_panel {
            if let Err(error) = panel_window::open_panel_window(panel, None, window, cx) {
                eprintln!("Could not open panel window: {error:#}");
            }
            return;
        }
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
                    transition::policy(Transition::PanelOpen).duration,
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
        crate::sounds::play(crate::sounds::Cue::PanelOpen, cx);
        self.set_active(insert_at, cx);
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Open a session without attributing the choice to the keyboard. Pointer
    /// paths call this directly, so clicking never earns keyboard credit.
    pub(super) fn open_new_session(&mut self, cx: &mut Context<Self>) {
        self.open_default_draft(self.new_session_directory(), cx);
    }

    pub(super) fn close_panel(&mut self, _: &ClosePanel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(slot) = self.slots.get(self.active).filter(|slot| !slot.closing)
            && !slot.panel.surface().confirm_close(cx)
        {
            cx.stop_propagation();
            return;
        }
        if self.single_panel && self.active == 0 {
            // A standalone chat has no empty workspace to return to.
            window.remove_window();
            return;
        }
        self.tutorial_cue("Q", "Close panel", "close", cx);
        if self
            .slots
            .get(self.active)
            .is_none_or(|slot| slot.row != self.active_row || slot.closing)
        {
            return;
        }
        if self.slots[self.active]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::is_default_directory)
        {
            self.close_default_directory_picker(cx);
            self.focus_active(window, cx);
            return;
        }
        if self.slots[self.active]
            .panel
            .read_chat(cx)
            .is_some_and(Panel::is_machines)
        {
            self.close_machines(cx);
            self.focus_active(window, cx);
            return;
        }
        self.learned("close_panel", cx);
        crate::sounds::play(crate::sounds::Cue::PanelClose, cx);
        let closed = self.active;
        let closed_id = self.slots[closed].panel.entity_id();
        self.slots[closed].closing = true;
        self.slots[closed].close_progress.set(0.0, Instant::now());
        let session_id = self.slots[closed].panel.session_id(cx).to_owned();
        // An open queue edit must not keep the daemon from delivering it.
        self.slots[closed]
            .panel
            .update_chat(cx, |panel, _| panel.release_queue_edit_hold());
        // Only a runtime conversation has a session to stop watching.
        let watched = self.slots[closed].panel.read_chat(cx).is_some_and(|panel| {
            !panel.is_pending_session()
                && !panel.is_side_document()
                && !panel.is_change_review()
                && !panel.is_accounts_panel()
                && !panel.is_machines()
                && !panel.is_preview()
        });
        if watched {
            self.sidebar_closed_sessions.insert(session_id.clone());
            self.bridge.send(Command::Unwatch { session_id });
        }
        if self.previous == Some(closed_id) {
            self.previous = None;
        }
        let remaining: Vec<_> = self
            .row_indices(self.active_row)
            // Dismissed slots remain mounted for their fade. In particular,
            // closing from the right must not return to the fading neighbour
            // on the next key repeat.
            .filter(|&index| !self.slots[index].closing)
            .collect();
        // A fading neighbour no longer owns canvas space. Restore the lone
        // ordinary column now, rather than leaving half the window empty after
        // the fade completes. Non-default widths remain the user's choice.
        if let [index] = remaining.as_slice() {
            let slot = &mut self.slots[*index];
            if slot.width_fraction == DEFAULT_WIDTH {
                slot.width_fraction = 1.0;
                slot.animated_width.set(1.0, Instant::now());
                slot.restore_fraction = None;
            }
        }
        self.active = if self.single_panel {
            0
        } else if self.slots[closed].panel.profile().is_changelog() {
            self.previous
                .and_then(|id| {
                    remaining
                        .iter()
                        .copied()
                        .find(|&index| self.slots[index].panel.entity_id() == id)
                })
                .unwrap_or_else(|| focus_after_close(closed, &remaining))
        } else {
            focus_after_close(closed, &remaining)
        };
        self.row_focus[self.active_row] = self
            .slots
            .get(self.active)
            .filter(|slot| slot.row == self.active_row && !slot.closing)
            .map(|slot| slot.panel.entity_id());
        self.retarget_camera();
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn remove_finished_closing_panels(
        &mut self,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| slot.panel.entity_id());
        let mut finished = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let progress = slot.close_progress.sample(now);
            if slot.closing && !slot.close_progress.is_animating() && progress <= f32::EPSILON {
                finished.push(index);
            }
        }
        if finished.is_empty() {
            return;
        }
        for index in finished.into_iter().rev() {
            self.slots.remove(index);
        }
        self.active = active_id
            .and_then(|id| {
                self.slots
                    .iter()
                    .position(|slot| slot.panel.entity_id() == id)
            })
            .unwrap_or_else(|| self.slots.len().saturating_sub(1));
        if self.slots.is_empty() {
            window.focus(&self.focus_handle, cx);
        }
        self.retarget_camera();
    }
}
