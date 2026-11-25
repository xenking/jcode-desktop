use super::*;
use crate::workspace::tests::session_info;

fn picker(cx: &mut gpui::TestAppContext) -> (Entity<Workspace>, &mut gpui::VisualTestContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("origin", cx);
        let mut alpha = session_info("alpha", Some("Alpha"));
        alpha.updated_at_ms = Some(30);
        let mut beta = session_info("beta", Some("Beta"));
        beta.updated_at_ms = Some(20);
        w.sessions = vec![alpha, beta];
        w
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    (workspace, vcx)
}

fn click(vcx: &mut gpui::VisualTestContext, selector: &'static str) {
    let position = vcx.debug_bounds(selector).expect(selector).center();
    vcx.simulate_click(position, gpui::Modifiers::default());
    vcx.run_until_parked();
}

fn messages() -> Vec<jcode_sdk::HistoryMessage> {
    (0..80)
        .map(|i| jcode_sdk::HistoryMessage {
            attachments: Vec::new(),
            response_stats: None,
            timestamp_ms: None,
            tool: None,
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: if i == 21 {
                "Beta search anchor".into()
            } else {
                format!("History row {i:02}")
            },
        })
        .collect()
}

fn seed_preview(
    workspace: &Entity<Workspace>,
    vcx: &mut gpui::VisualTestContext,
) -> Entity<Panel> {
    let preview = workspace.update(vcx, |w, cx| {
        let state = w.resume.as_mut().unwrap();
        let id = state.preview_id.clone().unwrap();
        let panel = preview_panel(id, messages(), Vec::new(), cx);
        state.preview_task = None;
        state.preview = Some(Ok(panel.clone()));
        cx.notify();
        panel
    });
    vcx.run_until_parked();
    preview
}

fn visible_rows(panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext) -> serde_json::Value {
    panel.update_in(vcx, |panel, window, cx| {
        panel.control_state(window, cx).unwrap()["visible_transcript_items"].clone()
    })
}

thread_local! {
    static OBSERVED_TITLES: std::cell::RefCell<Option<HashMap<String, gpui::TextLayout>>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn observe_title(id: &str, layout: &gpui::TextLayout) {
    OBSERVED_TITLES.with_borrow_mut(|titles| {
        if let Some(titles) = titles {
            titles.insert(id.to_owned(), layout.clone());
        }
    });
}

#[gpui::test]
fn resume_recent_renders_catalog_titles(cx: &mut gpui::TestAppContext) {
    OBSERVED_TITLES.with_borrow_mut(|titles| *titles = Some(HashMap::new()));
    let (workspace, vcx) = picker(cx);
    seed_preview(&workspace, vcx);
    for width in [220., 850.] {
        vcx.simulate_resize(gpui::size(px(width), px(650.)));
        vcx.run_until_parked();
        OBSERVED_TITLES.with_borrow(|titles| {
            let titles = titles.as_ref().unwrap();
            for (id, expected) in [("alpha", "Alpha"), ("beta", "Beta")] {
                let title = titles[id].text();
                assert!(title.ends_with(expected), "at {width}px: {title:?}");
            }
        });
    }
    OBSERVED_TITLES.with_borrow_mut(Option::take);
}

#[gpui::test]
fn resume_click_selects_without_opening_and_keyboard_opens(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = picker(cx);
    click(vcx, "resume-row-1");
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(
            w.resume
                .as_ref()
                .expect("single click keeps browser open")
                .preview_id
                .as_deref(),
            Some("beta")
        );
        assert_eq!(w.slots[w.active].panel.session_id(cx), "origin");
    });
    for (key, target) in [("up", "alpha"), ("down", "beta")] {
        vcx.simulate_keystrokes(key);
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, _| {
            assert_eq!(
                w.resume.as_ref().unwrap().preview_id.as_deref(),
                Some(target)
            )
        });
    }
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert!(!w.resume_open());
        assert_eq!(w.slots[w.active].panel.session_id(cx), "beta");
    });
}

