//! Restoring what a reload keeps of a panel, and the account and unfinished-work
//! panels.
use super::*;

impl Panel {
    pub fn restore_snapshot(&mut self, snapshot: PanelSnapshot, cx: &mut Context<Self>) {
        if self.restore_side_document_snapshot(&snapshot, cx) {
            return;
        }
        if let Some(transcript) = snapshot.transcript {
            self.restore_transcript(transcript);
        } else {
            self.turn_outcome = snapshot.turn_outcome;
        }
        self.finished_unseen = snapshot.finished_unseen;
        self.clean_chat_override = snapshot.clean_chat_override;
        self.image_pane_open = snapshot.image_pane_open;
        self.image_pane_selected = None;
        self.prompt_queue = snapshot.prompt_queue;
        self.prompt_queue.replay_pending_on_connect = true;
        self.pending_ask_draft = snapshot.ask_draft;
        if let (Some(draft), Some(card)) = (&self.pending_ask_draft, &self.ask_card)
            && draft.matches_questions(&card.questions)
        {
            let draft = self.pending_ask_draft.take().expect("checked above");
            self.ask_card
                .as_mut()
                .expect("checked above")
                .restore_draft(draft, cx);
        }
        self.title = snapshot.title.into();
        self.working_dir = snapshot.working_dir;
        if self.is_pending_session() {
            self.draft_config = snapshot.draft_config;
            if let Some(model) = self.draft_config.model.clone() {
                self.pick_draft_model(&model, cx);
            }
            self.reasoning_effort.clone_from(&self.draft_config.effort);
            self.pinned_modes.clone_from(&self.draft_config.modes);
            self.sync_effort_menu(cx);
            self.sync_mode_menu(cx);
        }
        self.stick_to_bottom = snapshot.stick_to_bottom;
        self.startup_layout = snapshot.startup_layout;
        self.pending_history_scroll =
            (!snapshot.stick_to_bottom).then_some((snapshot.scroll_x, snapshot.scroll_y));
        self.transcript_list
            .set_offset_from_scrollbar(point(px(snapshot.scroll_x), px(snapshot.scroll_y)));
        self.input.update(cx, |input, cx| {
            input.restore(snapshot.draft, cx);
            input.set_working_dir(self.working_dir.as_deref());
        });
        self.schedule_ask_recovery(cx);
    }

