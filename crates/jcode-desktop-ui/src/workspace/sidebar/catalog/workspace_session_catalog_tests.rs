//! Workspace tests: session_catalog.
use super::*;

#[gpui::test]
fn restoring_live_session_keeps_identity_while_attaching(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("same-id", cx);
        workspace
    });
    let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::Event {
                session_id: "same-id".into(),
                event: jcode_sdk::ApiEvent::ModelInfo {
                    session_id: "same-id".into(),
                    provider: Some("openai".into()),
                    model: Some("gpt-5.6".into()),
                    reasoning_effort: Some("high".into()),
                    auth_method: None,
                    account_label: None,
                },
            },
            cx,
        );
        workspace.apply(
            Update::SessionAttaching {
                session_id: "same-id".into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.model.as_deref(), Some("gpt-5.6"));
        assert_eq!(panel.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(panel.status, "attaching…");
    });
    assert!(vcx.debug_bounds("panel-model-effort").is_some());
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::Event {
                session_id: "same-id".into(),
                event: jcode_sdk::ApiEvent::RuntimeInfo {
                    speed_mode: Default::default(),
                    speed_options: Vec::new(),
                    session_id: "same-id".into(),
                    provider: None,
                    model: None,
                    routes: vec![],
                    reasoning_effort: None,
                    auth_method: None,
                    account_label: None,
                },
            },
            cx,
        );
    });
    panel.read_with(vcx, |panel, _| {
        assert_eq!(panel.model.as_deref(), Some("gpt-5.6"));
        assert_eq!(panel.reasoning_effort.as_deref(), Some("high"));
    });
}

#[test]
fn sidebar_never_adds_a_workspace_top_inset() {
    assert_eq!(content_top_inset(true, false), 0.0);
    assert_eq!(content_top_inset(true, true), 0.0);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn sidebar_header_keeps_its_linux_alignment() {
    assert_eq!(sidebar_header_left_padding(false), 12.0);
    assert_eq!(sidebar_header_left_padding(true), 12.0);
}

#[test]
#[cfg(target_os = "macos")]
fn sidebar_header_clears_macos_traffic_lights() {
    assert_eq!(
        sidebar_header_left_padding(false),
        MACOS_TRAFFIC_LIGHTS_WIDTH
    );
}

#[test]
#[cfg(target_os = "macos")]
fn fullscreen_hides_traffic_lights_so_the_header_realigns() {
    assert_eq!(sidebar_header_left_padding(true), 12.0);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn hidden_sidebar_does_not_leave_a_top_gap_off_macos() {
    assert_eq!(content_top_inset(false, false), 0.0);
    assert_eq!(content_top_inset(false, true), 0.0);
}

#[test]
#[cfg(target_os = "macos")]
fn hidden_sidebar_preserves_room_for_macos_traffic_lights() {
    assert_eq!(content_top_inset(false, false), TITLEBAR_HEIGHT);
}

#[test]
#[cfg(target_os = "macos")]
fn fullscreen_reclaims_the_titlebar_strip() {
    assert_eq!(content_top_inset(false, true), 0.0);
}

/// Opt-in micro-profiler for the complete keymap -> workspace state path.
///
/// Run with:
/// `cargo test --release -p jcode-desktop-ui interaction_latency_profile -- --ignored --nocapture`
///
/// This deliberately does not call `run_until_parked` inside the measured
/// section. The resulting number is input-to-state latency, while visual
/// settling time is the transition policy duration reported alongside it.
#[gpui::test]
#[ignore = "manual latency profiler"]
fn interaction_latency_profile(cx: &mut gpui::TestAppContext) {
    const WARMUP: usize = 50;
    const SAMPLES: usize = 500;

    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for row in 0..ROWS_PER_SPACE {
            workspace.active_row = row;
            for column in 0..8 {
                workspace.push_test_panel(&format!("{row}-{column}"), cx);
            }
        }
        workspace.active_row = 0;
        workspace.active = 3;
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| {
        window.focus(&workspace.read(cx).focus_handle.clone(), cx);
    });
    vcx.run_until_parked();

    // Prove every timed key pair mutates the intended state before trusting
    // its distribution. This prevents a broken binding or edge no-op from
    // looking implausibly fast and being reported as a latency improvement.
    let initial_active = workspace.read_with(vcx, |workspace, _| workspace.active);
    vcx.simulate_keystrokes("super-h");
    assert_ne!(
        workspace.read_with(vcx, |workspace, _| workspace.active),
        initial_active
    );
    vcx.simulate_keystrokes("super-l");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.active),
        initial_active
    );

    vcx.simulate_keystrokes("super-j");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.active_row),
        1
    );
    vcx.simulate_keystrokes("super-k");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.active_row),
        0
    );

    let initial_order = workspace.read_with(vcx, |workspace, _| {
        workspace
            .slots
            .iter()
            .map(|slot| slot.panel.entity_id())
            .collect::<Vec<_>>()
    });
    vcx.simulate_keystrokes("super-shift-h");
    assert_ne!(
        workspace.read_with(vcx, |workspace, _| workspace
            .slots
            .iter()
            .map(|slot| slot.panel.entity_id())
            .collect::<Vec<_>>()),
        initial_order
    );
    vcx.simulate_keystrokes("super-shift-l");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace
            .slots
            .iter()
            .map(|slot| slot.panel.entity_id())
            .collect::<Vec<_>>()),
        initial_order
    );

    vcx.simulate_keystrokes("super-1");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.slots[workspace.active]
            .width_fraction),
        0.25
    );
    vcx.simulate_keystrokes("super-2");
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.slots[workspace.active]
            .width_fraction),
        0.5
    );

    fn measure(
        cx: &mut gpui::VisualTestContext,
        first: &str,
        second: &str,
    ) -> (u128, u128, u128, u128) {
        let mut samples = Vec::with_capacity(SAMPLES);
        for iteration in 0..WARMUP + SAMPLES {
            let key = if iteration % 2 == 0 { first } else { second };
            let started = Instant::now();
            cx.simulate_keystrokes(key);
            let elapsed = started.elapsed().as_nanos();
            if iteration >= WARMUP {
                samples.push(elapsed);
            }
        }
        samples.sort_unstable();
        let percentile = |numerator: usize| samples[(samples.len() - 1) * numerator / 100];
        let mean = samples.iter().sum::<u128>() / samples.len() as u128;
        (mean, percentile(50), percentile(95), percentile(99))
    }

    for (name, first, second, transition) in [
        ("focus_horizontal", "super-h", "super-l", Transition::Focus),
        ("focus_vertical", "super-j", "super-k", Transition::Row),
        (
            "move_horizontal",
            "super-shift-h",
            "super-shift-l",
            Transition::PanelOrder,
        ),
        ("resize", "super-1", "super-2", Transition::PanelWidth),
    ] {
        let (mean, p50, p95, p99) = measure(vcx, first, second);
        println!(
            "LATENCY {name:>16} mean={:.1}us p50={:.1}us p95={:.1}us p99={:.1}us settle={}ms",
            mean as f64 / 1_000.0,
            p50 as f64 / 1_000.0,
            p95 as f64 / 1_000.0,
            p99 as f64 / 1_000.0,
            transition::policy(transition).duration.as_millis(),
        );
    }
}