#[gpui::test]
fn resume_opens_by_button_and_double_click_and_reuses_open_panel(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = picker(cx);
    click(vcx, "resume-row-1-action-open");
    let beta = workspace.read_with(vcx, |w, cx| {
        assert!(!w.resume_open());
        let panel = w.slots[w.active].panel.clone();
        assert_eq!(panel.read(cx).session_id, "beta");
        panel
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    let position = vcx.debug_bounds("resume-row-0").unwrap().center();
    for click_count in [1, 2] {
        vcx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        });
        vcx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        });
    }
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert!(!w.resume_open());
        assert_eq!(w.slots[w.active].panel.session_id(cx), "alpha");
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("resume-row-1-open").is_some());
    click(vcx, "resume-row-1-action-go-to");
    workspace.read_with(vcx, |w, cx| {
        assert!(!w.resume_open());
        assert_eq!(w.slots[w.active].panel, beta);
        assert_eq!(
            w.slots
                .iter()
                .filter(|slot| slot.panel.session_id(cx) == "beta")
                .count(),
            1
        );
    });
}

#[gpui::test]
fn resume_preview_scrolls_with_tab_keys_and_wheel_without_changing_selection(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = picker(cx);
    let preview = seed_preview(&workspace, vcx);
    let initial = visible_rows(&preview, vcx);
    assert!(!initial.as_array().unwrap().is_empty());
    vcx.simulate_keystrokes("tab pageup");
    vcx.run_until_parked();
    let scrolled = visible_rows(&preview, vcx);
    assert_ne!(scrolled, initial, "Tab then PageUp scrolls the preview");
    let position = vcx.debug_bounds("transcript").unwrap().center();
    vcx.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(250.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    for _ in 0..16 {
        vcx.executor().advance_clock(Duration::from_millis(16));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    assert_ne!(
        visible_rows(&preview, vcx),
        scrolled,
        "wheel also scrolls preview"
    );
    workspace.read_with(vcx, |w, _| {
        assert_eq!(
            w.resume.as_ref().unwrap().selected.as_deref(),
            Some("alpha")
        )
    });
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!workspace.read_with(vcx, |w, _| w.resume_open()));
}

#[gpui::test]
fn resume_metadata_query_reveals_first_transcript_match_in_preview_and_panel(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = picker(cx);
    workspace.update(vcx, |w, cx| {
        w.push_test_panel("beta", cx);
        w.slots.last().unwrap().panel.update(cx, |panel, cx| {
            panel.load_history(messages(), Vec::new(), cx)
        });
    });
    vcx.simulate_input("Beta");
    vcx.run_until_parked();
    let preview = seed_preview(&workspace, vcx);
    let marks = preview.read_with(vcx, |panel, _| panel.search_terms.clone());
    assert_eq!(marks, ["beta"]);
    assert!(
        visible_rows(&preview, vcx)
            .to_string()
            .contains("Beta search anchor")
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let panel = workspace.read_with(vcx, |w, cx| {
        assert!(!w.resume_open());
        let panel = w.slots[w.active].panel.chat().cloned().unwrap();
        assert_eq!(panel.read(cx).session_id, "beta");
        assert_eq!(panel.read(cx).search_terms, marks);
        panel
    });
    assert!(
        visible_rows(&panel, vcx)
            .to_string()
            .contains("Beta search anchor")
    );
}

fn roles_and_text(history: &PreviewHistory) -> Vec<(String, String)> {
    history
        .0
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect()
}

#[test]
fn resume_preview_uses_full_chat_history_shape() {
    use jcode_base::message::{ContentBlock, Role};
    let mut session =
        jcode_base::session::Session::create_with_id("preview-test".into(), None, None);
    for index in 0..24 {
        session.add_message(
            if index % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            vec![ContentBlock::Text {
                text: format!("Actual message {index}"),
                cache_control: None,
            }],
        );
    }
    let history = crate::persisted_history::render(&session);
    // Identical to the server History payload: no truncation or clipping.
    let expected: Vec<_> = jcode_base::session::render_messages(&session)
        .into_iter()
        .map(|message| (message.role, message.content))
        .collect();
    assert_eq!(roles_and_text(&history), expected);
    assert_eq!(history.0.len(), 24);
}

#[gpui::test]
fn resume_preview_renders_through_the_chat_panel_without_composer(
    cx: &mut gpui::TestAppContext,
) {
    let message = |role: &str, content: &str| jcode_sdk::HistoryMessage {
        attachments: Vec::new(),
        response_stats: None,
        timestamp_ms: None,
        tool: None,
        role: role.into(),
        content: content.into(),
    };
    let (host, vcx) = cx.add_window_view(|_, cx| {
        let panel = preview_panel(
            "abc".into(),
            vec![
                message("user", "question"),
                message("assistant", "**answer**"),
            ],
            Vec::new(),
            cx,
        );
        PanelHost(panel)
    });
    vcx.run_until_parked();
    host.read_with(vcx, |host, cx| {
        let panel = host.0.read(cx);
        assert!(panel.transcript_only);
        assert!(panel.history_loaded());
        assert!(matches!(&panel.items[..], [crate::panel::Item::User(q), crate::panel::Item::Assistant(a)]
            if q == "question" && a == "**answer**"));
    });
    assert!(vcx.debug_bounds("panel-meta").is_none());
    assert!(vcx.debug_bounds("fresh-session").is_none());
}

struct PanelHost(Entity<Panel>);
impl Render for PanelHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[test]
fn resume_preview_loads_persisted_messages_without_modifying_storage() {
    use jcode_base::message::{ContentBlock, Role};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preview.json");
    let mut session =
        jcode_base::session::Session::create_with_id("persisted-preview".into(), None, None);
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "Persisted user question".into(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "Persisted assistant answer".into(),
            cache_control: None,
        }],
    );
    let bytes = serde_json::to_vec(&session).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(
        roles_and_text(&crate::persisted_history::load_path(&path).unwrap()),
        vec![
            ("user".into(), "Persisted user question".into()),
            ("assistant".into(), "Persisted assistant answer".into()),
        ]
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    assert!(
        crate::persisted_history::load_path(&directory.path().join("missing.json")).is_err()
    );
    std::fs::write(&path, b"malformed").unwrap();
    assert!(crate::persisted_history::load_path(&path).is_err());
}

#[test]
fn resume_preview_rejects_nonlocal_paths() {
    for id in [
        "",
        "../secret",
        "/etc/passwd",
        "remote://host/session",
        "..",
        "foo\\bar",
    ] {
        assert!(load_preview_history(id).is_err(), "{id}");
    }
}

#[test]
fn resume_pasted_session_id_is_recognized() {
    let id = "session_orangutan_1791475004393_3e578ce05d8b6081";
    for q in [
        id.to_string(),
        format!(" `{id}` "),
        format!("jcode --resume {id}."),
        format!("--resume {id}"),
    ] {
        assert_eq!(pasted_session_id(&q).as_deref(), Some(id), "{q}");
    }
    for q in ["orangutan", "session_", "session_a b", "planner session_a"] {
        assert_eq!(pasted_session_id(q), None, "{q}");
    }
}

#[test]
fn resume_search_matches_save_label() {
    let mut labelled = session_info("session_labelled", Some("Fundraising catch-up"));
    labelled.saved = true;
    labelled.save_label = Some("Investor Catch Up Work".into());
    let other = session_info("session_other", Some("Other"));
    let matches = filtered_sessions(
        vec![labelled, other],
        "investor work",
        Filter::All,
        &HashSet::new(),
        &[],
        Default::default(),
    );
    assert_eq!(
        matches
            .iter()
            .map(|s| s.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["session_labelled"]
    );
}

#[test]
fn resume_search_is_case_insensitive_multiword_and_handles_unicode() {
    let mut alpha = session_info("alpha-123", Some("Résumé planner"));
    alpha.working_dir = Some("/workspace/Client".into());
    let beta = session_info("beta", Some("Other task"));
    let sessions = vec![alpha.clone(), beta];
    for query in ["  RÉSUMÉ   client ", "alpha-123", "PLANNER"] {
        assert_eq!(
            filtered_sessions(
                sessions.clone(),
                query,
                Filter::All,
                &HashSet::new(),
                &[],
                Default::default()
            ),
            vec![alpha.clone()]
        );
    }
    assert!(
        filtered_sessions(
            sessions,
            "not-found",
            Filter::All,
            &HashSet::new(),
            &[],
            Default::default()
        )
        .is_empty()
    );
}

#[test]
fn resume_filters_sort_saved_then_recent_exclude_archived_and_deduplicate() {
    let mut saved = session_info("saved", Some("Saved"));
    saved.saved = true;
    saved.updated_at_ms = Some(1);
    let mut running = session_info("running", None);
    running.status = "running".into();
    running.updated_at_ms = Some(20);
    let mut duplicate = running.clone();
    duplicate.updated_at_ms = Some(2);
    let mut archived = session_info("archived", None);
    archived.archived = true;
    let sessions = vec![duplicate, saved.clone(), archived, running.clone()];
    assert_eq!(
        filtered_sessions(
            sessions.clone(),
            "",
            Filter::All,
            &HashSet::new(),
            &[],
            Default::default()
        ),
        vec![saved.clone(), running.clone()]
    );
    assert_eq!(
        filtered_sessions(
            sessions.clone(),
            "",
            Filter::Active,
            &HashSet::new(),
            &[],
            Default::default()
        ),
        vec![running]
    );
    assert_eq!(
        filtered_sessions(
            sessions,
            "",
            Filter::Saved,
            &HashSet::new(),
            &[],
            Default::default()
        ),
        vec![saved]
    );
    assert!(is_resume_command(" /session \n"));
    assert!(is_resume_command("/resume"));
    assert!(is_resume_command("/sessions"));
    assert!(!is_resume_command("/resumeall"));
    assert!(!is_resume_command("/resume unrelated"));
}

#[test]
fn resume_active_filter_includes_daemon_streaming_sessions() {
    let idle = session_info("idle", None);
    let streaming = session_info("streaming", None);
    let working = HashSet::from(["streaming".to_string()]);
    assert_eq!(
        filtered_sessions(
            vec![idle, streaming.clone()],
            "",
            Filter::Active,
            &working,
            &[],
            Default::default(),
        ),
        vec![streaming]
    );
}

#[test]
fn resume_search_includes_transcript_hits_after_metadata_hits() {
    let mut titled = session_info("titled", Some("Kubernetes rollout"));
    titled.updated_at_ms = Some(1);
    let mut transcript = session_info("transcript", Some("Unrelated title"));
    transcript.updated_at_ms = Some(50);
    let other = session_info("other", Some("Other"));
    let mut second = session_info("second", Some("Unrelated too"));
    second.updated_at_ms = Some(99);
    // The index's relevance order, not recency, ranks transcript-only hits.
    let content = vec![
        "transcript".to_string(),
        "second".to_string(),
        "titled".to_string(),
    ];
    let ids = |content: &[String]| {
        filtered_sessions(
            vec![
                other.clone(),
                second.clone(),
                transcript.clone(),
                titled.clone(),
            ],
            "kubernetes",
            Filter::All,
            &HashSet::new(),
            content,
            Default::default(),
        )
        .into_iter()
        .map(|s| s.session_id)
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(&content), vec!["titled", "transcript", "second"]);
    assert_eq!(ids(&[]), vec!["titled"]);
}

#[gpui::test]
fn resume_rows_mark_working_and_open_sessions(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.single_panel = true;
        w.open_startup_draft(cx);
        let mut busy = session_info("busy", Some("Busy"));
        busy.updated_at_ms = Some(30);
        let mut quiet = session_info("quiet", Some("Quiet"));
        quiet.updated_at_ms = Some(20);
        let mut remote = session_info("remote", Some("Remote"));
        remote.updated_at_ms = Some(10);
        w.sessions = vec![busy, quiet, remote];
        w
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    // Let the first presence poll land before injecting daemon state.
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.resume.as_mut().unwrap().streaming = HashSet::from(["remote".to_string()]);
        cx.notify();
    });
    vcx.run_until_parked();
    let rows = workspace.read_with(vcx, |w, cx| {
        w.resume_matches(cx)
            .into_iter()
            .map(|s| s.session_id)
            .collect::<Vec<_>>()
    });
    let remote = rows.iter().position(|id| id == "remote").unwrap();
    let quiet = rows.iter().position(|id| id == "quiet").unwrap();
    assert!(
        vcx.debug_bounds(format!("resume-row-{remote}-working").leak())
            .is_some()
    );
    assert!(
        vcx.debug_bounds(format!("resume-row-{quiet}-working").leak())
            .is_none()
    );
    assert!(
        vcx.debug_bounds(format!("resume-row-{quiet}-open").leak())
            .is_none()
    );
    // The local startup draft is open in this window but idle.
    let draft = rows
        .iter()
        .position(|id| !["busy", "quiet", "remote"].contains(&id.as_str()));
    if let Some(draft) = draft {
        assert!(
            vcx.debug_bounds(format!("resume-row-{draft}-open").leak())
                .is_some()
        );
    }
}

