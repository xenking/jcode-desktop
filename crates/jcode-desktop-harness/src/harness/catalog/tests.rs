//! Catalog tests: persisted sessions, todos and swarm nesting.

use super::*;

use jcode_sdk::SessionInfo;
use std::time::Duration;

#[test]
fn todo_progress_counts_completed_over_total() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("todos")).unwrap();
    std::fs::write(
        home.path().join("todos/with.json"),
        r#"[{"content":"a","status":"completed"},{"content":"b","status":"in_progress"},{"content":"c","status":"pending"}]"#,
    )
    .unwrap();
    std::fs::write(home.path().join("todos/empty.json"), "[]").unwrap();
    let session = |id: &str| {
        serde_json::from_value::<SessionInfo>(
            serde_json::json!({"session_id": id, "status": "idle"}),
        )
        .unwrap()
    };
    let progress = todo_progress_in(
        home.path(),
        &[session("with"), session("empty"), session("missing")],
    );
    assert_eq!(progress.len(), 1);
    assert_eq!(progress.get("with"), Some(&(1, 3)));
}

fn session_info(id: &str) -> SessionInfo {
    SessionInfo {
        user_connected: false,
        session_id: id.into(),
        working_dir: None,
        title: None,
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
    }
}

/// Real files copied from the user's `~/.jcode`: the coordinator
/// `session_microbe_...` with 45 daemon-spawned workers. Only the
/// per-session metadata and the swarm snapshot are kept, not transcripts.
fn microbe_home() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/microbe_home")
}

const MICROBE: &str = "session_microbe_1794670947943_2648f3bbec7c7e5e";

#[test]
fn real_coordinator_workers_nest_under_it_with_swarm_status() {
    let home = microbe_home();
    let mut sessions = merge_persisted_sessions(Vec::new(), Some(&home));
    jcode_sdk::enrich_sessions_from_swarm_state(&mut sessions, home.join("state/swarm"));
    let by_id = |id: &str| {
        sessions
            .iter()
            .find(|s| s.session_id.starts_with(id))
            .unwrap_or_else(|| panic!("{id} missing"))
    };
    let workers = sessions
        .iter()
        .filter(|s| s.parent_session_id.as_deref() == Some(MICROBE))
        .count();
    assert_eq!(
        workers, 45,
        "every spawned worker is owned by the coordinator"
    );
    // The coordinator itself is a user session, not nested under its own
    // transcript parent.
    assert_eq!(by_id(MICROBE).parent_session_id, None);
    // Live swarm snapshot status wins over the transcript lifecycle.
    assert_eq!(
        by_id("session_badger").swarm_status.as_deref(),
        Some("running")
    );
    assert_eq!(by_id("session_frog").swarm_status.as_deref(), Some("ready"));
    assert_eq!(
        by_id("session_crab").agent_label.as_deref(),
        Some("queued-msg-wait")
    );
    // Workers absent from the snapshot keep no swarm status.
    assert_eq!(by_id("session_rose").swarm_status, None);
}

#[test]
fn transcript_forks_do_not_nest_as_swarm_workers() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("sessions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fork.json"),
        r#"{"parent_id":"root","is_debug":false,"status":"Closed"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("worker.json"),
        r#"{"parent_id":"root","is_debug":true,"status":"Active","last_pid":999999}"#,
    )
    .unwrap();
    let sessions = merge_persisted_sessions(Vec::new(), Some(home.path()));
    let parent = |id: &str| {
        sessions
            .iter()
            .find(|s| s.session_id == id)
            .unwrap()
            .parent_session_id
            .clone()
    };
    assert_eq!(parent("fork"), None);
    assert_eq!(parent("worker").as_deref(), Some("root"));
}

#[test]
fn sidebar_keeps_side_topics_private_until_promotion() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    let visibility = home.path().join("state/side-question-visibility");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::create_dir_all(&visibility).unwrap();
    let child = r#"{"parent_id":"root","hidden":true,"status":"Closed"}"#;
    for id in ["private-api", "private-disk", "legacy", "promoted"] {
        std::fs::write(sessions.join(format!("{id}.json")), child).unwrap();
    }
    for (id, value) in [
        ("private-api", "true"),
        ("private-disk", "true"),
        ("promoted", "false"),
    ] {
        std::fs::write(visibility.join(format!("{id}.json")), value).unwrap();
    }
    let mut promoted = session_info("promoted");
    promoted.parent_session_id = Some("root".to_string());
    promoted.swarm_status = Some("ready".to_string());
    let rows = merge_persisted_sessions(
        vec![session_info("root"), session_info("private-api"), promoted],
        Some(home.path()),
    );
    let mut ids: Vec<_> = rows.iter().map(|row| row.session_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["promoted", "root"]);
    let promoted = rows
        .iter()
        .find(|row| row.session_id == "promoted")
        .unwrap();
    assert_eq!(promoted.parent_session_id, None);
    assert_eq!(promoted.swarm_status, None);
}