#[gpui::test]
fn empty_strip_navigation_is_captured_but_boundary_noops_are_not(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let directory = tempfile::tempdir().unwrap();
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    vcx.update(|window, cx| window.focus(&workspace.read(cx).focus_handle.clone(), cx));
    for (key, row, expected) in [
        ("super-j", 1, true),
        ("super-k", 0, true),
        ("super-k", 0, false),
    ] {
        workspace.update(vcx, |workspace, _| {
            workspace.action_capture =
                Some(ActionCapture::new(directory.path().join("actions.jsonl")));
        });
        vcx.simulate_keystrokes(key);
        workspace.read_with(vcx, |workspace, _| {
            assert_eq!(workspace.active_row, row);
            assert_eq!(
                workspace.action_capture.as_ref().unwrap().is_pending(),
                expected
            );
        });
    }
}

/// Loaded key-to-first-render profiler. Unlike `interaction_latency_profile`,
/// this lets GPUI construct the next frame for 32 live panel entities before
/// stopping the clock. Presentation cadence is measured by the opt-in runtime
/// profile because the test platform intentionally has no compositor.
#[gpui::test]
#[ignore = "manual loaded animation profiler"]
fn loaded_animation_first_frame_profile(cx: &mut gpui::TestAppContext) {
    const WARMUP: usize = 10;
    const SAMPLES: usize = 100;

    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        for row in 0..ROWS_PER_SPACE {
            workspace.active_row = row;
            for column in 0..8 {
                workspace.push_test_panel(&format!("loaded-{row}-{column}"), cx);
            }
        }
        workspace.active_row = 0;
        workspace.active = 3;
        let _ = window;
        workspace
    });
    vcx.update(|window, cx| window.focus(&workspace.read(cx).focus_handle.clone(), cx));
    vcx.run_until_parked();

    let mut samples = Vec::with_capacity(SAMPLES);
    for iteration in 0..WARMUP + SAMPLES {
        let key = if iteration % 2 == 0 {
            "super-h"
        } else {
            "super-l"
        };
        let started = Instant::now();
        vcx.simulate_keystrokes(key);
        vcx.run_until_parked();
        if iteration >= WARMUP {
            samples.push(started.elapsed().as_nanos());
        }
    }
    samples.sort_unstable();
    let percentile = |value: usize| samples[(samples.len() - 1) * value / 100];
    println!(
        "LOADED_ANIMATION first_frame p50={:.1}us p95={:.1}us p99={:.1}us panels=32",
        percentile(50) as f64 / 1_000.0,
        percentile(95) as f64 / 1_000.0,
        percentile(99) as f64 / 1_000.0,
    );
    assert_eq!(workspace.read_with(vcx, |workspace, _| workspace.active), 3);
}