fn reload_picker(w: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let snapshot = w.snapshot_for_reload(window, cx).unwrap();
    let snapshot = WorkspaceSnapshot::decode(&snapshot.encode().unwrap()).unwrap();
    let picker = snapshot.resume.clone();
    w.resume = None;
    w.apply_snapshot(snapshot, cx);
    w.restore_resume(Some(picker), true, window, cx);
    w.restore_focus(window, cx);
}

#[test]
fn resume_activity_age_ignores_creation_and_metadata_edits() {
    use jcode_sdk::api::{SessionCatalogActivity, SessionCatalogContext, SessionOrigin};
    let now = 1_795_505_400_000;
    let sort = sidebar_session_catalog::Sort::AgentActivity;
    let mut session = session_info("session_dragon_1795408406006_fixture", Some("Work"));
    session.updated_at_ms = Some(now - 1_000);
    session.last_active_at_ms = Some(now - 2_000);
    session.transcript_bytes = Some(16_800);
    assert_eq!(
        resume_usage(&session, sort, now).as_deref(),
        Some("4.2k tok")
    );
    session.context = Some(SessionCatalogContext {
        activity: Some(SessionCatalogActivity {
            origin: SessionOrigin::User,
            last_agent_activity_at_ms: Some(now - 65_000),
            last_message_at_ms: None,
        }),
        ..Default::default()
    });
    assert_eq!(
        resume_usage(&session, sort, now).as_deref(),
        Some("1m ago · 4.2k tok")
    );
    let activity = |session: &mut jcode_sdk::SessionInfo, timestamp| {
        session
            .context
            .as_mut()
            .unwrap()
            .activity
            .as_mut()
            .unwrap()
            .last_message_at_ms = Some(timestamp);
    };
    activity(&mut session, Some(now - 5_000));
    assert_eq!(
        resume_usage(&session, sort, now).as_deref(),
        Some("5s ago · 4.2k tok")
    );
    activity(&mut session, None);
    assert_eq!(
        resume_usage(&session, sort, now).as_deref(),
        Some("4.2k tok")
    );
}