    pub(crate) fn observe_draft_images(input: &Entity<PromptInput>, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        gpui::App::observe(cx, input, move |input, cx| {
            let has_images = input.read(cx).has_draft_images();
            if owner
                .read_with(cx, |panel, _| panel.draft_has_images != has_images)
                .unwrap_or(false)
            {
                let _ = owner.update(cx, |panel, cx| {
                    panel.draft_has_images = has_images;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn new_accounts(
        source_session: &str,
        preview: Option<PreviewState>,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            format!("accounts://{source_session}"),
            Some("accounts".into()),
            None,
            bridge,
            cx,
        );
        panel.items.clear();
        panel.preview_state = preview;
        panel.open_login_picker(cx);
        panel
    }

    pub(crate) fn choose_account_model(&mut self, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        self.open_recovery_models(cx);
    }

    pub fn new_unfinished_work(
        sessions: Vec<crate::harness::UnfinishedSession>,
        session_opener: SessionOpener,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            "unfinished-work".into(),
            Some("unfinished work".into()),
            None,
            bridge,
            cx,
        );
        panel.unfinished_work = Some(sessions);
        panel.unfinished_session_opener = Some(session_opener);
        panel
    }

    pub fn set_unfinished_work(
        &mut self,
        sessions: Vec<crate::harness::UnfinishedSession>,
        cx: &mut Context<Self>,
    ) {
        if self.unfinished_work.as_ref() != Some(&sessions) {
            self.unfinished_work = Some(sessions);
            cx.notify();
        }
    }

    pub(super) fn build(
        session_id: String,
        title: Option<String>,
        working_dir: Option<String>,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let rig_feed = crate::openrig_feed::get(cx);
        cx.observe(&rig_feed, |panel, _, cx| panel.refresh_rig_hud(cx))
            .detach();
        let rig_host_id = (crate::harness::screenshot_mode()
            && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RIG_HUD").is_some())
        .then(|| "host-fixture-local".to_owned());
        let rig_identity = rig_host_id.as_deref().and_then(|host| {
            rig_hud::identify(rig_feed.read(cx).fleet()?, &session_id, Some(host))
        });
        let send_bridge = bridge.clone();
        let send_session = session_id.clone();
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(
                cx,
                "Type something…",
                move |content, images, _window, _app| {
                    send_bridge.send(Command::Send {
                        session_id: send_session.clone(),
                        content,
                        images,
                        attachments: Vec::new(),
                    });
                },
            )
            .with_example_prompts();
            input.enable_context_picker();
            input
        });
        // Local echo is appended by the workspace when submit fires; simplest
        // is to observe our own input entity... but the closure above has no
        // panel access. Instead the workspace routes sends through the panel.
        Self::observe_draft_images(&input, cx);
        let display_title = title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| short_id(&session_id));
        let transcript_list = ListState::new(0, ListAlignment::Top, px(600.));
        let transcript_selection = cx.new(TextSelection::new_scoped);
        cx.observe(&transcript_selection, |panel, selection, cx| {
            let dragging = selection.read(cx).is_dragging();
            if dragging && !panel.transcript_dragging {
                panel.cancel_transcript_momentum();
                panel.stick_to_bottom = false;
                panel.release_startup_preview();
            }
            panel.transcript_dragging = dragging;
            panel.transcript_measurements.invalidate();
            cx.notify();
        })
        .detach();
        // The panel owns this ListState. A strong entity in its persistent
        // callback would keep the panel and its transcript alive after close
        // or hot reload, even when no window references it anymore.
        let panel_entity = cx.entity().downgrade();
        transcript_list.set_scroll_handler(move |event, _, cx| {
            let _ = panel_entity.update(cx, |panel, cx| {
                panel.release_startup_preview();
                if event.is_scrolled && !event.is_following_tail && event.visible_range.start == 0 {
                    panel.request_older_history();
                }
                let stick_to_bottom =
                    event.is_following_tail && !panel.transcript_selection.read(cx).is_dragging();
                if panel.stick_to_bottom != stick_to_bottom {
                    panel.stick_to_bottom = stick_to_bottom;
                    cx.notify();
                }
            });
        });
        let fixture = screenshot_fixture_for(&session_id);
        let tool_card_fixture = crate::harness::screenshot_mode()
            && matches!(fixture.as_deref(), Some("tool-cards" | "tool-cards-details"));
        let (mut fixture_tool_durations, mut fixture_tool_wall_times) = demo_tool_times(fixture.as_deref());
        let (items, fixture_codemode_summaries) = if tool_card_fixture {
            let history = codemode_card::screenshot_history();
            let codemode = codemode_card::history_summaries(&history).collect();
            for (id, (start, finish, duration)) in history_tool_times(&history) {
                fixture_tool_wall_times.insert(id.clone(), (start, finish));
                if let Some(duration) = duration {
                    fixture_tool_durations.insert(id, duration);
                }
            }
            (crate::panel::history_items::build(history, Vec::new()).0, codemode)
        } else {
            (demo_items(fixture.as_deref()), HashMap::new())
        };
        let items: Transcript = items.into();
        // `JCODE_DESKTOP_SCREENSHOT_EXPANDED=1` opens every disclosure in the
        // fixture, so the capture shows the expanded text rather than only the
        // collapsed rows.
        let expanded_regions =
            if tool_card_fixture && fixture.as_deref() == Some("tool-cards-details") {
                HashSet::from(["fixture-codemode-multi".into(), "codemode-calls:fixture-codemode-multi".into()])
            } else if crate::harness::screenshot_mode()
                && (expandable::Regions::screenshot_expanded() || fixture.as_deref() == Some("btw-clean-expanded"))
            {
                expandable::Regions::all(&items)
            } else {
                HashSet::new()
            };
        let streaming_fixture = crate::harness::screenshot_mode()
            && session_id == "screenshot-fixture"
            && matches!(
                std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref(),
                Ok("streaming" | "mermaid")
            );
        let usage_fixture =
            crate::harness::screenshot_mode() && fixture.as_deref() == Some("tokens");
        let effort_fixture = usage_fixture
            || (crate::fixture_control_targets::enabled()
                && fixture.as_deref() == Some("mode-picker"));
        let activity_status = if crate::harness::screenshot_mode() {
            match fixture.as_deref() {
                Some("orb-working" | "orb-todos") => "running",
                Some("orb-thinking") => "thinking",
                Some("orb-tools") => "running_tools",
                _ => "idle",
            }
        } else {
            "idle"
        };
        if crate::harness::screenshot_mode()
            && matches!(fixture.as_deref(), Some("btw-normal" | "btw-clean-expanded"))
        {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.show_saved_side_question(side_question::inline::screenshot_thread(), cx);
                });
            });
        }
        if crate::harness::screenshot_mode()
            && let Some(questions) = fixture.as_deref().and_then(ask_fixtures::questions)
        {
            let panel = cx.weak_entity();
            let applet = fixture.as_deref() == Some("ask-applet");
            let opened = fixture.as_deref() == Some("ask");
            cx.defer(move |cx| {
                let event = serde_json::json!({"ev":"ask_request", "session_id":"screenshot-fixture", "request_id":"screenshot-ask", "tool_call_id": ask_fixtures::TOOL_CALL, "questions":questions});
                if applet {
                    crate::applet_runtime::get(cx).sync_agent("screenshot-fixture", &ask_fixtures::applet("screenshot-fixture"));
                }
                if let Ok(event) = serde_json::from_value::<ApiEvent>(event) {
                    let _ = panel.update(cx, |panel, cx| {
                        panel.apply(&event, cx);
                        if let Some(card) = panel.ask_card.as_mut().filter(|_| opened) {
                            card.toggle(0, 0);
                            card.open_comment(0, 0);
                        }
                    });
                }
            });
        }
        if crate::harness::screenshot_mode() && fixture.as_deref() == Some("mode-picker") {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| panel.seed_mode_picker_fixture(cx));
            });
        }
        if session_id == "screenshot-fixture" && crate::fixture_control_targets::voice_recording() {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| panel.seed_inline_voice_fixture(cx));
            });
        }
        if crate::harness::screenshot_mode() && fixture.as_deref() == Some("image-prompt") {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| {
                    let image = image_preview::fixture_image();
                    let attachment = (image.media_type, image.data);
                    panel.seed_screenshot_expanded_queue_image(attachment.clone());
                    panel.input.update(cx, |input, cx| {
                        input.recall_prompt(
                            "Draft [#image 2] and [#image 1] with more text",
                            vec![attachment.clone(), attachment],
                            cx,
                        );
                    });
                    cx.notify();
                });
            });
        }
        let activity_owner = cx.entity();
        let queue_items_scroll = gpui::ScrollHandle::new();
        let focus_handle = cx.focus_handle();
        let openrig = (crate::harness::screenshot_mode() && fixture.as_deref() == Some("openrig"))
            .then(|| {
                let focus = focus_handle.clone();
                let session = session_id.clone();
                cx.new(|_| openrig::State::screenshot_fixture(focus, session))
            });
        Self {
            session_id,
            title: display_title.into(),
            working_dir,
            status: activity_status.into(),
            turn_outcome: None,
            connection_phase: String::new(),
            continuing_after_reload: false,
            model: effort_fixture.then(|| "gpt-5.6-sol".into()),
            provider: effort_fixture.then(|| "openai".into()),
            auth_method: usage_fixture.then(|| "oauth".into()),
            account_label: usage_fixture.then(|| "work@example.com".into()),
            reasoning_effort: effort_fixture.then(|| "high".into()),
            skills: Vec::new(),
            mcp_servers: None,
            active_skill: None,
            pinned_modes: Vec::new(),
            mode_multiselect: false,
            mode_catalog: Default::default(),
            pending_model: None,
            pending_effort: None,
            pending_save_requests: 0,
            context_tokens: usage_fixture.then_some(100_000),
            context_usage: context_usage::State::default(),
            memory_activity: None,
            response_stats: response_stats::Tracker::default(),
            cache_telemetry: cache_timer::card::CacheTelemetry::default(),
            items,
            live_assistant_output: None,
            streaming_text: if streaming_fixture {
                "I’m checking the implementation and updating the active panel indicators as the response arrives…".into()
            } else {
                String::new()
            },
            streaming_reasoning: String::new(),
            text_reveal: Default::default(),
            reasoning_reveal: Default::default(),
            #[cfg(test)]
            animate_stream_in_tests: false,
            sound_events: crate::sound_events::SoundEvents::default(),
            activity_spinner: cx.new(|cx| activity::Spinner::for_panel(Self::activity_readings(&activity_owner), cx)),
            swarm_workers: Vec::new(),
            background_jobs: Default::default(),
            pending_awaits: Vec::new(),
            pending_await_refresh: None,
            tab_dependency_waiting: false,
            worker_record: None,
            swarm_list_open: false,
            swarm_list_focus: cx.focus_handle(),
            swarm_finished_open: false,
            swarm_finished_focus: cx.focus_handle(),
            swarm_picker_rows: Vec::new(),
            swarm_list_scroll: ScrollHandle::new(),
            swarm_trigger_focus: cx.focus_handle(),
            swarm_marks: Default::default(),
            swarm_usage: Default::default(),
            swarm_row_focus: Default::default(),
            latest_activity_spinner: cx
                .new(|cx| activity::Spinner::for_panel(Self::activity_readings(&activity_owner), cx)),
            tab_outline: cx.new(|cx| tab_outline::TabOutline::new(
                    Live::of(&activity_owner, |panel: &Panel, _| panel.tab_ring()),
                    cx,
                )),
            // The completed-plan fixture shows a turn that finished unseen.
            finished_unseen: crate::harness::screenshot_mode()
                && fixture.as_deref() == Some("todos-completed"),
            sidebar_spinner: cx
                .new(|cx| activity::Spinner::for_sidebar(Self::activity_readings(&activity_owner), cx)),
            surface_focused: true,
            input,
            voice: voice_state::VoiceState::default(),
            image_pane_open: false,
            session_image_gallery: Default::default(),
            session_image_base: 0,
            draft_has_images: false,
            image_pane_selected: None,
            image_pane_scroll: ScrollHandle::new(),
            image_pane_focus: cx.focus_handle().tab_stop(true),
            image_pane_stacked: false,
            image_preview: None,
            image_preview_focus_pending: false,
            pending_history_image: None,
            history_image_requests: std::cell::RefCell::new(HashMap::new()),
            image_preview_dimensions: None,
            diff_review: None,
            edit_previews: diff_review::EditPreviews::default(),
            image_preview_zoom: 1.0,
            image_preview_scroll: gpui::ScrollHandle::new(),
            image_preview_drag: None,
            focus_handle,
            rewind_selection: None,
            transcript_list,
            clean_chat_override: match fixture.as_deref().filter(|_| crate::harness::screenshot_mode()) {
                Some("external-dm-clean" | "btw-clean" | "btw-clean-expanded") => Some(true),
                Some("btw-normal") => Some(false),
                Some("external-dm") => Some(false),
                _ => None,
            },
            pending_approval: None,
            routing_chip: None,
            routing_popover: None,
            routing_popover_focus: cx.focus_handle(),
            routing_trigger_focus: cx.focus_handle(),
            routing_gate: None,
            expandable_viewport_height: px(0.),
            transcript_row_count: 0,
            transcript_measurements: Default::default(),
            flicker_diagnostics: Default::default(),
            startup_layout: None,
            offscreen_prompt: None,
            offscreen_prompt_clip: None,
            expanded_pinned_prompt: None,
            swarm_scrolls: Default::default(),
            publish_tracker: false,
            transcript_selection,
            transcript_text_document: Default::default(),
            transcript_dragging: false,
            transcript_wheel_glide: WheelGlide::default(),
            transcript_wheel_frame: None,
            transcript_wheel_frame_pending: false,
            tail_glide: Default::default(),
            stick_to_bottom: fixture.as_deref() != Some("transcript-scroll"),
            transcript_end_visible: true,
            composer_dock_height: 0.0,
            steady_activity: Default::default(),
            transcript_end_pad: 0.0,
            pending_history_scroll: None,
            bridge,
            rig_feed,
            rig_host_id,
            rig_demand: None,
            rig_identity,
            preview_state: None,
            demo: false,
            history_loaded: false,
            history_paging: None,
            reconnect_response: None,
            restored_transcript: false,
            provisional_items: 0,
            provisional_task: None,
            awaiting_persisted: false,
            draft_session: DraftSession::Requested,
            draft_config: DraftConfig::default(),
            speed: speed::SessionSpeed::default(),
            expanded_regions,
            tool_detail_motion: HashMap::new(),
            prompt_queue: queue::PromptQueue::default(),
            pending_users: VecDeque::new(),
            accepted_users: HashMap::new(),
            user_times: HashMap::new(),
            arriving_tools: HashMap::new(),
            applet_positions: HashMap::new(),
            applet_selection: cx.new(TextSelection::new),
            applet_generation: (u64::MAX, 0, false),
            applet_rows: Vec::new(),
            applet_rows_version: 0,
            transcript_derived: None,
            control_reveal: None,
            tool_started: HashMap::new(),
            tool_durations: fixture_tool_durations,
            tool_wall_times: fixture_tool_wall_times,
            codemode_summaries: fixture_codemode_summaries,
            running_tool_tickers: Default::default(),
            superseded_drafts: Default::default(),
            unfinished_work: None,
            unfinished_session_opener: None,
            side_document: None,
            applet: None,
            todoist: (crate::harness::screenshot_mode() && fixture.as_deref() == Some("todos"))
                .then(|| TodoistPanelState::fixture(80)),
            openrig,
            orchestration: (crate::harness::screenshot_mode()
                && fixture.as_deref() == Some("orchestration"))
            .then(orchestration::State::fixture),
            recovery_picker_open: false,
            ask_card: None,
            ask_resize: crate::input::VerticalResize::default(),
            queued_asks: VecDeque::new(),
            ask_focus_return: None,
            ask_recovery: None,
            answered_asks: HashSet::new(),
            unavailable_asks: HashMap::new(),
            ask_attachment_id: 0,
            queue_items_scroll: queue_items_scroll.clone(),
            queue_edit_anchor: gpui::ScrollAnchor::for_handle(queue_items_scroll),
            pending_ask_draft: None,
            side_question: None,
            side_question_history: None,
            side_question_resize: crate::input::VerticalResize::collapsible(),
            composer_overlay_room: Default::default(),
            model_picker_open: false,
            picker_search: None,
            picker_triggers: std::array::from_fn(|_| cx.focus_handle()),
            composer_popover_watch: None,
            composer_popover_trigger: None,
            focus_pending: false,
            transcript_only: false,
            search_terms: Vec::new(),
            login: None,
            available_models: Vec::new(),
        }
    }

    pub(super) fn wire_input(panel: &Entity<Panel>, cx: &mut App) {
        if panel.read(cx).is_side_document() {
            return;
        }
        let weak = panel.downgrade();
        panel.update(cx, |this, cx| {
            let cancel_weak = weak.clone();
            let change_weak = weak.clone();
            let speed_weak = weak.clone();
            let session_image_base = this.session_image_base;
            this.input = cx.new(|cx| {
                let mut input = PromptInput::new_with_queue_files(
                    cx,
                    "Type something…",
                    move |kind, content, images, files, queued, _window, app| {
                        let Some(panel) = weak.upgrade() else { return };
                        if kind != crate::input::SubmissionKind::Command {
                            panel.update(app, |this, cx| {
                                if this.preview_state.is_some() {
                                    return;
                                }
                                if !content.trim().is_empty()
                                    || !images.is_empty()
                                    || !files.is_empty()
                                    || (!queued && this.has_visible_queued_prompts())
                                {
                                    let restore = match kind {
                                        crate::input::SubmissionKind::Skill(name) => Some(this.active_skill.replace(name)),
                                        _ => None,
                                    };
                                    this.submit_or_queue_with_files(
                                        content, images, files, queued, cx,
                                    );
                                    if let Some(previous) = restore {
                                        this.active_skill = previous;
                                    }
                                }
                            });
                            return;
                        }
                        Panel::dispatch_composer_command(&panel, content, _window, app);
                    },
                )
                .with_empty_submit()
                .with_standalone_btw()
                .with_example_prompts()
                .with_on_model_speed(move |choice, app| {
                    let _ =
                        speed_weak.update(app, |panel, cx| panel.choose_model_speed(choice, cx));
                })
                .with_ghost_completions({
                    let weak = change_weak.clone();
                    move |app| {
                        weak.upgrade()
                            .map(|panel| panel.read(app).completion_context())
                            .unwrap_or_default()
                    }
                })
                .with_on_change(move |content, app| {
                    if let Some(panel) = change_weak.upgrade() {
                        panel.update(app, |this, cx| {
                            if content == "/model" || content.starts_with("/model ") {
                                this.seed_draft_models(cx);
                            }
                            if (content == "/model" || content.starts_with("/model "))
                                && (!this.is_pending_session() || !this.available_models.is_empty())
                            {
                                this.model_picker_open = true;
                                cx.notify();
                            } else if this.model_picker_open && !content.starts_with("/model ") {
                                this.close_model_picker(cx);
                            }
                        });
                    }
                })
                .with_on_overlay_cancel(move |app| {
                    let mut handled = false;
                    if let Some(panel) = cancel_weak.upgrade() {
                        panel.update(app, |this, cx| {
                            if this.recovery_picker_open {
                                this.recovery_picker_open = false;
                                handled = true;
                            } else if this.model_picker_open {
                                this.close_model_picker(cx);
                                handled = true;
                            } else if !this.is_pending_session()
                                && (this.activity_active()
                                    || this.has_unconfirmed_stop()
                                    || !this.pending_users.is_empty())
                            {
                                this.prompt_queue.paused = true;
                                this.sound_events.cancel();
                                this.request_user_stop(cx);
                                handled = true;
                            }
                            cx.notify();
                        });
                    }
                    handled
                });
                input.set_session_image_base(session_image_base, cx);
                input.set_speed_picker(this.speed_picker(), cx);
                input.enable_context_picker();
                input
            });
            Self::observe_draft_images(&this.input, cx);
            if std::env::var("JCODE_DESKTOP_SCREENSHOT").as_deref() == Ok("1")
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_MODELS").as_deref() == Ok("1")
            {
                let names = [
                    "anthropic:sonnet-review".to_string(),
                    "google:gemini-review".to_string(),
                ]
                .into_iter()
                .chain((1..=12).map(|index| format!("openai:atlas-{index:02}")));
                let routes = names
                    .map(|model| {
                        let provider = model.split(':').next().unwrap().to_string();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let usage = match provider.as_str() {
                            "anthropic" => Some(jcode_sdk::ModelUsage {
                                count: 42,
                                last_used_unix_secs: Some(now - 7200),
                                tracking_started_unix_secs: Some(now - 86400 * 14),
                                selection_count: 5,
                                last_selected_unix_secs: Some(now - 7200),
                            }),
                            "google" => Some(jcode_sdk::ModelUsage {
                                count: 7,
                                last_used_unix_secs: Some(now - 86400 * 3),
                                tracking_started_unix_secs: Some(now - 86400 * 14),
                                selection_count: 2,
                                last_selected_unix_secs: Some(now - 86400 * 3),
                            }),
                            _ => None,
                        };
                        jcode_sdk::ModelRouteInfo {
                            account: None,
                            icon: None,
                            speed_options: Vec::new(),
                            reasoning_efforts: None,
                            api_method: format!("{provider}-api-key"),
                            model,
                            provider,
                            available: true,
                            detail: String::new(),
                            usage,
                        }
                    })
                    .collect();
                this.apply(
                    &ApiEvent::RuntimeInfo {
                        speed_mode: Default::default(),
                        speed_options: Vec::new(),
                        session_id: this.session_id.clone(),
                        provider: Some("openai".into()),
                        model: Some("openai:atlas-01".into()),
                        reasoning_effort: None,
                        auth_method: None,
                        account_label: None,
                        routes,
                    },
                    cx,
                );
            }
            // Read ahead, so the `+` menu has its MCP row on the first frame.
            this.load_mcp_servers(cx);
            if this.is_pending_session() {
                this.draft_session = DraftSession::Unrequested;
                this.status = "Starting session · you can type now".into();
                this.input
                    .update(cx, |input, cx| input.set_pending_session(true, cx));
            }
        });
    }

    pub(super) fn capture(&self, cx: &App) -> PanelSnapshot {
        let offset = self
            .document_scroll_offset(cx)
            .unwrap_or_else(|| self.transcript_list.scroll_px_offset_for_scrollbar());
        PanelSnapshot {
            draft_config: self.draft_config.clone(),
            clean_chat_override: self.clean_chat_override,
            transcript: None,
            turn_outcome: self.turn_outcome,
            finished_unseen: self.finished_unseen,
            image_pane_open: self.image_pane_open,
            side_document: self.document_snapshot(cx),
            prompt_queue: self.prompt_queue.clone(),
            session_id: self.session_id.clone(),
            title: self.title.to_string(),
            working_dir: self.working_dir.clone(),
            draft: self.input.read(cx).snapshot(),
            ask_draft: self
                .ask_card
                .as_ref()
                .map(|card| card.draft(cx))
                .or_else(|| self.pending_ask_draft.clone()),
            scroll_x: f32::from(offset.x),
            scroll_y: f32::from(offset.y),
            stick_to_bottom: self.stick_to_bottom,
            startup_layout: self
                .startup_layout
                .clone()
                .filter(|layout| layout.committed),
            terminal_resource_id: None,
            terminal_output_cursor: None,
        }
    }
}