#[test]
fn sidebar_free_launch_flags_hide_the_sidebar() {
    assert!(sidebar_enabled(["jcode-desktop"], true));
    assert!(!sidebar_enabled(["jcode-desktop"], false));
    assert!(!sidebar_enabled(["jcode-desktop", "--no-sidebar"], true));
    assert!(!sidebar_enabled(["jcode-desktop", "--workspace"], true));
    assert!(!sidebar_enabled(["jcode-desktop", "--single-panel"], true));
}

#[test]
fn workspace_snapshot_round_trips_layout_focus_drafts_scroll_and_terminal_id() {
    let draft = PromptInputSnapshot {
        content: "unfinished prompt".into(),
        selection_start: 3,
        selection_end: 9,
        selection_reversed: true,
        history: vec!["older prompt".into()],
        history_index: None,
        live_draft: "unfinished prompt".into(),
        history_draft: None,
        stashed_prompt: None,
        attachments: vec![crate::input::AttachmentSnapshot {
            media_type: "image/png".into(),
            encoded: "aW1hZ2U=".into(),
            label: "diagram.png".into(),
        }],
    };
    let snapshot = WorkspaceSnapshot {
        launch: None,
        sidebar_closed_sessions: HashSet::new(),
        format_version: SNAPSHOT_FORMAT_VERSION,
        resume: screens::Snapshot::Closed,
        side_panel_snapshots: HashMap::new(),
        layout_mode: crate::config::LayoutMode::Normal,
        recent_accounts: Vec::new(),
        sidebar_view: SidebarView::Sessions,
        session_catalog: Default::default(),
        tutorial_page: 0,
        slots: vec![SlotSnapshot {
            panel: PanelSnapshot {
                draft_config: Default::default(),
                clean_chat_override: None,
                transcript: None,
                turn_outcome: None,
                finished_unseen: false,
                image_pane_open: false,
                side_document: None,
                prompt_queue: Default::default(),
                ask_draft: None,
                session_id: "terminal".into(),
                title: "build shell".into(),
                working_dir: Some("/workspace".into()),
                draft,
                scroll_x: -4.0,
                scroll_y: -128.5,
                stick_to_bottom: false,
                terminal_resource_id: Some(42),
                terminal_output_cursor: Some(1234),
                startup_layout: None,
            },
            row: 2,
            width_fraction: 0.75,
            restore_fraction: Some(0.5),
            shared_id: None,
        }],
        active: 0,
        active_row: 2,
        space_hosts: Vec::new(),
        row_focus: vec![None, None, Some(0), None],
        previous: Some(0),
        last_voice_chat: Some(0),
        camera_x: vec![0.0, 10.0, 20.0, 30.0],
        camera_target: vec![1.0, 11.0, 21.0, 31.0],
        overview: true,
        hints_overlay: true,
        folder_picker_sets_default: false,
        folder_picker_dir: Some(PathBuf::from("/workspace/src")),
        folder_picker_error: Some("example".into()),
        folder_search: None,
        focus: FocusSnapshot::Panel(0),
    };

    let encoded = snapshot.encode().expect("encode workspace snapshot");
    let decoded = WorkspaceSnapshot::decode(&encoded).expect("decode workspace snapshot");
    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.layout_mode, crate::config::LayoutMode::Normal);

    let mut legacy: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    legacy.as_object_mut().unwrap().remove("layout_mode");
    legacy["slots"][0]["panel"]
        .as_object_mut()
        .unwrap()
        .remove("terminal_output_cursor");
    let legacy = WorkspaceSnapshot::decode(&serde_json::to_vec(&legacy).unwrap())
        .expect("old workspace snapshot remains compatible");
    assert_eq!(legacy.layout_mode, crate::config::LayoutMode::FolderTabs);
    assert_eq!(legacy.slots[0].panel.terminal_output_cursor, None);
}

#[test]
fn workspace_snapshot_rejects_invalid_layout_before_root_replacement() {
    let invalid = serde_json::json!({
        "format_version": SNAPSHOT_FORMAT_VERSION,
        "slots": [],
        "active": 0,
        "active_row": ROWS_PER_SPACE,
        "row_focus": [null, null, null, null],
        "previous": null,
        "camera_x": [0.0, 0.0, 0.0, 0.0],
        "camera_target": [0.0, 0.0, 0.0, 0.0],
        "overview": false,
        "hints_overlay": false,
        "folder_picker_dir": null,
        "folder_picker_error": null,
        "folder_search": null,
        "focus": "Workspace"
    });
    let error = WorkspaceSnapshot::decode(&serde_json::to_vec(&invalid).unwrap())
        .expect_err("out-of-range row must be rejected");
    assert!(error.to_string().contains("active row"));
}