#[test]
fn large_worker_transcripts_still_report_their_coordinator() {
    let path = std::env::temp_dir().join(format!("jcode-worker-large-{}.json", std::process::id()));
    let padding = "x".repeat(SIDEBAR_METADATA_WINDOW * 3);
    let document = format!(
        r#"{{"id":"w","parent_id":"root","title":"t","messages":[{{"text":"{padding}"}}],"working_dir":"/p","status":"Closed","is_debug":true,"saved":false}}"#
    );
    std::fs::write(&path, &document).unwrap();
    let record = read_persisted_session(&path, document.len() as u64).unwrap();
    assert_eq!(record.swarm_parent().as_deref(), Some("root"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sdk_connection_presence_survives_lifecycle_enrichment() {
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join("sessions");
    std::fs::create_dir_all(&directory).unwrap();
    for id in ["connected", "loaded-history", "disk-only"] {
        std::fs::write(
            directory.join(format!("{id}.json")),
            serde_json::json!({
                "title": id,
                "status": "Active",
                "last_pid": std::process::id(),
            })
            .to_string(),
        )
        .unwrap();
    }

    // Exercise both compatibility disk enrichment and the authoritative
    // full-page path. A daemon-owned history record is not a user client.
    for full_page in [false, true] {
        let mut connected = session_info("connected");
        connected.user_connected = true;
        let mut sessions = vec![connected, session_info("loaded-history")];
        if full_page {
            for index in sessions.len()..MAX_PERSISTED_SIDEBAR_SESSIONS {
                sessions.push(session_info(&format!("filler-{index}")));
            }
        }
        let merged = merge_persisted_sessions(sessions, Some(home.path()));
        let connected = merged
            .iter()
            .find(|entry| entry.session_id == "connected")
            .unwrap();
        assert!(connected.user_connected);
        assert_eq!(connected.status, "active");
        let history = merged
            .iter()
            .find(|entry| entry.session_id == "loaded-history")
            .unwrap();
        assert!(!history.user_connected);
        assert_eq!(history.status, "active");
        if !full_page {
            let disk = merged
                .iter()
                .find(|entry| entry.session_id == "disk-only")
                .unwrap();
            assert!(!disk.user_connected);
            assert_eq!(disk.status, "active");
        }
    }
}

#[test]
fn persisted_status_handles_unit_and_detail_variants() {
    assert_eq!(
        persisted_session_status(&serde_json::json!("Closed")),
        "closed"
    );
    assert_eq!(
        persisted_session_status(&serde_json::json!({
            "Crashed": { "message": "process exited" }
        })),
        "crashed"
    );
    assert_eq!(persisted_session_status(&serde_json::json!(null)), "idle");
}

#[test]
fn active_record_with_dead_owner_is_reported_as_crashed() {
    let record = |last_pid| PersistedSession {
        working_dir: None,
        title: None,
        custom_title: None,
        model: None,
        reasoning_effort: None,
        pinned_skills: Vec::new(),
        pinned_skill: None,
        saved: false,
        save_label: None,
        status: serde_json::json!("Active"),
        last_pid,
        parent_id: None,
        is_debug: false,
        agent_started: false,
        is_swarm_worker: false,
        side_question_message_start: None,
        hidden: false,
    };
    // A pid beyond the kernel's pid_max can never be alive.
    assert_eq!(record(Some(u32::MAX - 1)).lifecycle_status(), "crashed");
    assert_eq!(
        record(Some(std::process::id())).lifecycle_status(),
        "active"
    );
    assert_eq!(record(None).lifecycle_status(), "active");
}

#[test]
fn session_recency_uses_timestamp_from_modern_and_legacy_ids() {
    assert_eq!(
        session_recency_ms("session_wolf_1787082160300_cab86a9cb334fa3f"),
        Some(1_787_082_160_300)
    );
    assert_eq!(
        session_recency_ms("session_1768160401354_2233921634250370970"),
        Some(1_768_160_401_354)
    );
    assert_eq!(session_recency_ms("legacy-name"), None);
}

#[test]
fn persisted_sessions_fill_sidebar_in_last_interaction_order_without_duplicates_or_archives() {
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-sessions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sessions_dir = home.join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    std::fs::write(
        sessions_dir.join("older.json"),
        r#"{"working_dir":"/old","title":"Old title"}"#,
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(
        sessions_dir.join("newer.json"),
        r#"{"working_dir":"/new","title":"Generated","custom_title":"Latest title"}"#,
    )
    .unwrap();
    std::fs::write(sessions_dir.join("archived.json"), r#"{"title":"Hidden"}"#).unwrap();
    std::fs::write(sessions_dir.join("malformed.json"), "not json").unwrap();
    std::fs::write(
        home.join("sdk-archive.json"),
        r#"{"sessions":{"archived":123}}"#,
    )
    .unwrap();

    let merged = merge_persisted_sessions(vec![session_info("older")], Some(&home));
    let ids = merged
        .iter()
        .map(|session| session.session_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["older", "newer"]);
    assert_eq!(merged[1].title.as_deref(), Some("Latest title"));
    assert_eq!(merged[1].working_dir.as_deref(), Some("/new"));
    assert!(merged[1].transcript_bytes.is_some());

    // Interacting with an older-created session rewrites its transcript and
    // should move it ahead of sessions created later.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(
        sessions_dir.join("older.json"),
        r#"{"working_dir":"/old","title":"Old title","messages":[]}"#,
    )
    .unwrap();
    let merged = merge_persisted_sessions(Vec::new(), Some(&home));
    let ids = merged
        .iter()
        .map(|session| session.session_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["newer", "older"]);

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn persisted_sidebar_bounds_transcript_parsing_to_recent_sessions() {
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-bounded-sessions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sessions_dir = home.join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    for index in 0..MAX_PERSISTED_SIDEBAR_SESSIONS + 20 {
        std::fs::write(
            sessions_dir.join(format!("session-{index:03}.json")),
            format!(r#"{{"title":"Session {index}"}}"#),
        )
        .unwrap();
    }

    let merged = merge_persisted_sessions(Vec::new(), Some(&home));
    assert_eq!(merged.len(), MAX_PERSISTED_SIDEBAR_SESSIONS);

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn persisted_sidebar_reads_large_transcript_metadata_from_file_edges() {
    let path = std::env::temp_dir().join(format!(
        "jcode-desktop-large-session-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let padding = "x".repeat(SIDEBAR_METADATA_WINDOW * 3);
    let document = format!(
        r#"{{"title":"Quick title","messages":[{{"text":"{padding}"}}],"working_dir":"/large/project"}}"#
    );
    std::fs::write(&path, &document).unwrap();

    let record = read_persisted_session(&path, document.len() as u64).unwrap();
    assert_eq!(record.title.as_deref(), Some("Quick title"));
    assert_eq!(record.working_dir.as_deref(), Some("/large/project"));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn persisted_sidebar_uses_tui_todo_title_for_live_sessions() {
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-todo-title-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(home.join("todos")).unwrap();
    std::fs::write(
        home.join("todos/live.json"),
        r#"[{"content":"Fallback item","status":"in_progress","group":"Sidebar performance"}]"#,
    )
    .unwrap();
    let mut live = session_info("live");
    live.title = None;

    let merged = merge_persisted_sessions(vec![live], Some(&home));
    assert_eq!(merged[0].title.as_deref(), Some("Sidebar performance"));

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn older_session_catalog_uses_persisted_model_and_modes() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    std::fs::write(
        sessions.join("older.json"),
        r#"{"model":"future-model","reasoning_effort":"high","pinned_skill":"selfdev"}"#,
    )
    .unwrap();

    for listed in [vec![], vec![session_info("older")]] {
        let merged = merge_persisted_sessions(listed, Some(home.path()));
        let context = merged[0].context.as_ref().unwrap();
        assert_eq!(context.model.as_deref(), Some("future-model"));
        assert_eq!(context.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(context.modes, ["selfdev"]);
        assert_eq!(context.cache_expires_at_ms, None);
    }
}

#[test]
fn full_api_page_is_ready_without_scanning_persisted_transcripts() {
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-api-page-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let sessions_dir = home.join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    std::fs::write(
        sessions_dir.join("disk-only.json"),
        r#"{"title":"Must not delay the API page"}"#,
    )
    .unwrap();

    let api_sessions = (0..MAX_PERSISTED_SIDEBAR_SESSIONS)
        .map(|index| session_info(&format!("api-{index:03}")))
        .collect::<Vec<_>>();
    let merged = merge_persisted_sessions(api_sessions, Some(&home));

    assert_eq!(merged.len(), MAX_PERSISTED_SIDEBAR_SESSIONS);
    assert_eq!(merged[0].session_id, "api-099");
    assert_eq!(merged.last().unwrap().session_id, "api-000");
    assert!(
        merged
            .iter()
            .all(|session| session.session_id != "disk-only")
    );

    std::fs::remove_dir_all(home).unwrap();
}
