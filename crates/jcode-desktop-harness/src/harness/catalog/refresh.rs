//! Debounced background refresh of the session catalog.

use std::path::Path;

use std::sync::mpsc::Receiver;
use std::time::Duration;

use jcode_sdk::SessionInfo;

use crate::harness::catalog::{catalog_todos_in, merge_persisted_sessions};
use crate::harness::local_runtime::connect;
use crate::harness::protocol::{CatalogTodos, Update};
use crate::harness::updates::UpdateSender;
use crate::platform::jcode_home;

pub(in crate::harness) struct SessionRefresh {
    wake: std::sync::mpsc::SyncSender<()>,
    initial: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl SessionRefresh {
    pub(in crate::harness) fn spawn(updates: UpdateSender) -> Self {
        let (wake, requests) = std::sync::mpsc::sync_channel(1);
        let initial = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let read_initial = initial.clone();
        std::thread::Builder::new()
            .name("jcode-bridge-sessions".into())
            .spawn(move || {
                let mut previous_todos = CatalogTodos::default();
                run_session_refreshes(requests, Duration::from_millis(150), || {
                    let include_disk_snapshot =
                        read_initial.swap(false, std::sync::atomic::Ordering::AcqRel);
                    refresh_session_catalog(&updates, include_disk_snapshot, &mut previous_todos);
                });
            })
            .expect("spawn session list thread");
        Self { wake, initial }
    }

    pub(in crate::harness) fn request(&self, include_disk_snapshot: bool) {
        if include_disk_snapshot {
            self.initial
                .store(true, std::sync::atomic::Ordering::Release);
        }
        let _ = self.wake.try_send(());
    }
}

pub(super) fn run_session_refreshes(
    requests: Receiver<()>,
    debounce: Duration,
    mut refresh: impl FnMut(),
) {
    while requests.recv().is_ok() {
        std::thread::sleep(debounce);
        // A wake during the debounce is covered by this read. A wake during
        // the read is retained for one trailing refresh, never dropped.
        match requests.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            _ => refresh(),
        }
    }
}

pub(super) fn refresh_session_catalog(
    updates: &UpdateSender,
    include_disk_snapshot: bool,
    previous_todos: &mut CatalogTodos,
) {
    let home = jcode_home();
    if include_disk_snapshot {
        let started = std::time::Instant::now();
        let mut sessions = merge_persisted_sessions(Vec::new(), home.as_deref());
        jcode_sdk::enrich_sessions_from_local_swarm_state(&mut sessions);
        jcode_sdk::enrich_sessions_from_local_edit_stats(&mut sessions);
        log::debug!(
            "jcode desktop: local session metadata loaded in {:.1}ms ({} sessions)",
            started.elapsed().as_secs_f64() * 1_000.0,
            sessions.len()
        );
        publish_session_catalog(updates, home.as_deref(), sessions, previous_todos);
    }
    let started = std::time::Instant::now();
    let listed = match connect("sessions").and_then(|client| client.list_sessions_limited(100)) {
        Ok(sessions) => sessions,
        Err(error) => {
            log::debug!(
                "jcode desktop: session list failed after {:.1}ms: {error}",
                started.elapsed().as_secs_f64() * 1_000.0
            );
            Vec::new()
        }
    };
    let mut sessions = merge_persisted_sessions(listed, home.as_deref());
    jcode_sdk::enrich_sessions_from_local_swarm_state(&mut sessions);
    jcode_sdk::enrich_sessions_from_local_edit_stats(&mut sessions);
    log::debug!(
        "jcode desktop: session list completed in {:.1}ms ({} sessions)",
        started.elapsed().as_secs_f64() * 1_000.0,
        sessions.len()
    );
    publish_session_catalog(updates, home.as_deref(), sessions, previous_todos);
}

pub(super) fn publish_session_catalog(
    updates: &UpdateSender,
    home: Option<&Path>,
    sessions: Vec<SessionInfo>,
    previous_todos: &mut CatalogTodos,
) {
    let todos = home
        .map(|home| catalog_todos_in(home, &sessions))
        .unwrap_or_default();
    send_worker_facts(updates, &sessions);
    let _ = updates.send(Update::Sessions { sessions });
    if *previous_todos != todos {
        previous_todos.clone_from(&todos);
        let _ = updates.send(Update::CatalogTodos(todos));
    }
}

/// Publish what each swarm worker's own record says about it, before the
/// catalog that lists the workers. A worker's model, effort and spend are not
/// in the catalog entry, and re-reading its transcript in the UI thread would
/// stall the sidebar.
pub(super) fn send_worker_facts(updates: &UpdateSender, sessions: &[SessionInfo]) {
    let workers = jcode_sdk::local_worker_meta(sessions);
    if !workers.is_empty() {
        let _ = updates.send(Update::WorkerFacts { workers });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::protocol::Update;
    use crate::harness::updates::update_channel;
    use std::sync::mpsc::channel;

    #[test]
    fn a_refresh_burst_during_a_read_has_exactly_one_trailing_read() {
        let (wake, requests) = std::sync::mpsc::sync_channel(1);
        let (started, received) = channel();
        let (release, resume) = channel();
        let worker = std::thread::spawn(move || {
            let mut reads = 0;
            run_session_refreshes(requests, Duration::ZERO, || {
                reads += 1;
                started.send(reads).unwrap();
                if reads == 1 {
                    resume.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            });
            reads
        });
        wake.try_send(()).unwrap();
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        for _ in 0..100 {
            let _ = wake.try_send(());
        }
        release.send(()).unwrap();
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
        drop(wake);
        assert_eq!(worker.join().unwrap(), 2);
    }

    #[test]
    fn todo_changes_publish_even_when_session_rows_are_identical() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("todos");
        std::fs::create_dir(&dir).unwrap();
        let session: SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": "session_fox_1_review", "status": "closed", "title": "Review"
        }))
        .unwrap();
        let path = dir.join("session_fox_1_review.json");
        let (updates, inbox) = update_channel();
        let mut previous = CatalogTodos::default();
        for (status, pending) in [("pending", true), ("completed", false)] {
            std::fs::write(
                &path,
                format!(r#"[{{"content":"Verify the build","status":"{status}"}}]"#),
            )
            .unwrap();
            publish_session_catalog(
                &updates,
                Some(home.path()),
                vec![session.clone()],
                &mut previous,
            );
            assert!(
                matches!(inbox.try_recv().unwrap(), Update::Sessions { sessions } if sessions == [session.clone()])
            );
            let Update::CatalogTodos(todos) = inbox.try_recv().unwrap() else {
                panic!("todo change missing")
            };
            assert_eq!(
                todos.progress[&session.session_id],
                (usize::from(!pending), 1)
            );
            assert_eq!(!todos.unfinished.is_empty(), pending);
        }
    }
}