#[test]
fn sidebar_in_place_sort_keeps_equal_recency_catalog_order() {
    let mut sessions = vec![
        session_info("first", Some("first")),
        session_info("second", Some("second")),
        session_info("saved-first", None),
        session_info("saved-second", None),
    ];
    for session in &mut sessions {
        session.updated_at_ms = Some(42);
        session.saved = session.session_id.starts_with("saved");
    }
    let allocation = sessions.as_ptr();
    sort_sidebar_sessions(&mut sessions, sidebar_session_catalog::Sort::Recent);
    assert_eq!(sessions.as_ptr(), allocation, "reuse the owned catalog");
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>(),
        ["saved-first", "saved-second", "first", "second"],
    );
    assert_eq!(sessions[2].title.as_deref(), Some("first"));
}

#[test]
fn sidebar_session_catalog_sorts_replies_ahead_of_renames_and_missing_times() {
    use jcode_sdk::api::{SessionCatalogActivity, SessionCatalogContext, SessionOrigin};
    let mut sessions = [
        ("newest-update", 100, Some(10)),
        ("latest-reply", 40, Some(30)),
        ("unanswered", 200, None),
    ]
    .map(|(id, updated, replied)| {
        let mut session = session_info(id, Some(id));
        session.updated_at_ms = Some(updated);
        session.saved = id == "newest-update";
        session.context = Some(SessionCatalogContext {
            activity: Some(SessionCatalogActivity {
                origin: SessionOrigin::User,
                last_agent_activity_at_ms: replied,
                last_message_at_ms: None,
            }),
            ..Default::default()
        });
        session
    });
    sort_sidebar_sessions(&mut sessions, sidebar_session_catalog::Sort::AgentActivity);
    assert_eq!(
        sessions.map(|session| session.session_id),
        ["latest-reply", "newest-update", "unanswered"]
    );
}

#[test]
fn sidebar_matches_tui_saved_then_recency_order() {
    let mut old = session_info("old", None);
    old.updated_at_ms = Some(10);
    let mut recent = session_info("recent", None);
    recent.updated_at_ms = Some(30);
    let mut saved_old = session_info("saved-old", None);
    saved_old.saved = true;
    saved_old.updated_at_ms = Some(5);
    let mut saved_recent = session_info("saved-recent", None);
    saved_recent.saved = true;
    saved_recent.updated_at_ms = Some(20);

    let ids = sidebar_session_order(&[old, saved_old, recent, saved_recent])
        .into_iter()
        .map(|session| session.session_id)
        .collect::<Vec<_>>();

    assert_eq!(ids, ["saved-recent", "saved-old", "recent", "old"]);
}

#[test]
fn sidebar_prefers_the_sdk_title_and_uses_the_session_animal_icon() {
    let session = session_info(
        "session_tigress_1234567890_deadbeef",
        Some("  Release planning  "),
    );
    assert_eq!(
        sidebar_session_title(&session),
        ("🐅", "Release planning".into())
    );
}

#[gpui::test]
fn identical_sdk_session_snapshots_do_not_invalidate_the_sidebar(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    let mut sessions: Vec<_> = (0..12)
        .map(|i| {
            let mut session =
                session_info(&format!("session_fox_{i}_deadbeef"), Some("Planning"));
            session.updated_at_ms = Some(i + 1);
            session
        })
        .collect();
    let panel_updates = std::rc::Rc::new(std::cell::Cell::new(0));
    workspace.update(vcx, |workspace, cx| {
        for session in &sessions {
            workspace.open_session(session.clone(), cx);
        }
        assert!(workspace.apply(
            Update::Sessions {
                sessions: sessions.clone(),
            },
            cx,
        ));
        for slot in &workspace.slots {
            let count = panel_updates.clone();
            cx.observe(slot.panel.chat().unwrap(), move |_, _, _| {
                count.set(count.get() + 1)
            })
            .detach();
        }
    });
    vcx.run_until_parked();
    panel_updates.set(0);
    let before = crate::render_stats::snapshot();
    for _ in 0..25 {
        for session in &mut sessions {
            session.updated_at_ms = session.updated_at_ms.map(|ms| ms + 20);
        }
        workspace.update(vcx, |workspace, cx| {
            // apply's SDK guard panics if it scans an await directory, even
            // for the first non-identical snapshot.
            assert!(!workspace.apply(
                Update::Sessions {
                    sessions: sessions.clone()
                },
                cx
            ));
        });
    }
    vcx.run_until_parked();
    assert_eq!(panel_updates.get(), 0);
    assert_eq!(crate::render_stats::snapshot(), before);

    // Recency still matters when it actually changes the displayed order.
    sessions[0].updated_at_ms = Some(i64::MAX);
    workspace.update(vcx, |workspace, cx| {
        assert!(workspace.apply(Update::Sessions { sessions }, cx));
    });
}