#[gpui::test]
fn session_catalog_reorders_when_message_activity_changes(cx: &mut gpui::TestAppContext) {
    use jcode_sdk::api::{SessionCatalogActivity, SessionCatalogContext, SessionOrigin};
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.open_startup_draft(cx);
        w.sessions = [
            ("updated", 100, Some(10), None, SessionOrigin::User),
            (
                "answered",
                40,
                Some(30),
                Some(Some(30)),
                SessionOrigin::Agent,
            ),
            (
                "unanswered",
                200,
                None,
                Some(Some(40)),
                SessionOrigin::Agent,
            ),
        ]
        .into_iter()
        .map(|(id, updated, replied, last_message, origin)| {
            let mut session = session_info(id, Some(id));
            session.updated_at_ms = Some(updated);
            session.saved = id == "updated";
            session.context = Some(SessionCatalogContext {
                activity: Some(SessionCatalogActivity {
                    origin,
                    last_agent_activity_at_ms: replied,
                    last_message_at_ms: last_message,
                }),
                ..Default::default()
            });
            session
        })
        .collect();
        w
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    let ids = |w: &Workspace, cx: &App| {
        w.resume_matches(cx)
            .into_iter()
            .map(|s| s.session_id)
            .filter(|id| ["updated", "answered", "unanswered"].contains(&id.as_str()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        workspace.read_with(vcx, ids),
        ["updated", "unanswered", "answered"]
    );
    let control = vcx
        .debug_bounds("resume-sort-agent-activity")
        .expect("Agent activity sort must be selectable");
    vcx.simulate_click(control.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(
        workspace.read_with(vcx, ids),
        ["unanswered", "answered", "updated"]
    );
    assert!(vcx.debug_bounds("resume-agent-origin-0").is_some());
    assert!(vcx.debug_bounds("resume-agent-origin-2").is_none());

    workspace.update(vcx, |w, cx| {
        let mut sessions = w.sessions.clone();
        let row = sessions
            .iter_mut()
            .find(|s| s.session_id == "updated")
            .unwrap();
        let context = row.context.as_mut().unwrap();
        context.activity.as_mut().unwrap().last_message_at_ms = Some(Some(50));
        assert!(w.apply(harness::Update::Sessions { sessions }, cx));
        cx.notify();
    });
    vcx.run_until_parked();
    assert_eq!(
        workspace.read_with(vcx, ids),
        ["updated", "unanswered", "answered"]
    );
    assert!(vcx.debug_bounds("resume-agent-origin-0").is_none());
}

#[gpui::test]
fn session_catalog_sort_and_origin_filter_survive_window_snapshot(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.open_startup_draft(cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    for id in ["resume-sort-agent-activity", "resume-origin-agents"] {
        let control = vcx.debug_bounds(id).expect(id);
        vcx.simulate_click(control.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
    }
    let encoded = workspace.update_in(vcx, |w, window, cx| {
        w.snapshot_for_reload(window, cx).unwrap().encode().unwrap()
    });
    let snapshot = WorkspaceSnapshot::decode(&encoded).unwrap();
    for id in ["resume-sort-recent", "resume-origin-all"] {
        let control = vcx.debug_bounds(id).expect(id);
        vcx.simulate_click(control.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
    }
    workspace.update_in(vcx, |w, window, cx| {
        let picker = snapshot.resume.clone();
        w.apply_snapshot(snapshot, cx);
        w.restore_resume(Some(picker), true, window, cx);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("resume-sort-agent-activity-selected")
            .is_some()
    );
    assert!(vcx.debug_bounds("resume-origin-agents-selected").is_some());
}

#[gpui::test]
fn resume_reload_roundtrip_preserves_search_selection_and_suppresses_connected(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.bridge = bridge;
        w.open_startup_draft(cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.open_resume(&OpenResume, window, cx);
        assert!(matches!(commands.try_recv(), Ok(Command::RefreshSessions)));
        let state = w.resume.as_mut().unwrap();
        state.search.update(cx, |input, cx| {
            let mut search = input.snapshot();
            search.content = "Résumé planner".into();
            search.selection_start = 2;
            search.selection_end = 5;
            search.selection_reversed = true;
            input.restore(search, cx);
        });
        state.query = "Résumé planner".into();
        state.selected = Some("selected-session".into());
        state.filter = Filter::Saved;
        state.start_on_close = true;
        let expected = w.resume_snapshot(cx);
        reload_picker(w, window, cx);
        assert!(matches!(commands.try_recv(), Ok(Command::RefreshSessions)));
        assert_eq!(w.resume_snapshot(cx), expected);
        assert!(
            w.resume
                .as_ref()
                .unwrap()
                .search
                .focus_handle(cx)
                .is_focused(window)
        );
        w.apply(harness::Update::Connected, cx);
        assert_eq!(w.resume_snapshot(cx), expected);
        assert!(
            commands.try_recv().is_err(),
            "reload must not create or attach a session"
        );
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| {
        assert_eq!(
            w.resume.as_ref().unwrap().selected.as_deref(),
            Some("selected-session")
        );
    });
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn resume_reload_cancelled_picker_stays_closed(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.open_startup_draft(cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.restore_resume(None, true, window, cx);
        w.close_resume(window, cx);
        assert!(
            w.slots[w.active]
                .panel
                .read_chat(cx)
                .is_some_and(Panel::is_startup_draft)
        );
        assert_eq!(w.resume_snapshot(cx), Snapshot::Closed);
        reload_picker(w, window, cx);
        assert!(
            w.resume.is_none(),
            "--resume must not reopen an intentionally closed picker"
        );
    });
}

#[gpui::test]
fn resume_reload_selected_picker_stays_closed(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.bridge = bridge;
        w.open_startup_draft(cx);
        w.sessions = vec![session_info("target", Some("Target"))];
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.restore_resume(None, true, window, cx);
        w.resume.as_mut().unwrap().selected = Some("target".into());
        w.resume_selected(window, cx);
        reload_picker(w, window, cx);
        assert!(w.resume.is_none());
        assert_eq!(w.slots[w.active].panel.session_id(cx), "target");
        while commands.try_recv().is_ok() {}
        w.apply(harness::Update::Connected, cx);
        assert!(
            commands.try_recv().is_err(),
            "must not start the hidden startup draft"
        );
    });
}

#[gpui::test]
fn resume_reload_legacy_recovers_only_requested_active_startup(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.bridge = bridge;
        w.open_startup_draft(cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        let mut json =
            serde_json::to_value(w.snapshot_for_reload(window, cx).unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("resume");
        let legacy = WorkspaceSnapshot::decode(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(legacy.resume, Snapshot::Legacy);
        w.apply_snapshot(legacy, cx);
        w.restore_resume(Some(Snapshot::Legacy), false, window, cx);
        assert!(w.resume.is_none());
        w.restore_resume(Some(Snapshot::Legacy), true, window, cx);
        assert!(matches!(commands.try_recv(), Ok(Command::RefreshSessions)));
        assert!(w.resume.as_ref().unwrap().start_on_close);
        assert!(
            w.resume
                .as_ref()
                .unwrap()
                .search
                .focus_handle(cx)
                .is_focused(window)
        );
        w.apply(harness::Update::Connected, cx);
        assert!(commands.try_recv().is_err());
        w.sessions = vec![session_info("target", Some("Target"))];
        w.resume.as_mut().unwrap().selected = Some("target".into());
        w.resume_selected(window, cx);
        w.restore_resume(Some(Snapshot::Legacy), true, window, cx);
        assert!(
            w.resume.is_none(),
            "legacy --resume must not replace an active chat"
        );
    });
}

#[gpui::test]
fn resume_startup_connected_does_not_create_until_cancel(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.single_panel = true;
        w.bridge = bridge;
        w.open_startup_draft(cx);
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.open_resume(&OpenResume, window, cx);
        assert!(matches!(commands.try_recv(), Ok(Command::RefreshSessions)));
        w.apply(harness::Update::Connected, cx);
        assert!(
            commands.try_recv().is_err(),
            "browsing must not create or attach a session"
        );
        w.close_resume(window, cx);
        assert!(
            commands.try_recv().is_err(),
            "cancel returns to a draft that waits for its first prompt"
        );
        assert!(w.slots[0].panel.read_chat(cx).is_some_and(Panel::session_deferred));
        assert_eq!(w.request_deferred_sessions(cx), 1);
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::CreateSession { request_id: Some(id), .. }) if id == Panel::STARTUP_SESSION_ID
        ));
    });
}

#[gpui::test]
fn resume_selection_before_connection_does_not_create_hidden_startup_draft(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.single_panel = true;
        w.bridge = bridge;
        w.open_startup_draft(cx);
        w.sessions = vec![session_info("target", Some("Existing conversation"))];
        w
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.open_resume(&OpenResume, window, cx);
        w.resume.as_mut().unwrap().selected = Some("target".into());
        w.resume_selected(window, cx);
        while commands.try_recv().is_ok() {}
        w.apply(harness::Update::Connected, cx);
        assert!(
            commands.try_recv().is_err(),
            "late connection must not create hidden startup draft"
        );
    });
}

