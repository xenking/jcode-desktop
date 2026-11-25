//! The sidebar, painted.
use super::*;

impl Workspace {
    pub(super) fn scroll_sidebar_navigation_to(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let handle = &self.sidebar_navigation_scroll;
        let bounds = handle.bounds();
        let width = f32::from(bounds.size.width).max(1.0);
        let max = f32::from(handle.max_offset().x).max(0.0);
        let thumb = (width * width / (width + max)).max(28.0).min(width);
        let travel = (width - thumb).max(1.0);
        let progress =
            ((f32::from(position.x - bounds.left()) - thumb / 2.0) / travel).clamp(0.0, 1.0);
        handle.set_offset(gpui::point(px(-max * progress), px(0.0)));
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_sidebar_navigation_scrollbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let handle = &self.sidebar_navigation_scroll;
        let width = f32::from(handle.bounds().size.width).max(1.0);
        let max = f32::from(handle.max_offset().x).max(0.0);
        let thumb = (width * width / (width + max)).max(28.0).min(width);
        let progress = if max > 0.0 {
            (-f32::from(handle.offset().x) / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let track = div()
            .id("sidebar-navigation-scrollbar")
            .debug_selector(|| "sidebar-navigation-scrollbar".into())
            .relative()
            .w_full()
            .h(px(if folders { 16.0 } else { 6.0 }))
            .when(folders, |el| el.absolute().top(px(10.0)))
            .flex_none()
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    this.scroll_sidebar_navigation_to(event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if event.dragging() {
                    this.scroll_sidebar_navigation_to(event.position, cx);
                }
            }))
            .when(!folders, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .w_full()
                        .h(px(4.0))
                        .rounded_full()
                        .bg(Theme::global().PANEL_BORDER),
                )
            })
            .when(!folders, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .left(relative((width - thumb) * progress / width))
                        .w(relative(thumb / width))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(Theme::global().TEXT_FAINT),
                )
            });
        div()
            .relative()
            .w_full()
            .h(px(if folders { 10.0 } else { 6.0 }))
            .flex_none()
            .child(track)
            .into_any_element()
    }

    pub(super) fn render_files_sidebar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self
            .slots
            .get(self.active)
            .is_some_and(|slot| harness::is_remote_session(&self.panel_metadata(slot, cx).session_id))
        {
            return div().debug_selector(|| "remote-file-browser-notice".into()).p_3().text_size(px(11.0)).text_color(Theme::global().TEXT_DIM)
                .child("This panel runs on a remote machine. Ask Jcode to inspect its files in chat. The local file browser is available on local panels.")
                .into_any_element();
        }
        let root = self.file_browser_root(cx);
        self.set_file_tree_root(&root);
        div()
            .id("sidebar-file-list")
            .debug_selector(|| "sidebar-file-list".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.sidebar_scroll)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .font_family(Theme::global().FONT_MONO)
                    .text_size(px(10.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(root.display().to_string()),
            )
            .children(self.render_file_entries(&root, 0, cx))
            .into_any_element()
    }

    pub(super) fn render_sidebar_scrollbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let sessions = self.sidebar_view == SidebarView::Sessions;
        let outset = if self.layout_mode == crate::config::LayoutMode::FolderTabs {
            SIDEBAR_SCROLLBAR_OUTSET
        } else {
            0.0
        };
        div()
            .id("sidebar-scroll-gutter")
            .debug_selector(|| "sidebar-scroll-gutter".into())
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-outset))
            .w(px(crate::scrollbar::GUTTER))
            // The gutter is a sibling overlay, not part of the scrollable
            // content. Handle its wheel input just like the folder-tab strip,
            // and consume it so the list underneath cannot scroll twice.
            .on_scroll_wheel(cx.listener(
                move |this, event: &gpui::ScrollWheelEvent, window, cx| {
                    let dy = event.delta.pixel_delta(window.line_height()).y;
                    if sessions {
                        this.sidebar_sessions_list.scroll_by(-dy);
                    } else {
                        let handle = &this.sidebar_scroll;
                        let y = (handle.offset().y + dy).clamp(-handle.max_offset().y, px(0.0));
                        handle.set_offset(gpui::point(px(0.0), y));
                    }
                    cx.stop_propagation();
                    cx.notify();
                },
            ))
            .child(if sessions {
                crate::scrollbar::vertical_list(&self.sidebar_sessions_list, "sidebar-scrollbar")
            } else {
                crate::scrollbar::vertical_with_track(&self.sidebar_scroll, "sidebar-scrollbar")
            })
            .into_any_element()
    }

    /// Replace the set of sessions the daemon is running. Retains the existing
    /// mark of a session that is still running so its animation continues.
    pub(super) fn set_daemon_running(&mut self, running: HashSet<String>, cx: &mut Context<Self>) {
        if running.len() == self.daemon_running.len()
            && running
                .iter()
                .all(|id| self.daemon_running.contains_key(id))
        {
            return;
        }
        self.daemon_running.retain(|id, _| running.contains(id));
        for id in running {
            self.daemon_running
                .entry(id)
                .or_insert_with(|| Panel::daemon_sidebar_mark(cx));
        }
        cx.notify();
    }

    /// Sidebar activity for a session without an open panel.
    pub(super) fn daemon_running_mark(&self, session_id: &str) -> Option<gpui::AnyView> {
        self.daemon_running.get(session_id).cloned()
    }

    pub(super) fn render_sidebar(&mut self, fullscreen: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Release checks and the packaged automatic-update loop start here, on
        // the first frame of the window, not from the avatar's update badge: a
        // fresh packaged launch has no release state yet, so there is nothing
        // for the badge to show until this runs. Idempotent, so every later
        // render is free.
        updates::ensure_release_check();
        let sidebar_panels = self
            .sidebar_panel_metadata
            .as_ref()
            .expect("sidebar metadata prepared before render")
            .read(cx);
        let active_id = self
            .slots
            .get(self.active)
            .map(|slot| sidebar_panels[&slot.panel.entity_id()].session_id.clone());
        let open_marks = self
            .slots
            .iter()
            .filter(|slot| !slot.closing)
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.sidebar_activity())
            })
            .collect::<HashMap<_, _>>();
        let open_todos = self
            .slots
            .iter()
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.latest_todo_progress())
            })
            .collect::<HashMap<_, _>>();
        let working = open_marks
            .iter()
            .filter(|(_, mark)| mark.is_some())
            .map(|(id, _)| id.clone())
            .chain(
                self.daemon_running
                    .keys()
                    .filter(|id| !open_marks.contains_key(*id))
                    .cloned(),
            )
            .collect::<HashSet<_>>();
        self.sidebar_title_pulses.retain(|id| working.contains(id));
        let open_titles = self
            .slots
            .iter()
            .map(|slot| {
                let panel = &sidebar_panels[&slot.panel.entity_id()];
                (panel.session_id.clone(), panel.title.to_string())
            })
            .collect::<HashMap<_, _>>();
        // A session which already has a panel is navigation. Everything else
        // is an invitation to open another panel, so keep those two actions in
        // visibly separate sections. Preserve the TUI saved/recency ordering
        // within each section.
        // Live panels are authoritative, including sessions still waiting for
        // creation/catalog acknowledgement. Never wait for a server refresh to
        // make a newly opened neighbor reachable in the sidebar.
        let mut catalog = self.sessions.clone();
        let mut known = catalog
            .iter()
            .map(|session| session.session_id.clone())
            .collect::<HashSet<_>>();
        // Keep legacy first-record lookup semantics even for duplicate catalog ids.
        let duplicate_catalog_ids = known.len() != catalog.len();
        for slot in self.slots.iter().filter(|slot| !slot.closing) {
            let panel = &sidebar_panels[&slot.panel.entity_id()];
            if (panel.supports_voice() || harness::is_remote_session(&panel.session_id))
                && panel.listable
                && known.insert(panel.session_id.clone())
            {
                catalog.push(jcode_sdk::SessionInfo {
                    user_connected: false,
                    session_id: panel.session_id.clone(),
                    working_dir: panel.working_dir.clone(),
                    title: Some(panel.title.to_string()),
                    status: "active".into(),
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
            }
        }
        // Only this window's explicitly closed sessions belong in Past. Keep
        // the full runtime catalog elsewhere for search and session recovery.
        let mut kept = catalog
            .iter()
            .filter(|session| {
                open_marks.contains_key(&session.session_id)
                    || session.user_connected
                    || self.sidebar_closed_sessions.contains(&session.session_id)
            })
            .map(|session| session.session_id.clone())
            .collect::<HashSet<_>>();
        // The TUI shows a coordinator's workers while nobody has that
        // coordinator open here. A live swarm is live work, not a window's
        // property, so list it wherever it is driven from.
        // A worker's own record can outrun a terminal swarm snapshot (a daemon
        // reload rewrites nothing), so liveness reads both.
        let live_swarms =
            sidebar_swarm::live_swarm_roots(&catalog, &self.worker_meta, swarm_now_ms());
        kept.extend(live_swarms.iter().cloned());
        // Headless swarm workers are never opened or closed in this window,
        // but they belong under a coordinator that is listed here.
        sidebar_swarm::keep_workers_of(&catalog, &mut kept);
        let preferences = self.session_catalog;
        catalog.retain(|session| {
            preferences.origin.matches(session)
                && (kept.contains(&session.session_id)
                    || (session.is_agent_started()
                        && (preferences.origin == sidebar_session_catalog::OriginFilter::Agents
                            || working.contains(&session.session_id))))
        });
        let original_catalog = duplicate_catalog_ids.then(|| catalog.clone());
        // Sort the owned render catalog in place, avoiding a second session clone.
        sort_sidebar_sessions(&mut catalog, preferences.sort);
        let ordered_sessions = catalog;
        let swarm = sidebar_swarm::groups(&ordered_sessions);
        let reply_times: HashMap<_, _> =
            if preferences.sort == sidebar_session_catalog::Sort::AgentActivity {
                ordered_sessions
                    .iter()
                    .filter_map(|session| {
                        Some((
                            session.session_id.clone(),
                            session.last_agent_activity_at_ms()?,
                        ))
                    })
                    .collect()
            } else {
                HashMap::new()
            };
        // Swarms and worktrees are one concept here: an agent is a thread. It
        // nests under its coordinator while it shares that checkout, and
        // surfaces under its own branch when it works in a separate worktree.
        let mut agents: HashMap<String, (usize, usize)> = HashMap::new();
        let mut nested_workers: HashMap<String, Vec<(usize, crate::swarm_view::Worker)>> =
            HashMap::new();
        let swarm_now = swarm_now_ms();
        let mut nested = HashSet::new();
        for (root, children) in &swarm.children {
            let root_checkout = original_catalog
                .as_ref()
                .unwrap_or(&ordered_sessions)
                .iter()
                .find(|session| &session.session_id == root)
                .and_then(|session| self.sidebar_projects.checkout(session))
                .map(|checkout| checkout.path);
            for child in children {
                let own = self
                    .sidebar_projects
                    .checkout(&child.session)
                    .map(|checkout| checkout.path);
                let worker = crate::swarm_view::Worker::from_session(
                    &child.session,
                    self.worker_meta.get(&child.session.session_id),
                    swarm_now,
                );
                let separate_checkout = own.is_some() && own != root_checkout;
                if separate_checkout && !worker.is_live() {
                    continue;
                }
                // Live worktree agents remain visible under their coordinator
                // and in their own checkout.
                if !separate_checkout {
                    nested.insert(child.session.session_id.clone());
                }
                let running = worker.is_running();
                nested_workers
                    .entry(root.clone())
                    .or_default()
                    .push((child.depth, worker));
                let entry = agents.entry(root.clone()).or_default();
                entry.0 += 1;
                if running {
                    entry.1 += 1;
                }
            }
        }
        // Long-running coordinators own dozens of workers. Show the live ones
        // and the latest few finished, fold the rest into "+N finished".
        let mut folded_workers: HashMap<String, usize> = HashMap::new();
        for (root, workers) in nested_workers.iter_mut() {
            workers.sort_by(|(_, a), (_, b)| {
                crate::swarm_view::spawn_order(&a.session_id)
                    .cmp(&crate::swarm_view::spawn_order(&b.session_id))
            });
            let (shown, hidden) = sidebar_swarm::visible(workers);
            if !self
                .expanded_lines
                .contains(&sidebar_swarm::finished_key(root))
            {
                *workers = shown;
            }
            if hidden > 0 {
                folded_workers.insert(root.clone(), hidden);
            }
            if preferences.sort == sidebar_session_catalog::Sort::AgentActivity {
                workers.sort_by_key(|(_, worker)| {
                    std::cmp::Reverse(reply_times.get(&worker.session_id))
                });
            }
        }
        // Fading local panels stay mounted, but must not reappear as history
        // or stale globally connected rows before their close completes.
        let closing_sessions = self
            .slots
            .iter()
            .filter(|slot| slot.closing)
            .map(|slot| sidebar_panels[&slot.panel.entity_id()].session_id.clone())
            .collect::<HashSet<_>>();
        let (mut open_sessions, other_sessions): (Vec<_>, Vec<_>) = ordered_sessions
            .into_iter()
            .filter(|session| {
                !nested.contains(&session.session_id)
                    && !closing_sessions.contains(&session.session_id)
            })
            .partition(|session| {
                open_marks.contains_key(&session.session_id)
                    || session.user_connected
                    // A coordinator with live workers is live work even when
                    // this window has no panel for it: putting it in history
                    // would fold it into a collapsed project.
                    || live_swarms.contains(&session.session_id)
            });
        let mut panel_positions = self.slots.iter().enumerate().collect::<Vec<_>>();
        panel_positions.sort_by_key(|(slot_index, slot)| (slot.row, *slot_index));
        let panel_positions = panel_positions
            .into_iter()
            .enumerate()
            .map(|(position, (_, slot))| {
                (
                    sidebar_panels[&slot.panel.entity_id()].session_id.clone(),
                    position,
                )
            })
            .collect::<HashMap<_, _>>();
        if preferences.sort == sidebar_session_catalog::Sort::Recent {
            open_sessions.sort_by_key(|session| {
                panel_positions
                    .get(&session.session_id)
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        // External user clients are live too, but are not locally open panels.
        // Keep their rows without changing navigation or selection semantics.
        // Group by project (Git repository, else spawn directory). Projects
        // appear in the order of their first open panel, then by history
        // recency. Inside a project, threads group by checkout (the main
        // working tree first, then linked worktrees), and open panels precede
        // history within each checkout.
        //
        // User-created collections come first, in their own order, and take
        // their sessions out of the directory groups below.
        let mut groups: Vec<(sidebar_projects::Project, usize, usize)> = Vec::new();
        let mut group_indices: HashMap<String, usize> = HashMap::new();
        let mut members: Vec<Vec<SidebarRow>> = Vec::new();
        let mut project_checkouts: Vec<Vec<sidebar_projects::Checkout>> = Vec::new();
        let collection_order = self
            .collections
            .all()
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>();
        let rank_collection = |session: &jcode_sdk::SessionInfo| {
            self.collections
                .collection_of(&session.session_id)
                .and_then(|c| collection_order.iter().position(|id| *id == c.id))
                .unwrap_or(usize::MAX)
        };
        let mut grouped = open_sessions
            .into_iter()
            .map(|session| (true, session))
            .chain(other_sessions.into_iter().map(|session| (false, session)))
            .collect::<Vec<_>>();
        // Stable: open-before-history and recency survive inside each group.
        grouped.sort_by_key(|(_, session)| rank_collection(session));
        for (live, session) in grouped {
            // open_marks contains exactly the non-closing local panel ids.
            let open = open_marks.contains_key(&session.session_id);
            let collection = self
                .collections
                .collection_of(&session.session_id)
                .map(|c| (c.id.clone(), c.name.clone()));
            let (project, subdirectory) = match collection {
                // Inside a collection, the session's folder is a detail line.
                Some((id, name)) => (
                    sidebar_projects::Project {
                        key: sidebar_collections::key(&id),
                        label: name,
                        location: None,
                        git: false,
                    },
                    sidebar_session_directory(&session),
                ),
                None => self.sidebar_projects.project(&session),
            };
            // Collections mix folders, so they never split into checkouts.
            let checkout = if sidebar_collections::id_from_key(&project.key).is_some() {
                None
            } else {
                self.sidebar_projects.checkout(&session)
            };
            let index = match group_indices.get(&project.key).copied() {
                Some(index) => index,
                None => {
                    group_indices.insert(project.key.clone(), groups.len());
                    groups.push((project, 0, 0));
                    members.push(Vec::new());
                    project_checkouts.push(Vec::new());
                    groups.len() - 1
                }
            };
            if let Some(checkout) = &checkout
                && !project_checkouts[index]
                    .iter()
                    .any(|c| c.path == checkout.path)
            {
                project_checkouts[index].push(checkout.clone());
            }
            // Inside a linked worktree the path relative to the main checkout
            // is noise. The checkout header already names the branch.
            let subdirectory = match &checkout {
                Some(checkout) if checkout.linked => session
                    .working_dir
                    .as_deref()
                    .and_then(|dir| Path::new(dir).strip_prefix(&checkout.path).ok())
                    .filter(|relative| !relative.as_os_str().is_empty())
                    .map(|relative| relative.to_string_lossy().into_owned()),
                _ => subdirectory,
            };
            groups[index].1 += usize::from(live);
            groups[index].2 += 1;
            let agents = agents.get(&session.session_id).copied().unwrap_or_default();
            members[index].push(SidebarRow {
                open,
                live,
                session,
                project: index,
                subdirectory,
                header: false,
                collapsed: false,
                divider: false,
                checkout,
                checkout_header: false,
                section: None,
                agents,
            });
        }
        // The main working tree leads, then worktrees in order of first use.
        for checkouts in &mut project_checkouts {
            checkouts.sort_by_key(|checkout| checkout.linked);
        }
        for (index, rows) in members.iter_mut().enumerate() {
            let rank = |row: &SidebarRow| {
                row.checkout
                    .as_ref()
                    .and_then(|c| {
                        project_checkouts[index]
                            .iter()
                            .position(|p| p.path == c.path)
                    })
                    .unwrap_or(usize::MAX)
            };
            rows.sort_by_key(rank);
        }
        // Branch rows appear once a project spans several checkouts. A single
        // checkout shows its branch on the project header instead.
        let checkout_rows = project_checkouts
            .iter()
            .map(|checkouts| checkouts.len() > 1 || checkouts.iter().any(|c| c.linked))
            .collect::<Vec<_>>();
        let project_branches = project_checkouts
            .iter()
            .zip(&checkout_rows)
            .map(|(checkouts, rows)| {
                (!rows)
                    .then(|| checkouts.first().map(|c| c.branch.clone()))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let active_project = active_id.as_deref().and_then(|id| {
            members
                .iter()
                .position(|rows| rows.iter().any(|row| row.session.session_id == id))
        });
        self.sidebar_projects.sync_focus(
            active_id.as_deref(),
            active_project.map(|index| groups[index].0.key.as_str()),
        );
        let headers = groups.len() > 1
            || groups
                .first()
                .is_some_and(|(project, _, _)| project.key != sidebar_projects::OTHER);
        let any_live = groups.iter().any(|(_, live, _)| *live > 0);
        let collapsed_projects = groups
            .iter()
            .map(|(project, live, _)| {
                // A collection the user built stays open unless they fold it.
                let default = sidebar_collections::id_from_key(&project.key).is_none()
                    && any_live
                    && *live == 0;
                headers && self.sidebar_projects.collapsed(&project.key, default)
            })
            .collect::<Vec<_>>();
        let mut ordered_sessions = Vec::new();
        for (index, rows) in members.into_iter().enumerate() {
            let collapsed = collapsed_projects[index];
            let mut previous: Option<(bool, bool)> = None;
            let mut previous_checkout: Option<Option<String>> = None;
            for (position, mut row) in rows.into_iter().enumerate() {
                if collapsed && position > 0 {
                    // One representative keeps the header a real list row.
                    break;
                }
                row.header = headers && position == 0;
                row.collapsed = collapsed;
                let checkout = row.checkout.as_ref().map(|c| c.path.clone());
                if previous_checkout.as_ref() != Some(&checkout) {
                    row.checkout_header = checkout_rows[index] && !collapsed;
                    previous = None;
                }
                previous_checkout = Some(checkout);
                // Live includes externally connected user clients, while
                // row.open continues to mean navigation to a local panel.
                if !collapsed && previous.is_none_or(|(live, _)| live != row.live) {
                    row.section = Some(if row.live { "Live" } else { "Past" });
                }
                row.divider = !row.live
                    && previous.is_some_and(|(live, saved)| !live && saved && !row.session.saved);
                previous = Some((row.live, row.session.saved));
                ordered_sessions.push(row);
            }
        }
        let selection_order = ordered_sessions
            .iter()
            .filter(|row| row.open && !row.collapsed)
            .map(|row| row.session.session_id.clone())
            .collect::<Vec<_>>();
        self.sidebar_selection.retain(&selection_order);
        // Keep only visible rows (plus a small overscan) in GPUI's layout and
        // text-shaping work. Hover invalidates this view, so eagerly building
        // all history rows here makes pointer latency scale with history size.
        let layout = ordered_sessions
            .iter()
            .map(|row| {
                let session = &row.session;
                let selected = active_id.as_deref() == Some(session.session_id.as_str());
                SidebarSessionLayout {
                    session_id: session.session_id.clone(),
                    open: row.open,
                    project: row.project,
                    header: row.header,
                    checkout_header: row.checkout_header,
                    divider: row.divider,
                    section: row.section,
                    collapsed: row.collapsed,
                    selected,
                    saved: session.saved,
                    details: (row.subdirectory.is_some()
                        || sidebar_session_created_ms(&session.session_id).is_some()
                        || session.transcript_bytes.is_some_and(|bytes| bytes > 0)),
                    menu: self.collection_menu.is_open_for(&session.session_id),
                    workers: (!row.collapsed)
                        .then(|| nested_workers.get(&session.session_id))
                        .flatten()
                        .map(|workers| {
                            sidebar_swarm::layout_key(
                                workers,
                                folded_workers
                                    .get(&session.session_id)
                                    .copied()
                                    .unwrap_or(0),
                            )
                        })
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();
        let newly_opened_active = layout.iter().position(|row| {
            row.open
                && row.selected
                && !self
                    .sidebar_session_layout
                    .iter()
                    .any(|old| old.open && old.session_id == row.session_id)
        });
        sync_sidebar_session_layout(
            &self.sidebar_sessions_list,
            &mut self.sidebar_session_layout,
            layout,
        );
        if let Some(index) = newly_opened_active {
            self.sidebar_sessions_list.scroll_to_reveal_item(index);
        }
        // Render read cached Git state only. Probe what it lacked off-thread.
        self.sidebar_projects.probe_in_background(cx, |workspace: &mut Self| &mut workspace.sidebar_projects);
        let folders = self.layout_mode == crate::config::LayoutMode::FolderTabs;
        let folder_frame = self.folder_frame.clone();
        let workspace = cx.entity();
        let mut list = div()
            .id("sidebar-session-list")
            .debug_selector(|| "sidebar-session-list".into())
            .flex_1()
            .min_h_0()
            .overflow_hidden();
        list = list.flex().flex_col();
        list = list.child(self.render_sidebar_quick_actions(cx));
        if let Some(applets) = self.sidebar_applets.take() {
            list = list.child(applets);
        }
        if let Some(form) = self.render_worktree_form(cx) {
            list = list.child(form);
        }
        if let Some(form) = self.render_collection_form(cx) {
            list = list.child(form);
        }
        // Collections with no chats yet have no row to carry a header.
        let empty_collections = self
            .collections
            .all()
            .into_iter()
            .filter(|c| {
                !groups
                    .iter()
                    .any(|(p, _, _)| p.key == sidebar_collections::key(&c.id))
            })
            .collect::<Vec<_>>();
        if let Some(empty) = self.render_empty_collections(&empty_collections, cx) {
            list = list.child(empty);
        }
        if !self.sidebar_selection.is_empty() {
            list = list.child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.0))
                    .child(format!("{} selected", self.sidebar_selection.len()))
                    .child(
                        div()
                            .id("sidebar-close-selected")
                            .debug_selector(|| "sidebar-close-selected".into())
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(Theme::global().ACCENT_DIM)
                            .cursor_pointer()
                            .child("Close selected")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    let ids = this
                                        .sidebar_session_layout
                                        .iter()
                                        .filter(|s| this.sidebar_selection.contains(&s.session_id))
                                        .map(|s| s.session_id.clone())
                                        .collect();
                                    this.close_sidebar_sessions(ids, window, cx);
                                }),
                            ),
                    ),
            );
        }
        if !ordered_sessions.is_empty() {
            list = list.child(div().relative().w_full().flex_1().min_h_0().child(
                gpui::list(
                    self.sidebar_sessions_list.clone(),
                    move |sidebar_index, _window, cx| {
                        let row = &ordered_sessions[sidebar_index];
                        let is_open = row.open;
                        let live = row.live;
                        let session = &row.session;
                        workspace.update(cx, |this, cx| {
                            let mut list = div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .relative()
                                .when(sidebar_index == 0, |el| el.pt_2())
                                .when(sidebar_index + 1 == ordered_sessions.len(), |el| el.pb_2());
                            if row.header {
                                let (project, _, count) = &groups[row.project];
                                let header = if sidebar_collections::id_from_key(&project.key).is_some() {
                                    this.render_collection_header(row.project, project, *count, collapsed_projects[row.project], cx)
                                } else {
                                    this.render_project_header(row.project, project, *count, collapsed_projects[row.project], project_branches[row.project].clone(), cx)
                                };
                                list = list
                                    .when(sidebar_index > 0, |el| el.pt_3())
                                    .child(header);
                            }
                            if row.checkout_header {
                                list = list.child(this.render_checkout_header(sidebar_index, row.checkout.as_ref(), cx));
                            }
                            if row.collapsed {
                                return list.into_any_element();
                            }
                            if let Some(section) = row.section {
                                list = list.child(
                                    div()
                                        .debug_selector(move || format!("sidebar-section-{sidebar_index}"))
                                        .ml(px(if checkout_rows[row.project] { 30.0 } else { 20.0 }))
                                        .mt(px(2.0))
                                        .mb(px(2.0))
                                        .text_size(px(9.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Theme::global().TEXT_FAINT)
                                        .child(section),
                                );
                            }
                            if row.divider {
                                list = list.child(
                                    div()
                                        .id("sidebar-session-divider")
                                        .debug_selector(|| "sidebar-session-divider".into())
                                        .mx_4()
                                        .my_2()
                                        .border_t_1()
                                        .border_color(Theme::global().PANEL_BORDER),
                                );
                            }
                            let selected =
                                active_id.as_deref() == Some(session.session_id.as_str());
                            let (icon, title) = sidebar_session_title_with_open_title(
                                session,
                                open_titles.get(&session.session_id).map(String::as_str),
                            );
                            #[cfg(test)]
                            tests::SIDEBAR_TITLE_RENDERS.with(|count| count.set(count.get() + 1));
                            let directory = row.subdirectory.clone();
                            // Todo progress leads the details line: live from an
                            // open panel, otherwise from the persisted snapshot.
                            let todo_progress = open_todos
                                .get(&session.session_id)
                                .copied()
                                .flatten()
                                .or_else(|| this.sidebar_todo_progress.get(&session.session_id).copied());
                            let todo_session = session.clone();
                            let meta = sidebar_session_meta(session);
                            let details = match (directory, meta) {
                                (Some(directory), Some(meta)) => {
                                    Some(format!("{directory} · {meta}"))
                                }
                                (Some(directory), None) => Some(directory),
                                (None, Some(meta)) => Some(meta),
                                (None, None) => None,
                            };
                            // An open panel owns its richer live mark. Otherwise
                            // show the daemon's view, so sessions running in the
                            // background (other clients, swarm workers, closed
                            // panels) still spin in the sidebar.
                            let activity = match open_marks.get(&session.session_id) {
                                Some(mark) => mark.clone(),
                                None => this.daemon_running_mark(&session.session_id),
                            };

                            let group_selected = is_open && this.sidebar_selection.contains(&session.session_id);
                            let (_, agent_working) = row.agents;
                            let running_workers = nested_workers.get(&session.session_id).into_iter().flatten()
                                .filter(|(_, worker)| worker.is_running())
                                .map(|(_, worker)| worker.clone()).collect::<Vec<_>>();
                            let under_checkout = checkout_rows[row.project];
                            let close_id = session.session_id.clone();
                            let pin_id = session.session_id.clone();
                            let pinned = session.saved;
                            let agent_started = session.is_agent_started();
                            let release_id = session.session_id.clone();
                            let release_out_id = session.session_id.clone();
                            let context_id = session.session_id.clone();
                            let menu_open = this.collection_menu.is_open_for(&session.session_id);
                            let menu_session = session.session_id.clone();
                            let selection_order = selection_order.clone();
                            let hovered = this.sidebar_hovered_session.as_deref() == Some(session.session_id.as_str());
                            let working = activity.is_some();
                            let hover_id = session.session_id.clone();
                            let title_pulse = working.then(|| {
                                this.sidebar_title_pulses.get(
                                    &session.session_id,
                                    crate::pulse_text::Kind::PulseText {
                                        text: title.clone().into(),
                                        period: crate::pulse_text::TOOL_PULSE_PERIOD,
                                        curve: sidebar_title_pulse,
                                    },
                                    cx,
                                )
                            });
                            let session = session.clone();
                            let keyboard_session = session.clone();
                            list = list.child(
                                crate::keyboard::button(div().id(("sidebar-session", sidebar_index)))
                                    .on_click(cx.listener(move |this, event, window, cx| {
                                        if matches!(event, gpui::ClickEvent::Keyboard(_)) {
                                            cx.stop_propagation();
                                            this.activate_session(keyboard_session.clone(), window, cx);
                                        }
                                    }))
                                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                                        let current = this.sidebar_hovered_session.as_deref() == Some(hover_id.as_str());
                                        if *hovering && !current {
                                            this.sidebar_hovered_session = Some(hover_id.clone());
                                            cx.notify();
                                        } else if !*hovering && current {
                                            this.sidebar_hovered_session = None;
                                            cx.notify();
                                        }
                                    }))
                                    .group("sidebar-session-row")
                                    .debug_selector(move || {
                                        format!("sidebar-session-{sidebar_index}")
                                    })
                                    .ml_2()
                                    .when(under_checkout, |el| el.ml(px(18.0)))
                                    .mr_2()
                                    .mb(px(2.0))
                                    .relative()
                                    .px_2()
                                    .py(px(2.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .rounded_lg()
                                    .cursor_pointer()
                                    .when(selected && folders, |el| el.child(folder_surface::measure(
                                        folder_frame.clone(), folder_surface::Region::SelectedTab,
                                    )))
                                    .when(!folders, |el| el.mr_2().rounded_lg().bg(if selected {
                                        Theme::global().PANEL_BG
                                    } else {
                                        Theme::global().HEADER_BG
                                    }))
                                    .border_l_1()
                                    .border_t_1()
                                    .border_b_1()
                                    .border_color(gpui::rgba(0x00000000))
                                    .when(group_selected, |el| el.bg(Theme::global().ACCENT_DIM).border_color(Theme::global().ACCENT))
                                    .pr(px(crate::scrollbar::GUTTER + 8.0))
                                    .when(!selected, |el| el.hover(move |el| el.bg(Theme::global().PANEL_BG)))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, _cx| {
                                            // Release will explicitly focus the session's composer.
                                            // Otherwise the focusable workspace ancestor handles
                                            // this same press afterward and steals focus back.
                                            window.prevent_default();
                                            this.begin_sidebar_gesture(session.clone(), selection_order.clone(), is_open, event);
                                        }),
                                    )
                                    .on_mouse_up(gpui::MouseButton::Left, cx.listener(move |this, event, window, cx| {
                                        this.finish_sidebar_gesture(&release_id, true, event, window, cx);
                                    }))
                                    .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(move |this, event, window, cx| {
                                        this.finish_sidebar_gesture(&release_out_id, false, event, window, cx);
                                    }))
                                    // Right-click opens the collection menu.
                                    .on_mouse_down(gpui::MouseButton::Right, cx.listener(move |this, _, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.sidebar_gesture = None;
                                        this.collection_menu.show(&context_id);
                                        cx.notify();
                                    }))
                                    .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                        this.sidebar_gesture = None;
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .h(px(18.0))
                                            .child(
                                                div()
                                                    .w(px(16.0))
                                                    .flex_none()
                                                    .text_size(px(12.0))
                                                    .child(icon),
                                            )
                                            .child({
                                                let container = div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .max_h(px(18.0))
                                                    .truncate()
                                                    .debug_selector(move || {
                                                        format!(
                                                            "sidebar-session-title-{sidebar_index}"
                                                        )
                                                    })
                                                    .text_size(px(12.0))
                                                    .line_height(relative(1.5))
                                                    // Past sessions stay listed but read dimmer than live ones.
                                                    .when(!live, |el| el.text_color(Theme::global().TEXT_DIM));
                                                // A working session's title breathes in step
                                                // with running tool rows. The pulse is its own
                                                // view, so a tick redraws only the title instead
                                                // of the whole workspace.
                                                match title_pulse {
                                                    Some(pulse) => container.child(pulse),
                                                    None => container.child(title),
                                                }
                                            })
                                            .children(agent_started.then(|| sidebar_session_catalog::agent_badge()))
                                            .when(agent_working > 0, |row| row.child(
                                                div()
                                                    .id(("sidebar-agents", sidebar_index))
                                                    .debug_selector(move || format!("sidebar-agents-{sidebar_index}"))
                                                    .flex_none().h(px(16.0)).px(px(6.0))
                                                    .flex().items_center().gap(px(3.0)).rounded_full()
                                                    .bg(Theme::global().ACCENT_DIM)
                                                    .text_size(px(9.0)).text_color(Theme::global().TEXT_DIM)
                                                    .cursor_pointer()
                                                    .tooltip(move |_, cx| cx.new(|_| sidebar_header::WorkersTooltip(running_workers.clone())).into())
                                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                        window.prevent_default();
                                                        cx.stop_propagation();
                                                        this.sidebar_gesture = None;
                                                        this.sidebar_sessions_list.scroll_to(gpui::ListOffset { item_ix: sidebar_index, offset_in_item: px(0.0) });
                                                        cx.notify();
                                                    }))
                                                    .child(gpui::svg().data(include_bytes!("../../../assets/icons/swarm.svg").as_slice()).size(px(9.0)).text_color(Theme::global().TEXT_DIM))
                                                    .child(div().debug_selector(move || format!("sidebar-agents-count-{sidebar_index}-{agent_working}")).child(agent_working.to_string())),
                                            ))
                                            // Idle rows keep the pin visible when pinned. Hovering
                                            // swaps any spinner for the pin and close actions, so the
                                            // title can use the full width the rest of the time.
                                            .when(hovered || (pinned && !working), |row| row
                                                // Pinning is /save: pinned sessions sort to the top.
                                                .child(
                                                    div()
                                                        .id(("sidebar-pin", sidebar_index))
                                                        .debug_selector(move || format!("sidebar-pin-{sidebar_index}"))
                                                        .flex_none()
                                                        .size(px(18.0))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded_full()
                                                        .cursor_pointer()
                                                        .hover(|el| el.bg(Theme::global().TOOL_BG))
                                                        .tooltip(move |_, cx| {
                                                            cx.new(|_| remotes::HeaderTooltip(
                                                                if pinned { "Unpin session" } else { "Pin session" }.into(),
                                                            ))
                                                            .into()
                                                        })
                                                        .child(
                                                            gpui::svg()
                                                                .data(include_bytes!("../../../assets/icons/pin.svg").as_slice())
                                                                .size(px(11.0))
                                                                .text_color(if pinned {
                                                                    Theme::global().ACCENT
                                                                } else {
                                                                    Theme::global().TEXT_DIM
                                                                }),
                                                        )
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.sidebar_gesture = None;
                                                            this.set_session_pinned(&pin_id, !pinned, cx);
                                                        })),
                                                )
                                                // Last child: the hover close sits at the row's far right.
                                                .when(is_open && hovered, |row| row.child(
                                                    div().id(("sidebar-close", sidebar_index))
                                                        .debug_selector(move || format!("sidebar-close-{sidebar_index}"))
                                                        .flex_none().size(px(18.0)).flex().items_center().justify_center().cursor_pointer().child("×")
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                            window.prevent_default();
                                                            cx.stop_propagation();
                                                            this.sidebar_gesture = None;
                                                            this.close_sidebar_sessions(vec![close_id.clone()], window, cx);
                                                        }))
                                                ))
                                            )
                                            // Last child: the spinner sits at the row's far right.
                                            .when_some(activity.filter(|_| !hovered), |row, spinner| {
                                                row.child(
                                                    div()
                                                        .debug_selector(move || {
                                                            format!("sidebar-session-spinner-{sidebar_index}")
                                                        })
                                                        .flex_none()
                                                        .child(spinner),
                                                )
                                            })
                                    )
                                    .when(details.is_some() || todo_progress.is_some(), |row| {
                                        row.child(
                                            div()
                                                .when(todo_progress.is_none(), |line| line.pl(px(20.0)))
                                                .flex()
                                                .items_center()
                                                .gap(px(3.0))
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_size(px(9.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .when_some(todo_progress, |line, (done, total)| line.child(
                                                    crate::keyboard::press_button(
                                                        div().id(("sidebar-todos", sidebar_index)),
                                                        {
                                                            let todo_session = todo_session.clone();
                                                            let entity = cx.weak_entity();
                                                            move |window, cx| {
                                                                window.prevent_default();
                                                                cx.stop_propagation();
                                                                let _ = entity.update(cx, |this, cx| {
                                                                    this.sidebar_gesture = None;
                                                                    this.show_orchestration(Some(todo_session.clone()), window, cx);
                                                                });
                                                            }
                                                        },
                                                    )
                                                        .min_w_0().flex().items_center().gap(px(2.0))
                                                        .relative()
                                                        .debug_selector(move || format!("sidebar-todos-{sidebar_index}"))
                                                        .children(crate::fixture_control_targets::marker(
                                                            || format!("sidebar-todos:{}", todo_session.session_id),
                                                            || None,
                                                        ))
                                                        .when(done == total, |el| el.text_color(Theme::global().OK))
                                                        .rounded_full().pr(px(2.0)).cursor_pointer()
                                                        .hover(|el| el.bg(Theme::global().TOOL_BG))
                                                        .tooltip(|_, cx| cx.new(|_| remotes::HeaderTooltip("Open session todos".into())).into())
                                                        .child(div().debug_selector(move || format!("sidebar-todos-icon-{sidebar_index}")).flex_none().child(
                                                            gpui::svg().data(include_bytes!("../../../assets/icons/todo-check.svg").as_slice()).size(px(9.0)).text_color(if done == total { Theme::global().OK } else { Theme::global().TEXT_DIM })
                                                        ))
                                                        .child(div().min_w_0().truncate().child(format!("{done}/{total}"))),
                                                ))
                                                .when_some(details.map(|d| if todo_progress.is_some() { format!("· {d}") } else { d }), |line, details| {
                                                    line.child(div().min_w_0().flex_1().truncate().child(details))
                                                }),
                                        )
                                    }),
                            );
                            if let Some(workers) = nested_workers.get(&menu_session) {
                                let folded = folded_workers.get(&menu_session).copied().unwrap_or(0);
                                let expanded_lines = this.expanded_lines.clone();
                                list = list.child(sidebar_swarm::render_workers(
                                    sidebar_swarm::WorkerRowsLayout {
                                        sidebar_index,
                                        session_id: &menu_session,
                                        under_checkout,
                                        width: if this.compact_sidebar_open { SIDEBAR_WIDTH } else { this.sidebar_custom_width },
                                    },
                                    workers,
                                    folded,
                                    &mut this.swarm_marks,
                                    &expanded_lines,
                                    cx,
                                    |this, id, window, cx| {
                                        this.open_swarm_worker(id, cx);
                                        this.focus_active(window, cx);
                                    },
                                    |this, key, _| { crate::panel::expandable::Regions::flip(&mut this.expanded_lines, key); },
                                ));
                            }
                            if menu_open {
                                let menu = this.render_collection_menu(&menu_session, cx);
                                list = list.child(div().ml(px(28.0)).mr(px(crate::scrollbar::GUTTER + 8.0)).mb_1().child(menu));
                            }
                            list.into_any_element()
                        })
                    },
                )
                .size_full(),
            ));
        }

        if self.sidebar_session_layout.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_size(px(11.0))
                    .text_color(Theme::global().TEXT_DIM)
                    .child(if self.connected {
                        "no previous sessions"
                    } else {
                        "loading sessions..."
                    }),
            );
        }

        div()
            .w(px(if self.compact_sidebar_open {
                SIDEBAR_WIDTH
            } else {
                self.sidebar_custom_width
            }))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .debug_selector(|| "sidebar".into())
            .on_mouse_move(cx.listener(|this, event, _, _| this.move_sidebar_gesture(event)))
            .relative()
            .when(!folders, |el| el.bg(Theme::global().HEADER_BG))
            // Folder mode leaves this transparent: the native path owns its tab.
            .child({
                let navigation = if folders {
                    self.render_sidebar_roller(fullscreen, cx)
                } else {
                    div()
                        .h(px(TITLEBAR_HEIGHT))
                        // The transparent macOS titlebar puts the traffic lights in
                        // this row. Start navigation after them; on other platforms
                        // preserve the existing 12px inset.
                        .pl(px(sidebar_header_left_padding(fullscreen)))
                        .pr_3()
                        .flex()
                        .items_end()
                        .justify_start()
                        .relative()
                        // Keep one backing tone around the folder's top-left curve.
                        // A dark navigation rectangle here pinches that curve into a notch.
                        .bg(if folders {
                            Theme::global().HEADER_BG
                        } else {
                            Theme::global().PANEL_BG
                        })
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(1.0))
                                .bg(Theme::global().PANEL_BORDER),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .justify_end()
                                // Folder mode uses the tabs' upper contour as the thumb.
                                // Reserve the same header geometry in both modes.
                                .child(self.render_sidebar_navigation_scrollbar(cx))
                                .child(
                                    div()
                                        .id("sidebar-navigation-tabs")
                                        .debug_selector(|| "sidebar-navigation-tabs".into())
                                        // Clicks here select tabs, not the Windows caption drag.
                                        .occlude()
                                        .flex_none()
                                        .h(px(34.0))
                                        .min_w_0()
                                        .flex()
                                        .items_end()
                                        .gap_1()
                                        .overflow_x_scroll()
                                        .track_scroll(&self.sidebar_navigation_scroll)
                                        .on_scroll_wheel(cx.listener(
                                            |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                                let delta =
                                                    event.delta.pixel_delta(window.line_height());
                                                let dx = if delta.x != px(0.0) {
                                                    delta.x
                                                } else {
                                                    delta.y
                                                };
                                                let handle = &this.sidebar_navigation_scroll;
                                                let x = (handle.offset().x + dx)
                                                    .clamp(-handle.max_offset().x, px(0.0));
                                                handle.set_offset(gpui::point(x, px(0.0)));
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ))
                                        .child(
                                            div()
                                                .id("sidebar-sessions-tab")
                                                .debug_selector(|| "sidebar-sessions-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Sessions,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Sessions {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Sessions;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("chat"),
                                        )
                                        .child(self.render_tutorial_tab(cx))
                                        .child(
                                            div()
                                                .id("sidebar-files-tab")
                                                .debug_selector(|| "sidebar-files-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Files,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Files {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Files;
                                                        let mut tree = this.file_tree.borrow_mut();
                                                        tree.rows.clear();
                                                        tree.revision += 1;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("files"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-accounts-tab")
                                                .debug_selector(|| "sidebar-accounts-tab".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .when(
                                                    self.sidebar_view == SidebarView::Accounts,
                                                    |el| {
                                                        el.h(px(34.0))
                                                            .border_b_0()
                                                            .bg(Theme::global().HEADER_BG)
                                                    },
                                                )
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(
                                                    if self.sidebar_view == SidebarView::Accounts {
                                                        Theme::global().TEXT
                                                    } else {
                                                        Theme::global().TEXT_DIM
                                                    },
                                                )
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.sidebar_view = SidebarView::Accounts;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child("accounts"),
                                        )
                                        .child(self.render_preference_tab(
                                            SidebarView::Theme,
                                            "theme",
                                            cx,
                                        ))
                                        .child(self.render_preference_tab(
                                            SidebarView::Settings,
                                            "settings",
                                            cx,
                                        ))
                                        .child(self.render_sidebar_search_button(cx))
                                        .child(
                                            div()
                                                .id("sidebar-openrig")
                                                .px_2()
                                                .py_1()
                                                .rounded_full()
                                                .cursor_pointer()
                                                .text_size(px(11.))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| el.bg(Theme::global().HEADER_BG))
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_openrig(&OpenOpenrig, window, cx)
                                                    }),
                                                )
                                                .child("OpenRig"),
                                        )
                                        .child(
                                            crate::keyboard::press_button(
                                                div().id("sidebar-orchestration"),
                                                {
                                                    let owner = cx.weak_entity();
                                                    move |window, cx| {
                                                        let _ = owner.update(cx, |this, cx| {
                                                            this.open_orchestration(
                                                                &OpenOrchestration,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                },
                                            )
                                            .debug_selector(|| "sidebar-orchestration".into())
                                            .flex_none()
                                            .border_1()
                                            .border_color(Theme::global().PANEL_BORDER)
                                            .px_2()
                                            .py_1()
                                            .rounded_t_md()
                                            .h(px(30.0))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .text_size(px(11.0))
                                            .text_color(Theme::global().TEXT_DIM)
                                            .hover(|el| {
                                                el.bg(Theme::global().HEADER_BG)
                                                    .text_color(Theme::global().TEXT)
                                            })
                                            .child("orchestration"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-unfinished-work")
                                                .debug_selector(|| "sidebar-unfinished-work".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _event, window, cx| {
                                                        this.new_unfinished_work(
                                                            &NewUnfinishedWork,
                                                            window,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("todos"),
                                        )
                                        .child(
                                            div()
                                                .id("open-todoist")
                                                .debug_selector(|| "open-todoist".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_todoist(&OpenTodoist, window, cx)
                                                    }),
                                                )
                                                .child("todoist"),
                                        )
                                        .child(
                                            div()
                                                .id("open-gmail")
                                                .debug_selector(|| "open-gmail".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _, window, cx| {
                                                        this.open_gmail(&OpenGmail, window, cx)
                                                    }),
                                                )
                                                .child("email"),
                                        )
                                        .child(
                                            div()
                                                .id("sidebar-open-folder")
                                                .debug_selector(|| "sidebar-open-folder".into())
                                                .flex_none()
                                                .border_1()
                                                .border_color(Theme::global().PANEL_BORDER)
                                                .px_2()
                                                .py_1()
                                                .rounded_t_md()
                                                .h(px(30.0))
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(11.0))
                                                .text_color(Theme::global().TEXT_DIM)
                                                .hover(|el| {
                                                    el.bg(Theme::global().HEADER_BG)
                                                        .text_color(Theme::global().TEXT)
                                                })
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(|this, _event, window, cx| {
                                                        this.open_folder(&OpenFolder, window, cx);
                                                    }),
                                                )
                                                .child("folder"),
                                        )
                                        .child(
                                            crate::keyboard::press_button(
                                                div().id("sidebar-new-session"),
                                                {
                                                    let owner = cx.weak_entity();
                                                    move |_, cx| {
                                                        let _ = owner.update(cx, |this, cx| {
                                                            this.missed("new_panel", cx);
                                                            this.open_new_session(cx);
                                                        });
                                                    }
                                                },
                                            )
                                            // Tagged so a render test can click the real button
                                            // and confirm it counts as a slow path, not as
                                            // knowledge of super-enter.
                                            .debug_selector(|| "sidebar-new-session".into())
                                            .flex_none()
                                            .border_1()
                                            .border_color(Theme::global().PANEL_BORDER)
                                            .px_2()
                                            .py_1()
                                            .rounded_t_md()
                                            .h(px(30.0))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .text_size(px(16.0))
                                            .text_color(Theme::global().TEXT_DIM)
                                            .hover(|el| {
                                                el.bg(Theme::global().HEADER_BG)
                                                    .text_color(Theme::global().TEXT)
                                            })
                                            .child("+"),
                                        ),
                                ),
                        )
                        .into_any_element()
                };
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(div().flex_1().min_w_0().child(navigation))
            })
            // Folder mode's header already is the destination row.
            .children((!folders).then(|| self.render_machine_switcher(None, cx)))
            .children(
                (self.sidebar_view == SidebarView::Sessions)
                    .then(|| self.session_catalog_controls("sidebar", cx).px_2().py_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .relative()
                    .debug_selector(|| "sidebar-tab-body".into())
                    .when(self.sidebar_view == SidebarView::Files, |el| {
                        el.pr(px(crate::scrollbar::GUTTER))
                    })
                    .child(match self.sidebar_view {
                        SidebarView::Sessions => list.into_any_element(),
                        SidebarView::Learn => self.render_tutorial_guides(cx),
                        SidebarView::Files => self.render_files_sidebar(cx),
                        SidebarView::Theme => self.render_theme_settings(cx),
                        SidebarView::Settings => self.render_settings(cx),
                        SidebarView::Accounts => self.render_accounts(cx).unwrap_or_else(|| {
                            div()
                                .debug_selector(|| "accounts-empty".into())
                                .p_4()
                                .text_size(px(11.0))
                                .text_color(Theme::global().TEXT_DIM)
                                .child("No connected accounts")
                                .into_any_element()
                        }),
                    })
                    .when(
                        matches!(
                            self.sidebar_view,
                            SidebarView::Sessions | SidebarView::Files
                        ),
                        |el| el.child(self.render_sidebar_scrollbar(cx)),
                    ),
            )
            .child(self.render_sidebar_account(cx))
            .into_any_element()
    }
}