#[gpui::test]
fn session_refresh_updates_the_footer_directory_without_a_title(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(cx, |workspace, cx| {
        let mut session = session_info("session_fox_1234567890_deadbeef", None);
        session.working_dir = None;
        let slot = workspace.open_session(session.clone(), cx);
        for directory in ["/srv/project", "/srv/other"] {
            session.working_dir = Some(directory.into());
            workspace.apply(
                Update::Sessions {
                    sessions: vec![session.clone()],
                },
                cx,
            );
            assert_eq!(workspace.slots[slot].panel.profile().working_dir(cx), Some(directory));
        }
        for directory in [None, Some("   ")] {
            session.working_dir = directory.map(str::to_owned);
            workspace.apply(
                Update::Sessions {
                    sessions: vec![session.clone()],
                },
                cx,
            );
            assert_eq!(
                workspace.slots[slot].panel.profile().working_dir(cx),
                Some("/srv/other")
            );
        }
    });
}

#[gpui::test]
fn session_catalog_refresh_keeps_a_restored_open_session_in_the_sidebar(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.apply_snapshot(
            WorkspaceSnapshot {
                launch: None,
                sidebar_closed_sessions: HashSet::new(),
                format_version: SNAPSHOT_FORMAT_VERSION,
                resume: screens::Snapshot::Closed,
                side_panel_snapshots: HashMap::new(),
                layout_mode: crate::config::LayoutMode::FolderTabs,
                recent_accounts: Vec::new(),
                sidebar_view: SidebarView::Sessions,
                session_catalog: Default::default(),
                tutorial_page: 0,
                slots: vec![SlotSnapshot {
                    panel: PanelSnapshot {
                        draft_config: Default::default(),
                        clean_chat_override: None,
                        transcript: None,
                        turn_outcome: None,
                        finished_unseen: false,
                        image_pane_open: false,
                        side_document: None,
                        prompt_queue: Default::default(),
                        ask_draft: None,
                        session_id: "session_fox_1234567890000_deadbeef".into(),
                        title: "Still running".into(),
                        working_dir: Some("/home/example/project".into()),
                        draft: PromptInputSnapshot {
                            content: String::new(),
                            selection_start: 0,
                            selection_end: 0,
                            selection_reversed: false,
                            history: Vec::new(),
                            history_index: None,
                            live_draft: String::new(),
                            history_draft: None,
                            stashed_prompt: None,
                            attachments: Vec::new(),
                        },
                        scroll_x: 0.0,
                        scroll_y: 0.0,
                        stick_to_bottom: true,
                        terminal_resource_id: None,
                        terminal_output_cursor: None,
                        startup_layout: None,
                    },
                    row: 0,
                    width_fraction: 1.0,
                    restore_fraction: None,
                    shared_id: None,
                }],
                active: 0,
                active_row: 0,
                space_hosts: Vec::new(),
                row_focus: vec![Some(0), None, None, None],
                previous: None,
                last_voice_chat: None,
                camera_x: vec![0.0; ROWS_PER_SPACE],
                camera_target: vec![0.0; ROWS_PER_SPACE],
                overview: false,
                hints_overlay: false,
                folder_picker_sets_default: false,
                folder_picker_dir: None,
                folder_picker_error: None,
                folder_search: None,
                focus: FocusSnapshot::Workspace,
            },
            cx,
        );
        assert!(workspace.apply(Update::Sessions { sessions: vec![] }, cx));

        assert_eq!(workspace.sessions.len(), 1);
        let session = &workspace.sessions[0];
        assert_eq!(session.session_id, "session_fox_1234567890000_deadbeef");
        assert_eq!(session.title.as_deref(), Some("Still running"));
        assert_eq!(
            session.working_dir.as_deref(),
            Some("/home/example/project")
        );
        assert_eq!(session.status, "active");
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("sidebar-session-0").is_some(),
        "the snapshot-restored running session must paint a sidebar row"
    );

    workspace.update(vcx, |workspace, cx| {
        let mut canonical = session_info(
            "session_fox_1234567890000_deadbeef",
            Some("Canonical runtime title"),
        );
        canonical.saved = true;
        canonical.updated_at_ms = Some(1_787_695_000_000);
        assert!(workspace.apply(
            Update::Sessions {
                sessions: vec![canonical],
            },
            cx,
        ));
        assert_eq!(
            workspace.sessions.len(),
            1,
            "the catalog must not duplicate it"
        );
        assert_eq!(
            workspace.sessions[0].title.as_deref(),
            Some("Canonical runtime title"),
            "canonical metadata must replace the restored placeholder"
        );
        assert!(workspace.sessions[0].saved);
        let mut worker = session_info("session_puppy_1234567890000_abcdef", Some("Worker"));
        worker.parent_session_id = Some("session_fox_1234567890000_deadbeef".into());
        worker.swarm_status = Some("running".into());
        workspace.sessions.push(worker);
        workspace.sync_swarm_strips(cx);
    });
    vcx.run_until_parked();
    let pill = vcx.debug_bounds("composer-workers").expect("workers pill");
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let row = vcx.debug_bounds("swarm-list-title-0").expect("worker row");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.update(vcx, |workspace, cx| {
        assert_eq!(
            workspace.slots[workspace.active].panel.session_id(cx),
            "session_puppy_1234567890000_abcdef",
            "clicking in a restored panel must activate the worker's chat"
        );
    });
}

