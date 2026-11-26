//! Building a workspace: the real window and the test fixture.
use super::*;

impl Workspace {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        host: HostHandle,
        snapshot: Option<WorkspaceSnapshot>,
        crash_launch: Option<jcode_desktop_api::WindowLaunch>,
    ) -> Self {
        crate::init_harness();
        // Launch work that must not wait for a pane: a packaged build whose
        // release state is still `Unknown` shows no update badge, so a
        // render-triggered release check would never start the automatic update
        // loop for a user who stays in the session view.
        updates::ensure_release_check();
        #[cfg(target_os = "macos")]
        super::runtime_updates::install_update_dialog_host::<Self>(cx);
        // Read the account file here so the first frame needs no disk access.
        jcode_account::JcodeAccount::prime();
        let bridge = harness::spawn();
        let accounts_feed = accounts::spawn();
        // Offline fixtures arrive at once, so the first frame already lists
        // them instead of waiting for the housekeeping tick.
        let initial_accounts = if harness::screenshot_mode() {
            accounts_feed.latest().unwrap_or_default()
        } else {
            Vec::new()
        };
        let routing_feed = crate::routing::spawn();
        let recovering_crash = crash_launch.is_some();
        let launch = crash_launch
            .or_else(|| snapshot.as_ref()?.launch.clone().map(Into::into))
            .unwrap_or_else(|| {
                jcode_desktop_api::WindowLaunches::get(
                    cx,
                    window.window_handle().window_id().as_u64(),
                )
            });
        let performance_enabled = crate::performance::enabled(std::env::args_os());
        let resume_snapshot = snapshot.as_ref().map(|snapshot| snapshot.resume.clone());
        let single_panel = jcode_desktop_api::LaunchMode::from_args(&launch.args)
            == jcode_desktop_api::LaunchMode::SinglePanel;

        // Wake immediately when a bridge update arrives rather than polling an
        // empty channel at the display refresh rate.
        let update_bridge = bridge.clone();
        let bridge_task = cx.spawn(async move |this, cx| {
            while let Some(first) = update_bridge.recv().await {
                let updates = update_bridge.extend_batch(vec![first], 127);
                let alive = cx.update(|cx| {
                    let Some(workspace) = this.upgrade() else {
                        return false;
                    };
                    Self::dispatch_bridge_batch(&workspace, updates, cx);
                    true
                });
                if !alive {
                    break;
                }
                // recv() is immediately ready while a producer has a backlog.
                // Bound each batch and yield to input/layout between batches,
                // without reintroducing idle polling or dropping stream events.
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            }
        });

        let housekeeping_bridge = bridge.clone();
        let session_refresh_interval = crate::config::get().session_refresh_interval();
        let window_handle = window.window_handle();
        let (focus_wake, focus_changes) = async_channel::bounded(1);
        let initially_focused = window.is_window_active();
        if initially_focused {
            housekeeping_bridge.send(Command::SetCatalogForeground(true));
        }
        // Streaming markers are local files, so only poll them for a real
        // local daemon, never in tests or offline screenshot fixtures.
        let poll_daemon_running = !cfg!(test) && !harness::screenshot_mode();
        // Also applied by the host at startup. Repeating it here lets a
        // hot-reloaded UI fix an older, still-running host process.
        crate::memory::disable_transparent_huge_pages();
        let housekeeping_task = cx.spawn(async move |this, cx| {
            let mut catalog = housekeeping::CatalogRefresh::new(Instant::now(), session_refresh_interval);
            let mut focused = initially_focused;
            let mut last_update_state = updates::current();
            let mut last_release_state = updates::release_status();
            let mut last_check_visible = updates::user_check_visible();
            loop {
                let delay = if focused { housekeeping::FOREGROUND_TICK } else { housekeeping::BACKGROUND_TICK };
                let activated = futures_lite::future::race(
                    async { focus_changes.recv().await.is_ok() },
                    async { cx.background_executor().timer(delay).await; false },
                ).await;
                let Ok(is_active) = window_handle.update(cx, |_, window, _| window.is_window_active()) else {
                    break;
                };
                focused = is_active;
                if crate::memory::claim_trim_slot() {
                    cx.background_executor()
                        .spawn(async { crate::memory::trim() })
                        .detach();
                }
                let update_state = updates::current();
                let release_state = updates::release_status();
                let check_visible = updates::user_check_visible();
                if update_state != last_update_state
                    || release_state != last_release_state
                    || check_visible != last_check_visible
                {
                    last_update_state = update_state;
                    last_release_state = release_state;
                    last_check_visible = check_visible;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
                if focused && poll_daemon_running {
                    let running = cx
                        .background_executor()
                        .spawn(async { daemon_running_sessions() })
                        .await;
                    if this
                        .update(cx, |workspace: &mut Workspace, cx| {
                            workspace.set_daemon_running(running, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                if let Some(view) = routing_feed.latest() {
                    let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                        if workspace.routing_view.as_ref() != Some(&view) {
                            workspace.routing_view = Some(view);
                            workspace.accounts_layout_pending = true;
                            cx.notify();
                        }
                    });
                    if outcome.is_err() {
                        break;
                    }
                }
                let accounts = accounts_feed.latest();
                let picker_open = focused && this.read_with(cx, |w, _| w.resume.is_some()).unwrap_or(false);
                if catalog.due(Instant::now(), focused, activated, picker_open) {
                    housekeeping_bridge.send(Command::RefreshSessions);
                }
                let Some(accounts) = accounts else {
                    continue;
                };
                let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                    if workspace.accounts != accounts {
                        cx.set_global(crate::account_status::StatusAccounts(accounts.clone()));
                        for slot in &workspace.slots {
                            slot.panel.update_chat(cx, |_, cx| cx.notify());
                        }
                        workspace.accounts = accounts;
                        workspace.accounts_layout_pending = true;
                        cx.notify();
                    }
                });
                if outcome.is_err() {
                    break;
                }
            }
        });

        let performance_task = cx.spawn(async move |this, cx| {
            if !performance_enabled {
                return;
            }
            const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
            const DISPLAY_INTERVAL: Duration = Duration::from_millis(250);
            let mut last_wake = Instant::now();
            let mut last_display = Instant::now();
            loop {
                cx.background_executor().timer(SAMPLE_INTERVAL).await;
                let now = Instant::now();
                let wake_lag = now
                    .duration_since(last_wake)
                    .saturating_sub(SAMPLE_INTERVAL);
                last_wake = now;
                let refresh_display = last_display.elapsed() >= DISPLAY_INTERVAL;
                if refresh_display {
                    last_display = now;
                }
                let outcome = this.update(cx, |workspace: &mut Workspace, cx| {
                    if let Some(profile) = workspace.performance.as_mut() {
                        profile.observe_wake_lag(wake_lag);
                        if refresh_display {
                            cx.notify();
                        }
                    }
                });
                if outcome.is_err() {
                    break;
                }
            }
        });

        let _ = window;
        let mut workspace = Self {
            window_background: None,
            global_voice: Default::default(),
            voice_key: Default::default(),
            last_voice_chat: None,
            _voice_activation: Some(cx.observe_window_activation(window, move |this, window, cx| {
                let active = window.is_window_active();
                let _ = focus_wake.try_send(());
                crate::openrig_feed::window_activation_changed(cx);
                this.bridge.send(Command::SetCatalogForeground(active));
                this.global_voice.set_active(active);
                if !active {
                    if this.voice_key.is_down() {
                        this.end_voice_hold(&EndVoiceHold, window, cx);
                    }
                    this.voice_key = Default::default();
                }
            })),
            #[cfg(unix)]
            desktop_control: None,
            #[cfg(unix)]
            desktop_control_task: None,
            control_drafts: HashMap::new(),
            preview_control: None,
            preview_task: None,
            side_panel_snapshots: HashMap::new(),
            applet_toast: None,
            applet_consent_dismissed: HashSet::new(),
            applet_text_selection: None,
            sidebar_applets: None,
            sidebar_chrome: None,
            sidebar_panel_metadata: None,
            panel_metadata_observers: HashMap::new(),
            bridge,
            host,
            single_panel,
            launch: launch.clone(),
            resume: None,
            account_sign_in: account_sign_in::State::startup(),
            account_sign_in_visible: account_sign_in::offered_at_startup(),
            account_menu: account_menu::State::startup(),
            show_minimap: false,
            layout_mode: crate::config::get().appearance.layout_mode,
            folder_frame: Default::default(),
            sidebar_custom_width: SIDEBAR_WIDTH,
            sidebar_resizing: false,
            show_sidebar: sidebar_enabled(&launch.args, crate::config::get().workspace.sidebar),
            compact_sidebar_open: false,
            compact_sidebar_motion: responsive::VisibilityMotion::default(),
            last_canvas_width: None,
            // Offline captures can open on the accounts list directly.
            sidebar_view: if crate::harness::screenshot_mode()
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_SIDEBAR").as_deref() == Ok("accounts")
            {
                SidebarView::Accounts
            } else {
                SidebarView::Sessions
            },
            session_catalog: Default::default(),
            worktrees: sidebar_worktrees::State::default(),
            collections: Default::default(),
            collection_menu: Default::default(),
            pending_collection_drafts: Vec::new(),
            tutorial_page: 0,
            onboarding_launch: screens::LaunchState::default(),
            expanded_directories: HashSet::new(),
            file_tree: Default::default(),
            files_pane: Default::default(),
            expanded_lines: HashSet::new(),
            slots: Vec::new(),
            active: 0,
            active_row: 0,
            row_focus: [None; STRIP_COUNT],
            outgoing_row: None,
            row_origin: 0.0,
            row_progress: AnimatedValue::new(1.0, transition::policy(Transition::Row).duration),
            workspace_pull: workspace_pull::PullPreview::default(),
            previous: None,
            camera_x: [0.0; STRIP_COUNT],
            camera_target: [0.0; STRIP_COUNT],
            camera_from: [0.0; STRIP_COUNT],
            camera_started: [None; STRIP_COUNT],
            camera_touch_pan: [false; STRIP_COUNT],
            camera_dirty: [true; STRIP_COUNT],
            camera_snap_pending: [false; STRIP_COUNT],
            overview: false,
            map_gesture: Default::default(),
            overview_progress: AnimatedValue::new(
                0.0,
                transition::policy(Transition::Overview).duration,
            ),
            hints_overlay: false,
            hints_progress: AnimatedValue::new(0.0, transition::policy(Transition::Hints).duration),
            showcase_mode: crate::config::get().workspace.showcase_keys,
            showcase_cue: None,
            showcase_task: None,
            coach: learning::load(),
            learning_persistence: Some(learning::Persistence::spawn()),
            coach_progress: AnimatedValue::new(0.0, transition::policy(Transition::Coach).duration),
            coach_expiry_task: None,
            coach_display_hint: None,
            sessions: Vec::new(),
            worker_meta: HashMap::new(),
            live_swarm_status: Default::default(),
            sidebar_closed_sessions: HashSet::new(),
            daemon_running: HashMap::new(),
            pinned_working_dir: crate::config::get().workspace.pinned_working_dir.clone(),
            remotes: machines::Machines::from_config(),
            accounts: initial_accounts,
            routing_view: None,
            recent_accounts: Vec::new(),
            accounts_scroll: ScrollHandle::new(),
            accounts_scroll_remainder: 0.0,
            accounts_layout_pending: true,
            status: "starting...".into(),
            connected: false,
            focus_handle: cx.focus_handle().tab_stop(true),
            _navigation_fallback: Self::install_navigation_fallback(cx),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_sessions_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(100.0)),
            sidebar_session_layout: Vec::new(),
            sidebar_projects: Default::default(),
            sidebar_selection: Default::default(),
            sidebar_gesture: None,
            sidebar_hovered_session: None,
            sidebar_todo_progress: Default::default(),
            catalog_tasks: Default::default(),
            unfinished_sessions: Vec::new(),
            sidebar_title_pulses: Default::default(),
            swarm_marks: Default::default(),
            sidebar_navigation_scroll: ScrollHandle::new(),
            live_tabs: live_tabs::TabMotion::default(),
            sidebar_roller: sidebar_roller::Roller::default(),
            focus_pending: false,
            focus_synced_active: None,
            gesture_last: None,
            gesture: StripGesture::default(),
            gesture_seen: None,
            scroll_snap: Default::default(),
            default_directory_picker: None,
            voice_settings: Default::default(),
            modal_layer: Self::new_modal_layer(cx),
            focus_restore: FocusSnapshot::Workspace,
            performance: performance_enabled.then(PerformanceProfile::default),
            gpui_performance: GpuiPerformanceSnapshot::default(),
            fps_counter: crate::fps_counter::FpsCounter::default(),
            action_capture: ActionCapture::from_env(),
            animation_tick_task: None,
            _bridge_task: bridge_task,
            _housekeeping_task: housekeeping_task,
            _stream_fixture: None,
            queued_fixture: None,
            _performance_task: performance_task,
            _live_profile_task: crate::live_profile::spawn(window, cx),
        };
        if snapshot.is_none()
            && !single_panel
            && !(harness::screenshot_mode()
                && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_HIDE_BETA").is_some())
        {
            workspace.show_beta_notice(window, cx);
        }
        if let Some(snapshot) = snapshot {
            workspace.apply_snapshot(snapshot, cx);
        } else if harness::screenshot_mode()
            && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_CLOUD_PICKER").is_some()
        {
            workspace.show_cloud_picker_fixture(cx);
        } else if harness::screenshot_mode()
            && let Some(variant) = std::env::var_os("JCODE_DESKTOP_SCREENSHOT_MACHINES")
        {
            workspace.show_machines_fixture(variant == "paired", cx);
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_PREVIEW_STATE").is_ok()
        {
            if let Ok(state) = std::env::var("JCODE_DESKTOP_SCREENSHOT_PREVIEW_STATE")
                .unwrap()
                .parse()
            {
                workspace.open_preview_unchecked(state, cx);
            }
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_CLOUD_STARTUP").is_ok()
        {
            workspace.remotes.default_host = Some(machines::MANAGED_CLOUD_HOST.into());
            workspace.open_startup_draft(cx);
            workspace.start_default_startup(cx);
            // Offline fixture: show real managed-cloud phases, never contact the service.
            let phases: &[&str] = if std::env::var("JCODE_DESKTOP_SCREENSHOT_CLOUD_STARTUP")
                .as_deref()
                == Ok("failed")
            {
                &[
                    machines::MANAGED_CLOUD_STARTING,
                    "Jcode Cloud requires an active paid Jcode subscription.",
                ]
            } else {
                &[
                    machines::MANAGED_CLOUD_STARTING,
                    "Creating your Jcode Cloud machine…",
                    "Installing Jcode on your cloud machine…",
                ]
            };
            for (index, message) in phases.iter().enumerate() {
                let failed = index + 1 == phases.len() && phases.len() == 2;
                workspace.update_pending_remote_status(
                    Some(Panel::STARTUP_SESSION_ID),
                    message,
                    failed,
                    cx,
                );
            }
        } else if harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_PENDING").as_deref() == Ok("1")
        {
            workspace.remotes.default_host = Some("offline-remote".into());
            workspace.open_startup_draft(cx);
            workspace.start_default_startup(cx);
            workspace.update_startup_status(
                "offline-remote: connection failed (offline fixture)",
                true,
                cx,
            );
        } else if harness::controlled_runtime_socket().is_some() {
            if let Ok(session_id) = std::env::var("JCODE_DESKTOP_CONTROL_RUNTIME_SESSION")
                && let Ok(session) =
                    serde_json::from_value::<jcode_sdk::SessionInfo>(serde_json::json!({
                        "session_id": session_id,
                        "status": "idle",
                    }))
            {
                workspace.sessions = vec![session.clone()];
                workspace.active = workspace.open_session(session, cx);
            }
        } else if crate::fixture_control_targets::draft_session() {
            workspace.connected = true;
            workspace.open_local_draft(workspace.default_working_dir(), cx);
        } else if harness::screenshot_mode() {
            let worktree_fixture = crate::fixture_control_targets::enabled()
                .then(|| std::env::var("JCODE_DESKTOP_CONTROL_SWARM_WORKTREE").ok())
                .flatten();
            let file_mentions = std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT")
                .as_deref()
                == Ok("file-mentions");
            let session = jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: "screenshot-fixture".into(),
                // Keep this longer than the sidebar to exercise single-line ellipsis.
                title: Some("Review markdown rendering with a very long session title".into()),
                working_dir: (worktree_fixture.is_some()
                    || crate::fixture_control_targets::files_panel()
                    || file_mentions)
                    .then(|| std::env::var("JCODE_DESKTOP_WORKING_DIR").ok())
                    .flatten()
                    .or_else(|| Some("/workspace/example".into())),
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
                context: None,
                edit_stats: Some(jcode_sdk::SessionEditStats {
                    added: 128,
                    removed: 37,
                    approximate: false,
                }),
            };
            workspace.connected = true;
            workspace.sessions = vec![session.clone()];
            workspace.active = workspace.open_session(session, cx);
            workspace.init_worktree_fixture(cx);
            workspace.init_collections_fixture();
            if crate::fixture_control_targets::enabled()
                && let Ok(session_id) = std::env::var("JCODE_DESKTOP_CONTROL_TODO_SESSION")
                && let Some(home) = harness::jcode_home()
            {
                let mut session = workspace.sessions[0].clone();
                session.session_id = session_id;
                session.title = Some("Verify a background project".into());
                session.working_dir = Some("/workspace/side-project".into());
                session.transcript_bytes = Some(65_600_000);
                session.user_connected = true;
                session.context = Some(jcode_sdk::api::SessionCatalogContext {
                    activity: Some(jcode_sdk::api::SessionCatalogActivity {
                        origin: jcode_sdk::api::SessionOrigin::Agent,
                        last_agent_activity_at_ms: Some(42),
                        last_message_at_ms: Some(Some(42)),
                    }),
                    ..Default::default()
                });
                workspace.sidebar_todo_progress =
                    harness::todo_progress_in(&home, std::slice::from_ref(&session));
                workspace.sessions.push(session);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_SWARM").as_deref() == Ok("real") {
                workspace.load_real_swarm_fixture(cx);
            } else if std::env::var_os("JCODE_DESKTOP_SCREENSHOT_SWARM").is_some() {
                for (index, (label, status, model, provider, effort)) in [
                    (
                        "API reviewer",
                        "working",
                        "claude-opus-5-5",
                        "claude-oauth",
                        "high",
                    ),
                    (
                        "Sidebar implementation",
                        "working",
                        "gpt-5.6",
                        "openai",
                        "low",
                    ),
                    (
                        "Test runner",
                        "completed",
                        "deepseek-v4.1-flash",
                        "opencode-go",
                        "max",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut child = workspace.sessions[0].clone();
                    child.session_id = format!("swarm-fixture-{index}");
                    child.title = None;
                    child.agent_label = Some(label.into());
                    if index == 0
                        && let Some(directory) = &worktree_fixture
                    {
                        child.working_dir = Some(directory.clone());
                        // Older daemons supplied the seeded todo title in the
                        // catalog but no live agent label for auto-lanes.
                        child.title = Some("btw-enter".into());
                        child.agent_label = None;
                    }
                    child.parent_session_id = Some("screenshot-fixture".into());
                    child.swarm_status = Some(status.into());
                    if index == 2 && worktree_fixture.is_some() {
                        child.title = Some("Retry after the previous worker failed".into());
                    }
                    child.edit_stats = Some(jcode_sdk::SessionEditStats {
                        added: 14 + index as u64 * 50,
                        removed: 2 + index as u64 * 8,
                        approximate: index == 1,
                    });
                    // The rows draw each worker's model and its reasoning rung,
                    // so the fixture must carry a record or the screenshot
                    // shows a bare name and proves nothing about the dots.
                    workspace.worker_meta.insert(
                        child.session_id.clone(),
                        jcode_sdk::WorkerMeta {
                            model: Some(model.into()),
                            provider: Some(provider.into()),
                            effort: Some(effort.into()),
                            input_tokens: Some(12_400 + index as u64 * 3_000),
                            output_tokens: Some(3_100 + index as u64 * 800),
                            ..jcode_sdk::WorkerMeta::default()
                        },
                    );
                    workspace.sessions.push(child);
                }
            }
            if worktree_fixture.is_some() {
                workspace.sync_swarm_strips(cx);
            }
            if let Ok(effort) = std::env::var("JCODE_DESKTOP_SCREENSHOT_BAR") {
                // Hand the swarm fixture's workers to the bar now; the strips
                // otherwise sync on the first session-list update.
                workspace.sync_swarm_strips(cx);
                if let Some(slot) = workspace.slots.first() {
                    slot.panel.update_chat(cx, |panel, cx| {
                        panel.apply_bar_fixture(&effort, cx);
                        if crate::fixture_control_targets::enabled() {
                            match std::env::var("JCODE_DESKTOP_SCREENSHOT_CACHE_CARD").as_deref() {
                                Ok("1") => panel.apply_cache_card_fixture(false, cx),
                                Ok("warning") => panel.apply_cache_card_fixture(true, cx),
                                _ => {}
                            }
                            // Builds on the fixture above, so the cache card
                            // and the chip can be reviewed after a switch.
                            if let Ok(stage) = std::env::var("JCODE_DESKTOP_SCREENSHOT_MODEL_SWITCH") {
                                panel.apply_model_switch_fixture(&stage, cx);
                            }
                        }
                    });
                }
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("publish") {
                workspace.open_publish_fixture(cx);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("applet") {
                workspace.open_applet_fixture(cx);
            }
            let panel_count = std::env::var("JCODE_DESKTOP_SCREENSHOT_PANELS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 12);
            let panel_count = if single_panel { 1 } else { panel_count };
            let panel_titles = std::env::var("JCODE_DESKTOP_SCREENSHOT_PANEL_TITLES").ok();
            let panel_title = |index: usize| {
                panel_titles
                    .as_deref()
                    .and_then(|titles| titles.split(',').nth(index))
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_owned)
            };
            if let Some(title) = panel_title(0) {
                workspace.sessions[0].title = Some(title.clone());
                if let Some(slot) = workspace.slots.first() {
                    slot.panel
                        .update_chat(cx, |panel, cx| panel.set_title(title, cx));
                }
            }
            for index in 1..panel_count {
                let mut session = workspace.sessions[0].clone();
                session.session_id = format!("screenshot-fixture-{index}");
                session.title = Some(
                    panel_title(index).unwrap_or_else(|| format!("Review folder {}", index + 1)),
                );
                workspace.sessions.push(session.clone());
                workspace.active = workspace.open_session(session, cx);
            }
            if panel_count > 1
                || std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("openrig")
            {
                for slot in &mut workspace.slots {
                    slot.width_fraction = 1.0 / panel_count as f32;
                    slot.animated_width = AnimatedValue::new(
                        slot.width_fraction,
                        transition::policy(Transition::PanelWidth).duration,
                    );
                }
                workspace.active = panel_count / 2;
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_TAB_STATUSES").as_deref() == Ok("1") {
                workspace.seed_tab_status_fixture(cx);
            }
            if let Some(path) = std::env::var_os("JCODE_DESKTOP_SCREENSHOT_MARKDOWN_FILE") {
                let file = cx.new(|cx| FilePanel::new(PathBuf::from(path), cx));
                workspace.slots[workspace.active].panel = PanelItem::File(file);
            }
            workspace.focus_pending = true;
            if workspace_stream_fixture::enabled() {
                workspace._stream_fixture = Some(workspace_stream_fixture::spawn(
                    "screenshot-fixture".into(),
                    cx,
                    Self::apply,
                ));
            }
            if let Some(count) = std::env::var("JCODE_DESKTOP_SCREENSHOT_QUEUED")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|count| *count > 0)
            {
                // Re-seed on a timer: the offline demo turn ends, and its
                // turn_done drains the queue, so a one-shot seed would show the
                // rows for a frame and then hand them to the transcript.
                // `JCODE_DESKTOP_SCREENSHOT_QUEUE_EDIT=1` also opens the first
                // row in its inline editor, with a screenshot and files.
                let queue_edit =
                    std::env::var("JCODE_DESKTOP_SCREENSHOT_QUEUE_EDIT").as_deref() == Ok("1");
                workspace.queued_fixture = Some(cx.spawn(async move |this, cx| {
                    let executor = cx.background_executor().clone();
                    let mut round = 0usize;
                    loop {
                        let mut seeded = false;
                        this.update(cx, |workspace, cx| {
                            if let Some(slot) = workspace.slots.first() {
                                slot.panel.update_chat(cx, |panel, cx| {
                                    if !panel.has_visible_queued_prompts() {
                                        panel.seed_screenshot_queued(count, round);
                                        if queue_edit {
                                            panel.seed_screenshot_queue_edit(cx);
                                        }
                                        seeded = true;
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .ok();
                        if seeded {
                            round += 1;
                        }
                        executor.timer(Duration::from_millis(1500)).await;
                    }
                }));
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_SIDEBAR").as_deref() == Ok("accounts") {
                workspace.sidebar_view = SidebarView::Accounts;
                // The routing section below the logins: labelled API keys with
                // their icons, and budgets.
                workspace.routing_view = Some(crate::routing::fixture());
                // Past the end: the list clamps it, so the routing rows show.
                workspace.accounts_scroll.set_offset(gpui::point(px(0.0), px(-4000.0)));
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_ACCOUNTS").as_deref() == Ok("1")
                && let Some(slot) = workspace.slots.first()
            {
                let session = slot.panel.session_id(cx).to_owned();
                let bridge = workspace.bridge.clone();
                let panel = cx.new(|cx| Panel::new_accounts(&session, None, bridge, cx));
                workspace.slots[0].panel = PanelItem::Chat(panel);
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_HISTORY").as_deref() == Ok("1") {
                for index in 0..80 {
                    let mut session = workspace.sessions[0].clone();
                    session.session_id = format!("screenshot-history-{index:02}");
                    session.title = Some(format!("History session {index:02}"));
                    workspace.sessions.push(session);
                }
            }
            if std::env::var("JCODE_DESKTOP_SCREENSHOT_NOTIFICATION").as_deref() == Ok("1") {
                workspace.showcase_cue = Some(ShowcaseCue {
                    shortcut: "Super + Shift + S".into(),
                    action: "Showcase mode on",
                    tutorial_group: "",
                    started_at: None,
                });
            }
            if let Some(stage) = std::env::var("JCODE_DESKTOP_SCREENSHOT_LEARN_STAGE")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|stage| (1..=3).contains(stage))
            {
                workspace.sidebar_view = SidebarView::Learn;
                workspace.tutorial_page = stage - 1;
            }
        } else {
            workspace.open_startup_draft(cx);
        }
        workspace.finish_launch(resume_snapshot, recovering_crash, window, cx);
        if workspace.resume.is_none() && workspace.remotes.default_host.is_some() {
            workspace.start_default_startup(cx);
        }
        workspace.start_preview_control(cx);
        #[cfg(unix)]
        workspace.start_desktop_control(window, cx);
        workspace.start_global_voice(window, cx);
        workspace.detect_account_imports(cx);
        workspace.load_collections(cx);
        workspace.open_collections_fixture_form(window, cx);
        workspace
    }

    pub(super) fn finish_launch(
        &mut self,
        resume_snapshot: Option<screens::Snapshot>,
        recovering_crash: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use jcode_desktop_api::LaunchMode;
        let apply_launch = resume_snapshot.is_none() || recovering_crash;
        let resume_requested = LaunchMode::resume_requested(&self.launch.args);
        // A crash restores tabs, then applies this launch's arguments. Clean
        // quit, restart manifests and in-process reloads keep their saved UI.
        if recovering_crash && resume_requested {
            self.open_resume(&OpenResume, window, cx);
        } else {
            self.restore_resume(resume_snapshot, resume_requested, window, cx);
        }
        if apply_launch && let Some(session_id) = LaunchMode::requested_session(&self.launch.args) {
            self.resume = None;
            self.open_requested_session(session_id, window, cx);
            self.focus_restore = FocusSnapshot::Workspace;
        }
    }

    pub(super) fn open_startup_draft(&mut self, cx: &mut Context<Self>) {
        self.active = self.open_session(
            jcode_sdk::SessionInfo {
                user_connected: false,
                session_id: Panel::STARTUP_SESSION_ID.into(),
                title: Some(self.remotes.default_host.as_ref().map_or_else(
                    || "New session".into(),
                    |host| format!("Connecting to {host}"),
                )),
                working_dir: self
                    .remotes
                    .default_host
                    .is_none()
                    .then(|| {
                        launch_directory_override(&self.launch)
                            .or_else(|| self.new_session_directory())
                    })
                    .flatten(),
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
        // The first frame is a full-sized editor, not a zero-width entrance
        // animation that makes typing appear to go nowhere.
        let slot = &mut self.slots[self.active];
        slot.animated_width = AnimatedValue::new(
            slot.width_fraction,
            transition::policy(Transition::PanelOpen).duration,
        );
        self.focus_pending = true;
    }

    #[cfg(test)]
    pub(crate) fn enable_test_minimap(&mut self) {
        self.show_minimap = true;
    }

    /// A workspace with no runtime and a caller-supplied coach, for tests that
    /// drive real keystrokes through the real keymap.
    #[cfg(test)]
    pub fn for_test(coach: learning::Coach, cx: &mut Context<Self>) -> Self {
        Self {
            window_background: None,
            global_voice: Default::default(),
            voice_key: Default::default(),
            last_voice_chat: None,
            _voice_activation: None,
            #[cfg(unix)]
            desktop_control: None,
            #[cfg(unix)]
            desktop_control_task: None,
            control_drafts: HashMap::new(),
            preview_control: None,
            preview_task: None,
            side_panel_snapshots: HashMap::new(),
            applet_toast: None,
            applet_consent_dismissed: HashSet::new(),
            applet_text_selection: None,
            sidebar_applets: None,
            sidebar_chrome: None,
            sidebar_panel_metadata: None,
            panel_metadata_observers: HashMap::new(),
            bridge: harness::spawn_inert(),
            host: HostHandle::inert(),
            single_panel: false,
            launch: Default::default(),
            resume: None,
            account_sign_in: account_sign_in::State::default(),
            account_sign_in_visible: false,
            account_menu: account_menu::State::default(),
            show_sidebar: true,
            sidebar_custom_width: SIDEBAR_WIDTH,
            sidebar_resizing: false,
            show_minimap: false,
            layout_mode: crate::config::LayoutMode::FolderTabs,
            folder_frame: Default::default(),
            compact_sidebar_open: false,
            compact_sidebar_motion: responsive::VisibilityMotion::default(),
            last_canvas_width: None,
            sidebar_view: SidebarView::Sessions,
            session_catalog: Default::default(),
            worktrees: sidebar_worktrees::State::default(),
            collections: Default::default(),
            collection_menu: Default::default(),
            pending_collection_drafts: Vec::new(),
            tutorial_page: 0,
            onboarding_launch: screens::LaunchState::default(),
            expanded_directories: HashSet::new(),
            file_tree: Default::default(),
            files_pane: Default::default(),
            expanded_lines: HashSet::new(),
            slots: Vec::new(),
            active: 0,
            active_row: 0,
            row_focus: [None; STRIP_COUNT],
            outgoing_row: None,
            row_origin: 0.0,
            row_progress: AnimatedValue::new(1.0, transition::policy(Transition::Row).duration),
            workspace_pull: workspace_pull::PullPreview::default(),
            previous: None,
            camera_x: [0.0; STRIP_COUNT],
            camera_target: [0.0; STRIP_COUNT],
            camera_from: [0.0; STRIP_COUNT],
            camera_started: [None; STRIP_COUNT],
            camera_touch_pan: [false; STRIP_COUNT],
            camera_dirty: [true; STRIP_COUNT],
            camera_snap_pending: [false; STRIP_COUNT],
            overview: false,
            map_gesture: Default::default(),
            overview_progress: AnimatedValue::new(
                0.0,
                transition::policy(Transition::Overview).duration,
            ),
            hints_overlay: false,
            hints_progress: AnimatedValue::new(0.0, transition::policy(Transition::Hints).duration),
            showcase_mode: true,
            showcase_cue: None,
            showcase_task: None,
            coach,
            learning_persistence: None,
            coach_progress: AnimatedValue::new(0.0, transition::policy(Transition::Coach).duration),
            coach_expiry_task: None,
            coach_display_hint: None,
            sessions: Vec::new(),
            worker_meta: HashMap::new(),
            live_swarm_status: Default::default(),
            sidebar_closed_sessions: HashSet::new(),
            daemon_running: HashMap::new(),
            pinned_working_dir: None,
            remotes: machines::Machines::default(),
            accounts: Vec::new(),
            routing_view: None,
            recent_accounts: Vec::new(),
            accounts_scroll: ScrollHandle::new(),
            accounts_scroll_remainder: 0.0,
            accounts_layout_pending: true,
            status: "test".into(),
            connected: true,
            focus_handle: cx.focus_handle().tab_stop(true),
            _navigation_fallback: Self::install_navigation_fallback(cx),
            sidebar_scroll: ScrollHandle::new(),
            sidebar_sessions_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(100.0)),
            sidebar_session_layout: Vec::new(),
            sidebar_projects: Default::default(),
            sidebar_selection: Default::default(),
            sidebar_gesture: None,
            sidebar_hovered_session: None,
            sidebar_todo_progress: Default::default(),
            catalog_tasks: Default::default(),
            unfinished_sessions: Vec::new(),
            sidebar_title_pulses: Default::default(),
            swarm_marks: Default::default(),
            sidebar_navigation_scroll: ScrollHandle::new(),
            live_tabs: live_tabs::TabMotion::default(),
            sidebar_roller: sidebar_roller::Roller::default(),
            focus_pending: false,
            focus_synced_active: None,
            gesture_last: None,
            gesture: StripGesture::default(),
            gesture_seen: None,
            scroll_snap: Default::default(),
            default_directory_picker: None,
            voice_settings: Default::default(),
            modal_layer: Self::new_modal_layer(cx),
            focus_restore: FocusSnapshot::Workspace,
            performance: None,
            gpui_performance: GpuiPerformanceSnapshot::default(),
            fps_counter: crate::fps_counter::FpsCounter::default(),
            action_capture: None,
            animation_tick_task: None,
            _bridge_task: cx.spawn(async move |_, _| {}),
            _housekeeping_task: cx.spawn(async move |_, _| {}),
            _stream_fixture: None,
            queued_fixture: None,
            _performance_task: cx.spawn(async move |_, _| {}),
            _live_profile_task: cx.spawn(async move |_, _| {}),
        }
    }

    /// Add a bare panel for tests, bypassing the runtime.
    #[cfg(test)]
    pub fn push_test_panel(&mut self, name: &str, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let panel =
            cx.new(|cx| Panel::new(name.to_string(), Some(name.to_string()), None, bridge, cx));
        Panel::connect_input(&panel, cx);
        self.slots.push(Slot {
            panel: PanelItem::Chat(panel),
            row: self.active_row,
            width_fraction: DEFAULT_WIDTH,
            animated_width: AnimatedValue::new(
                DEFAULT_WIDTH,
                transition::policy(Transition::PanelWidth).duration,
            ),
            order_offset: AnimatedValue::new(
                0.0,
                transition::policy(Transition::PanelOrder).duration,
            ),
            order_distance_fraction: DEFAULT_WIDTH,
            close_progress: AnimatedValue::new(
                1.0,
                transition::policy(Transition::PanelClose).duration,
            ),
            closing: false,
            restore_fraction: None,
        });
    }

    #[cfg(test)]
    pub fn set_test_bridge(&mut self, bridge: Bridge) {
        self.bridge = bridge;
    }

    /// Seed the routing view for tests, bypassing the runtime.
    #[cfg(test)]
    pub fn set_test_routing_view(&mut self, view: jcode_session_types::routing::RoutingView) {
        self.routing_view = Some(view);
    }

    /// Seed accounts for tests, bypassing the CLI.
    #[cfg(test)]
    pub fn set_test_accounts(&mut self, accounts: Vec<accounts::Account>) {
        self.accounts = accounts;
    }

    #[cfg(test)]
    pub fn test_coach(&self) -> &learning::Coach {
        &self.coach
    }

    #[cfg(test)]
    pub fn test_coach_mut(&mut self) -> &mut learning::Coach {
        &mut self.coach
    }

    /// The panel entity at `index`, so tests can drive real events through it.
    #[cfg(test)]
    pub fn test_panel(&self, index: usize) -> Option<Entity<Panel>> {
        self.slots
            .get(index)
            .and_then(|slot| slot.panel.chat().cloned())
    }

    #[cfg(test)]
    pub fn test_focus_position(&self) -> Option<usize> {
        self.row_indices(self.active_row)
            .position(|index| index == self.active)
    }
}