#[gpui::test]
fn resume_cancel_preserves_draft_and_never_watches_a_picker(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.single_panel = true;
        workspace.bridge = bridge;
        workspace.open_startup_draft(cx);
        workspace
    });
    workspace.update_in(vcx, |w, window, cx| {
        let input = w.slots[0].panel.read(cx).input.clone();
        input.update(cx, |input, cx| {
            let mut draft = input.snapshot();
            draft.content = "keep this composer draft".into();
            input.restore(draft, cx);
        });
        w.open_resume(&OpenResume, window, cx);
    });
    assert!(matches!(commands.try_recv(), Ok(Command::RefreshSessions)));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("resume-panel").is_some());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert!(w.resume.is_none());
        assert_eq!(w.slots.len(), 1);
        assert_eq!(
            w.slots[0].panel.read(cx).input.read(cx).snapshot().content,
            "keep this composer draft"
        );
    });
    assert!(
        commands.try_recv().is_err(),
        "picker must not Watch/Unwatch a runtime session"
    );
}

#[gpui::test]
fn resume_single_panel_reuses_entities_preserves_drafts_and_primary_anchor(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.single_panel = true;
        w.open_startup_draft(cx);
        w.sessions = vec![session_info("target", Some("Resumed target"))];
        w
    });
    let original = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
    original.update(vcx, |panel, cx| {
        panel.input.update(cx, |input, cx| {
            let mut draft = input.snapshot();
            draft.content = "Unsent draft before switching".into();
            input.restore(draft, cx);
        });
    });
    workspace.update_in(vcx, |w, window, cx| {
        w.open_resume(&OpenResume, window, cx);
        w.resume.as_mut().unwrap().selected = Some("target".into());
        w.resume_selected(window, cx);
        assert!(w.single_panel);
        assert_eq!(w.active, 0);
        assert_eq!(w.slots[0].panel.session_id(cx), "target");
        assert_eq!(w.slots.len(), 2);
        assert!(
            w.resume_sessions(cx)
                .iter()
                .any(|s| s.session_id == original.read(cx).session_id)
        );
        let target = w.slots[0].panel.clone();
        w.open_resume(&OpenResume, window, cx);
        w.resume.as_mut().unwrap().selected = Some("target".into());
        w.resume_selected(window, cx);
        assert_eq!(w.slots[0].panel, target);
        assert_eq!(w.slots.len(), 2);
        w.open_resume(&OpenResume, window, cx);
        w.resume.as_mut().unwrap().selected = Some(original.read(cx).session_id.clone());
        w.resume_selected(window, cx);
        assert_eq!(w.slots[0].panel, original);
        assert_eq!(
            original.read(cx).input.read(cx).snapshot().content,
            "Unsent draft before switching"
        );
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("single-panel-root").is_some());
    assert!(vcx.debug_bounds("single-panel-back").is_none());
    assert!(vcx.debug_bounds("sidebar").is_none());
    assert!(vcx.debug_bounds("workspace-canvas").is_none());
}