#[gpui::test]
fn live_sdk_events_advance_sidebar_recency_at_turn_boundaries(cx: &mut gpui::TestAppContext) {
    let (workspace, cx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    let mut session = session_info("session_fox_1234567890000_deadbeef", None);
    session.updated_at_ms = Some(1);

    workspace.update(cx, |workspace, cx| {
        workspace.apply(
            Update::Sessions {
                sessions: vec![session],
            },
            cx,
        );
        assert!(!workspace.apply(
            Update::Event {
                session_id: "session_fox_1234567890000_deadbeef".into(),
                event: jcode_sdk::ApiEvent::TextDelta {
                    message_id: None,
                    session_id: "session_fox_1234567890000_deadbeef".into(),
                    text: "working".into(),
                },
            },
            cx,
        ));
        assert_eq!(workspace.sessions[0].updated_at_ms, Some(1));
        assert!(workspace.apply(
            Update::Event {
                session_id: "session_fox_1234567890000_deadbeef".into(),
                event: jcode_sdk::ApiEvent::TurnDone {
                    session_id: "session_fox_1234567890000_deadbeef".into(),
                    pending_soft_interrupts: None,
                },
            },
            cx,
        ));
        assert!(
            workspace.sessions[0]
                .updated_at_ms
                .is_some_and(|value| value > 1)
        );
    });
}

#[gpui::test]
fn sidebar_spins_for_daemon_running_sessions_without_open_panels(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace
            .sidebar_closed_sessions
            .extend(["background-run".into(), "history".into()]);
        workspace.sessions = vec![
            session_info("background-run", Some("Running in the daemon")),
            session_info("history", Some("Previous session")),
        ];
        workspace
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("sidebar-session-spinner-0").is_none());
    assert!(vcx.debug_bounds("sidebar-session-spinner-1").is_none());

    let running = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<HashSet<_>>();
    workspace.update(vcx, |workspace, cx| {
        workspace.set_daemon_running(running(&["background-run", "not-listed"]), cx)
    });
    vcx.run_until_parked();
    let row = workspace.read_with(vcx, |workspace, _| {
        workspace
            .sidebar_session_layout
            .iter()
            .position(|row| row.session_id == "background-run")
            .unwrap()
    });
    let spinner = |index: usize| format!("sidebar-session-spinner-{index}");
    assert!(vcx.debug_bounds(spinner(row).leak()).is_some());
    assert!(vcx.debug_bounds(spinner(1 - row).leak()).is_none());

    // Idle rows give the title the full width: the spinner sits at the
    // far right and no hidden actions reserve space.
    let row_name = format!("sidebar-session-{row}");
    let row_bounds = vcx.debug_bounds(row_name.clone().leak()).unwrap();
    let spin_bounds = vcx.debug_bounds(spinner(row).leak()).unwrap();
    let title = vcx
        .debug_bounds(format!("sidebar-session-title-{row}").leak())
        .unwrap();
    assert!(
        vcx.debug_bounds(format!("sidebar-pin-{row}").leak())
            .is_none()
    );
    // No todo progress known yet: nothing in the details line.
    assert!(
        vcx.debug_bounds(format!("sidebar-todos-{row}").leak())
            .is_none()
    );
    assert!(
        vcx.debug_bounds(format!("sidebar-todos-{}", 1 - row).leak())
            .is_none()
    );
    assert!(title.right() <= spin_bounds.left());
    assert!(row_bounds.right() - spin_bounds.right() < px(crate::scrollbar::GUTTER + 12.0));

    // Hovering swaps the spinner for the row actions.
    vcx.simulate_mouse_move(row_bounds.center(), None, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds(spinner(row).leak()).is_none());
    assert!(
        vcx.debug_bounds(format!("sidebar-pin-{row}").leak())
            .is_some()
    );
    vcx.simulate_mouse_move(
        gpui::point(px(2000.0), px(2000.0)),
        None,
        gpui::Modifiers::default(),
    );
    vcx.run_until_parked();
    assert!(vcx.debug_bounds(spinner(row).leak()).is_some());
    // Working rows no longer show a timer: the spinner already says it,
    // and the details line stays compact. Todo progress takes that spot.
    assert!(
        vcx.debug_bounds(format!("sidebar-todos-{row}").leak())
            .is_none()
    );
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_todo_progress
            .insert("background-run".into(), (2, 5));
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds(format!("sidebar-todos-{row}").leak())
            .is_some()
    );
    let todo_icon = vcx
        .debug_bounds(format!("sidebar-todos-icon-{row}").leak())
        .unwrap();
    assert_eq!(todo_icon.size.width, px(9.0));
    assert!(todo_icon.left() + px(16.0) <= title.left());
    assert!(
        vcx.debug_bounds(format!("sidebar-todos-{}", 1 - row).leak())
            .is_none()
    );
    let mark = workspace.read_with(vcx, |workspace, _| {
        workspace
            .daemon_running_mark("background-run")
            .unwrap()
            .entity_id()
    });

    // A repeated poll keeps the same mark so its animation never restarts.
    workspace.update(vcx, |workspace, cx| {
        workspace.set_daemon_running(running(&["background-run", "not-listed"]), cx)
    });
    workspace.read_with(vcx, |workspace, _| {
        assert_eq!(
            workspace
                .daemon_running_mark("background-run")
                .unwrap()
                .entity_id(),
            mark
        );
    });

    workspace.update(vcx, |workspace, cx| {
        workspace.set_daemon_running(running(&[]), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds(spinner(row).leak()).is_none());
}

#[gpui::test]
fn sidebar_mark_shows_only_while_open_session_is_active(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("sidebar-activity", cx);
        workspace.push_test_panel("quiet-session", cx);
        workspace.sidebar_closed_sessions.insert("history".into());
        workspace.sessions = vec![
            session_info("sidebar-activity", Some("Live session")),
            session_info("quiet-session", Some("Idle session")),
            session_info("history", Some("Previous session")),
        ];
        workspace
    });
    vcx.run_until_parked();
    // Idle open sessions show no orb: a frozen mark reads as "running".
    assert!(vcx.debug_bounds("sidebar-session-spinner-0").is_none());
    assert!(vcx.debug_bounds("sidebar-session-spinner-1").is_none());
    assert!(vcx.debug_bounds("sidebar-session-spinner-2").is_none());
    let mark_id = workspace.read_with(vcx, |workspace, cx| {
        workspace.slots[0]
            .panel
            .read(cx)
            .sidebar_mark()
            .unwrap()
            .entity_id()
    });

    for event in [
        jcode_sdk::ApiEvent::ReasoningDelta {
            session_id: "sidebar-activity".into(),
            text: "Thinking".into(),
        },
        jcode_sdk::ApiEvent::TextDelta {
            message_id: None,
            session_id: "sidebar-activity".into(),
            text: "Responding".into(),
        },
    ] {
        workspace.update(vcx, |workspace, cx| {
            for event in [
                jcode_sdk::ApiEvent::SessionStatus {
                    session_id: "sidebar-activity".into(),
                    status: "idle".into(),
                    pending_soft_interrupts: None,
                },
                event,
            ] {
                workspace.apply(
                    Update::Event {
                        session_id: "sidebar-activity".into(),
                        event,
                    },
                    cx,
                );
            }
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("sidebar-session-spinner-0").is_some());
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                Update::Event {
                    session_id: "sidebar-activity".into(),
                    event: jcode_sdk::ApiEvent::TurnDone {
                        session_id: "sidebar-activity".into(),
                        pending_soft_interrupts: None,
                    },
                },
                cx,
            );
            let panel = workspace.slots[0].panel.read(cx);
            assert!(panel.sidebar_activity().is_none());
            assert!(
                panel.tab_ring().is_none(),
                "idle tabs keep their normal outline"
            );
            assert_eq!(panel.sidebar_mark().unwrap().entity_id(), mark_id);
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("sidebar-session-spinner-0").is_none());
        assert!(vcx.debug_bounds("sidebar-session-spinner-2").is_none());
    }
}

/// The sidebar row's metadata line mirrors the TUI `/resume` picker:
/// relative creation time plus an estimated token count.
#[test]
fn sidebar_meta_shows_relative_age_and_estimated_tokens() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut session = session_info(
        &format!("session_fox_{}_deadbeef", now_ms - 5 * 60 * 1_000),
        None,
    );
    session.transcript_bytes = Some(48_000);
    let meta = sidebar_session_meta(&session).expect("meta line");
    let tokens = jcode_base::message::TokenFamily::default().tokens_for_bytes(48_000);
    assert_eq!(
        meta,
        format!("5m ago · {}", format_estimated_tokens(tokens as u64))
    );
}

#[test]
fn sidebar_meta_is_omitted_when_no_metadata_exists() {
    assert_eq!(sidebar_session_meta(&session_info("legacy-id", None)), None);
}

#[test]
fn sidebar_time_ago_matches_the_tui_picker_buckets() {
    let now = 1_800_000_000_000_u64;
    let minute = 60 * 1_000;
    assert_eq!(format_time_ago(now - 30 * 1_000, now), "30s ago");
    assert_eq!(format_time_ago(now - 12 * minute, now), "12m ago");
    assert_eq!(format_time_ago(now - 3 * 60 * minute, now), "3h ago");
    assert_eq!(format_time_ago(now - 2 * 24 * 60 * minute, now), "2d ago");
    assert_eq!(format_time_ago(now - 10 * 24 * 60 * minute, now), "1w ago");
    assert_eq!(format_time_ago(now - 65 * 24 * 60 * minute, now), "2mo ago");
}