#[gpui::test]
fn resume_keyboard_navigation_search_empty_and_catalog_refresh_are_safe(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.single_panel = true;
        w.open_startup_draft(cx);
        w.sessions = vec![
            session_info("a", Some("Alpha")),
            session_info("b", Some("Beta")),
        ];
        w
    });
    workspace.update_in(vcx, |w, window, cx| w.open_resume(&OpenResume, window, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("down");
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.resume.as_ref().unwrap().selected.as_deref(), Some("b"))
    });
    vcx.simulate_keystrokes("up");
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.resume.as_ref().unwrap().selected.as_deref(), Some("a"))
    });
    vcx.simulate_input("no such session");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("pagedown enter");
    workspace.read_with(vcx, |w, _| assert!(w.resume.is_some()));
    vcx.simulate_keystrokes("ctrl-a");
    vcx.simulate_input("Beta");
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.sessions.clear();
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    workspace.read_with(vcx, |w, _| {
        assert!(
            w.resume.is_some(),
            "stale selection must not resume removed session"
        )
    });
}
#[gpui::test]
fn resume_aliases_open_locally_from_startup_in_both_modes(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    for single_panel in [false, true] {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.single_panel = single_panel;
            w.open_startup_draft(cx);
            w
        });
        for command in ["/resume", "/sessions", "/session"] {
            workspace.update_in(vcx, |w, window, cx| w.focus_active(window, cx));
            vcx.run_until_parked();
            vcx.simulate_input(command);
            vcx.simulate_keystrokes("enter");
            vcx.simulate_keystrokes("enter");
            vcx.run_until_parked();
            workspace.read_with(vcx, |w, _| {
                assert!(w.resume.is_some(), "{command} should open locally");
                assert_eq!(w.single_panel, single_panel);
            });
            vcx.simulate_keystrokes("escape");
            vcx.run_until_parked();
        }
    }
}