#[test]
fn sidebar_token_estimate_uses_tui_style_units() {
    assert_eq!(format_estimated_tokens(500), "500 tok");
    assert_eq!(format_estimated_tokens(4_200), "4.2k tok");
    assert_eq!(format_estimated_tokens(250_000), "250k tok");
    assert_eq!(format_estimated_tokens(1_500_000), "1.5M tok");
}

#[test]
fn sidebar_and_tab_titles_match_for_untitled_and_named_sessions() {
    for id in ["session_fox_1234567890_deadbeef", "opaque-session-deadbeef"] {
        for title in [
            None,
            Some(""),
            Some("  "),
            Some("session deadbeef"),
            Some(id),
        ] {
            let session = session_info(id, title);
            assert_eq!(sidebar_session_title(&session).1, "New session");
            assert_eq!(
                sidebar_session_title(&session).1,
                crate::panel::folder_session_title(id, title.unwrap_or_default()).as_ref(),
            );
        }
    }
    let session = session_info(
        "session_fox_1234567890_deadbeef",
        Some("  Plan\n the   release "),
    );
    assert_eq!(
        sidebar_session_title(&session),
        ("🦊", "Plan the release".into())
    );
    assert_eq!(
        sidebar_session_title(&session).1,
        crate::panel::folder_session_title(
            &session.session_id,
            session.title.as_deref().unwrap()
        )
        .as_ref(),
    );
}

#[test]
fn sidebar_uses_the_open_tabs_title_even_when_catalog_is_stale() {
    let session = session_info("session_fox_1234567890_deadbeef", Some("Old catalog title"));
    for title in [
        "New session",
        "session deadbeef",
        "  New\n prompt title  ",
        "fox",
    ] {
        assert_eq!(
            sidebar_session_title_with_open_title(&session, Some(title)).1,
            crate::panel::folder_session_title(&session.session_id, title).as_ref(),
        );
    }
}

#[test]
fn missing_sidebar_directory_is_omitted_instead_of_rendering_a_placeholder() {
    let mut session = session_info("session_fox_1234567890_deadbeef", None);
    assert_eq!(sidebar_session_directory(&session), None);

    session.working_dir = Some("   ".into());
    assert_eq!(sidebar_session_directory(&session), None);
}

#[gpui::test]
fn persisted_session_record_paints_as_a_sidebar_row(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-sidebar-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    std::fs::write(
        home.join("sessions/session_fox_disk.json"),
        r#"{"working_dir":"/persisted/project","custom_title":"Persisted work"}"#,
    )
    .unwrap();
    let sessions = crate::harness::merge_persisted_sessions(Vec::new(), Some(&home));
    assert_eq!(
        sessions.len(),
        1,
        "the disk record must cross the harness boundary"
    );

    let (workspace, vcx) =
        cx.add_window_view(|_window, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace
            .sidebar_closed_sessions
            .insert("session_fox_disk".into());
        workspace.apply(Update::Sessions { sessions }, cx);
        cx.notify();
    });
    vcx.run_until_parked();

    assert!(
        vcx.debug_bounds("sidebar-session-0").is_some(),
        "an on-disk session must paint through the real sidebar renderer"
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[gpui::test]
fn sidebar_title_lengths_keep_normal_font_size_and_equal_row_heights(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        crate::bind_workspace_keys(cx);
    });
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    workspace.update(vcx, |workspace, cx| {
        workspace.sidebar_closed_sessions.extend([
            "session_fox_short".into(),
            "session_owl_long".into(),
            "session_cat_multiline".into(),
        ]);
        workspace.apply(
            Update::Sessions {
                sessions: vec![
                    session_info("session_fox_short", Some("Short")),
                    session_info("session_owl_long", Some(&"Long title ".repeat(40))),
                    session_info("session_cat_multiline", Some("Title\nwith\nseveral\nlines")),
                ],
            },
            cx,
        );
        cx.notify();
    });
    vcx.run_until_parked();
    // At 12px with 1.5 line height, every title occupies 18px. Long
    // titles must truncate at that size, never shrink to fit the row.
    for selector in [
        "sidebar-session-title-0",
        "sidebar-session-title-1",
        "sidebar-session-title-2",
    ] {
        let title = vcx.debug_bounds(selector).unwrap();
        assert_eq!(title.size.height, px(18.0));
    }
    let short = vcx.debug_bounds("sidebar-session-0").unwrap();
    for selector in ["sidebar-session-1", "sidebar-session-2"] {
        let bounds = vcx.debug_bounds(selector).unwrap();
        assert_eq!(short.size.height, bounds.size.height);
    }
}
