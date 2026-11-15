//! Coordinator view of its swarm: worker lifecycle states, pending
//! `await_members` waits, and recent "worker reported, coordinator woke"
//! events. Data comes from the daemon's durable state (swarm snapshots, via
//! `SessionInfo::swarm_status`, and `jcode-await-members/*.json`), never
//! from guesses based on transcript activity.

use jcode_sdk::SessionInfo;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Directory name the daemon uses under its runtime dir for await state.
pub const AWAIT_MEMBERS_DIR: &str = "jcode-await-members";
/// How long a resolved await keeps its "reported · woke" line visible.
pub const REPORT_VISIBLE_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkerState {
    Running,
    Ready,
    Failed,
    Stopped,
    Queued,
    Blocked,
    Unknown,
}

impl WorkerState {
    pub fn parse(status: Option<&str>) -> Self {
        match status
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("running" | "working" | "active" | "busy") => Self::Running,
            Some("ready" | "completed" | "done" | "idle") => Self::Ready,
            Some("failed" | "crashed" | "error") => Self::Failed,
            Some("stopped" | "cancelled" | "canceled") => Self::Stopped,
            Some("queued" | "spawned" | "spawning" | "pending") => Self::Queued,
            Some("blocked") => Self::Blocked,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Queued => "queued",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    /// Ready, completed, stopped or failed: no longer worth waiting on.
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Ready | Self::Failed | Self::Stopped)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Queued)
    }
}

/// `session_rose_1794717290730_574a...` -> `rose`.
pub fn friendly_name(session_id: &str) -> Option<&str> {
    let rest = session_id.strip_prefix("session_")?;
    let (name, tail) = rest.split_once('_')?;
    (!name.is_empty() && tail.starts_with(|c: char| c.is_ascii_digit())).then_some(name)
}

pub fn display_name(session: &SessionInfo) -> String {
    friendly_name(&session.session_id)
        .map(str::to_owned)
        .or_else(|| session.agent_label.clone())
        .or_else(|| session.title.clone())
        .unwrap_or_else(|| session.session_id.clone())
}

/// Compact age: `45s`, `4m`, `2h`, `3d`.
pub fn format_age(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worker {
    pub session_id: String,
    pub name: String,
    pub label: Option<String>,
    pub state: WorkerState,
    /// Since last daemon-reported activity, when known.
    pub age_ms: Option<u64>,
}

impl Worker {
    pub fn from_session(session: &SessionInfo, now_ms: u64) -> Self {
        let last = session.last_active_at_ms.or(session.updated_at_ms);
        Self {
            session_id: session.session_id.clone(),
            name: display_name(session),
            label: session.agent_label.clone(),
            state: WorkerState::parse(session.swarm_status.as_deref()),
            age_ms: last
                .filter(|ms| *ms > 0)
                .map(|ms| now_ms.saturating_sub(ms as u64)),
        }
    }

    /// `running 4m` or `ready`.
    pub fn status_text(&self) -> String {
        match self.age_ms {
            Some(age) => format!("{} {}", self.state.label(), format_age(age)),
            None => self.state.label().to_owned(),
        }
    }
}

/// Direct swarm workers per coordinator, in catalog order. Only swarm
/// ownership (`parent_session_id`) counts, never transcript forks.
pub fn workers_by_coordinator(
    sessions: &[SessionInfo],
    now_ms: u64,
) -> HashMap<String, Vec<Worker>> {
    let mut map: HashMap<String, Vec<Worker>> = HashMap::new();
    for session in sessions {
        let Some(parent) = session.parent_session_id.as_deref() else {
            continue;
        };
        if parent == session.session_id {
            continue;
        }
        let list = map.entry(parent.to_owned()).or_default();
        if list.iter().any(|w| w.session_id == session.session_id) {
            continue;
        }
        list.push(Worker::from_session(session, now_ms));
    }
    map
}

#[derive(Deserialize)]
struct AwaitFile {
    session_id: String,
    #[serde(default)]
    requested_ids: Vec<String>,
    #[serde(default)]
    created_at_unix_ms: u64,
    #[serde(default)]
    deadline_unix_ms: u64,
    #[serde(default = "yes")]
    wake: bool,
    #[serde(default)]
    final_response: Option<AwaitResult>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct AwaitResult {
    #[serde(default)]
    members: Vec<AwaitMember>,
    #[serde(default)]
    resolved_at_unix_ms: u64,
}

#[derive(Deserialize)]
struct AwaitMember {
    session_id: String,
    #[serde(default)]
    friendly_name: Option<String>,
    #[serde(default)]
    done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AwaitState {
    /// Worker ids the coordinator is waiting on right now.
    pub waiting_on: Vec<String>,
    /// Recent resolved waits: (worker name, resolved at, woke coordinator).
    pub reports: Vec<Report>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub name: String,
    pub at_ms: u64,
    pub woke: bool,
}

/// Await directories to read: beside the daemon socket first, then the
/// resolved runtime dir. The app can inherit a `TMPDIR` (an agent scratch
/// dir) that is not where the shared daemon writes `jcode-await-members`, so
/// `runtime_dir()` alone found only stale waits and the strip never showed.
pub fn await_dirs(socket: Option<&Path>, runtime: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    for base in socket.and_then(Path::parent).into_iter().chain([runtime]) {
        let dir = base.join(AWAIT_MEMBERS_DIR);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Await state per coordinator from the daemon's durable await files.
pub fn load_awaits(dir: &Path, now_ms: u64) -> HashMap<String, AwaitState> {
    load_awaits_from(std::slice::from_ref(&dir.to_path_buf()), now_ms)
}

/// Like [`load_awaits`], merging several directories.
pub fn load_awaits_from(dirs: &[std::path::PathBuf], now_ms: u64) -> HashMap<String, AwaitState> {
    let files = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| std::fs::read(p).ok())
        .filter_map(|bytes| serde_json::from_slice::<AwaitFile>(&bytes).ok());
    collect_awaits(files, now_ms)
}

fn collect_awaits(
    files: impl IntoIterator<Item = AwaitFile>,
    now_ms: u64,
) -> HashMap<String, AwaitState> {
    let mut map: HashMap<String, AwaitState> = HashMap::new();
    let mut files = files.into_iter().collect::<Vec<_>>();
    files.sort_by_key(|f| f.created_at_unix_ms);
    for file in files {
        match file.final_response {
            None if file.deadline_unix_ms == 0 || file.deadline_unix_ms > now_ms => {
                let state = map.entry(file.session_id).or_default();
                for id in file.requested_ids {
                    if !state.waiting_on.contains(&id) {
                        state.waiting_on.push(id);
                    }
                }
            }
            None => {}
            Some(result) => {
                if now_ms.saturating_sub(result.resolved_at_unix_ms) > REPORT_VISIBLE_MS {
                    continue;
                }
                let state = map.entry(file.session_id).or_default();
                for member in result.members.into_iter().filter(|m| m.done) {
                    let name = member
                        .friendly_name
                        .or_else(|| friendly_name(&member.session_id).map(str::to_owned))
                        .unwrap_or(member.session_id);
                    state.reports.push(Report {
                        name,
                        at_ms: result.resolved_at_unix_ms,
                        woke: file.wake,
                    });
                }
            }
        }
    }
    for state in map.values_mut() {
        state.reports.sort_by_key(|r| std::cmp::Reverse(r.at_ms));
        state.reports.dedup_by(|a, b| a.name == b.name);
    }
    map
}

/// What the coordinator's composer strip shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Strip {
    pub workers: Vec<Worker>,
    /// `Waiting for 2 workers: tulip (running 4m), rose (ready) · wakes when they report`
    pub waiting: Option<String>,
    /// `rose reported 2m ago · coordinator woke`
    pub report: Option<String>,
}

pub fn strip(workers: &[Worker], awaits: Option<&AwaitState>, now_ms: u64) -> Option<Strip> {
    if workers.is_empty() && awaits.is_none_or(|a| a.waiting_on.is_empty()) {
        return None;
    }
    // Members whose live swarm status is terminal already reported. The
    // await file still lists them until every requested member is done.
    let parts = awaits
        .map(|a| {
            a.waiting_on
                .iter()
                .filter_map(|id| match workers.iter().find(|w| &w.session_id == id) {
                    Some(w) if w.state.is_finished() => None,
                    Some(w) => Some(format!("{} ({})", w.name, w.status_text())),
                    None => Some(friendly_name(id).unwrap_or(id).to_owned()),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let waiting = (!parts.is_empty()).then(|| {
        let n = parts.len();
        format!(
            "Waiting for {n} worker{}: {} · wakes when they report",
            if n == 1 { "" } else { "s" },
            parts.join(", ")
        )
    });
    let report = awaits.and_then(|a| a.reports.first()).map(|r| {
        format!(
            "{} reported {} ago{}",
            r.name,
            format_age(now_ms.saturating_sub(r.at_ms)),
            if r.woke { " · coordinator woke" } else { "" }
        )
    });
    Some(Strip {
        workers: workers.to_vec(),
        waiting,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, parent: Option<&str>, status: Option<&str>) -> SessionInfo {
        serde_json::from_value(serde_json::json!({
            "session_id": id, "status": "idle", "parent_session_id": parent,
            "swarm_status": status, "last_active_at_ms": 1_000_000,
        }))
        .unwrap()
    }

    #[test]
    fn states_cover_daemon_vocabulary() {
        for (s, want) in [
            ("running", WorkerState::Running),
            ("ready", WorkerState::Ready),
            ("completed", WorkerState::Ready),
            ("failed", WorkerState::Failed),
            ("crashed", WorkerState::Failed),
            ("stopped", WorkerState::Stopped),
            ("spawned", WorkerState::Queued),
            ("blocked", WorkerState::Blocked),
            ("weird", WorkerState::Unknown),
        ] {
            assert_eq!(WorkerState::parse(Some(s)), want, "{s}");
        }
        assert_eq!(WorkerState::parse(None), WorkerState::Unknown);
    }

    #[test]
    fn friendly_names_from_ids() {
        assert_eq!(
            friendly_name("session_rose_1794717290730_574aea8d9f6adcc3"),
            Some("rose")
        );
        assert_eq!(friendly_name("screenshot-fixture"), None);
        assert_eq!(friendly_name("session_x_abc"), None);
    }

    #[test]
    fn workers_group_under_coordinator_with_status_and_age() {
        let sessions = vec![
            session("session_microbe_1_a", None, Some("running")),
            session(
                "session_rose_2_b",
                Some("session_microbe_1_a"),
                Some("running"),
            ),
            session(
                "session_tulip_3_c",
                Some("session_microbe_1_a"),
                Some("failed"),
            ),
            session(
                "session_tulip_3_c",
                Some("session_microbe_1_a"),
                Some("failed"),
            ),
            session("loop", Some("loop"), None),
        ];
        let map = workers_by_coordinator(&sessions, 1_240_000);
        assert_eq!(map.len(), 1);
        let w = &map["session_microbe_1_a"];
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "rose");
        assert_eq!(w[0].status_text(), "running 4m");
        assert_eq!(w[1].state, WorkerState::Failed);
    }

    fn file(json: serde_json::Value) -> AwaitFile {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn pending_awaits_and_recent_reports() {
        let now = 10_000_000;
        let awaits = collect_awaits(
            [
                file(
                    serde_json::json!({"session_id":"c","requested_ids":["session_rose_2_b","session_tulip_3_c"],"created_at_unix_ms":1,"deadline_unix_ms":now+1}),
                ),
                // Expired pending wait is not a wait.
                file(
                    serde_json::json!({"session_id":"c","requested_ids":["old"],"created_at_unix_ms":0,"deadline_unix_ms":now-1}),
                ),
                file(
                    serde_json::json!({"session_id":"c","requested_ids":["x"],"created_at_unix_ms":2,"deadline_unix_ms":now+1,
                    "final_response":{"members":[{"session_id":"session_otter_9_z","friendly_name":"otter","done":true}],"resolved_at_unix_ms":now-120_000}}),
                ),
                // Stale report is hidden.
                file(
                    serde_json::json!({"session_id":"c","requested_ids":["y"],"created_at_unix_ms":3,"deadline_unix_ms":0,
                    "final_response":{"members":[{"session_id":"s","friendly_name":"old","done":true}],"resolved_at_unix_ms":1}}),
                ),
            ],
            now,
        );
        let c = &awaits["c"];
        assert_eq!(c.waiting_on, vec!["session_rose_2_b", "session_tulip_3_c"]);
        assert_eq!(c.reports.len(), 1);

        let sessions = vec![session("session_rose_2_b", Some("c"), Some("running"))];
        let workers = workers_by_coordinator(&sessions, 1_240_000)
            .remove("c")
            .unwrap();
        let s = strip(&workers, Some(c), now).unwrap();
        assert_eq!(
            s.waiting.as_deref(),
            Some("Waiting for 2 workers: rose (running 4m), tulip · wakes when they report")
        );
        assert_eq!(
            s.report.as_deref(),
            Some("otter reported 2m ago · coordinator woke")
        );
    }

    /// User report: strip listed fish, fox, hamster, pig, rooster although
    /// most had already finished. Only members still running count.
    #[test]
    fn waiting_strip_omits_finished_members() {
        let state = AwaitState {
            waiting_on: ["fish", "fox", "hamster", "pig", "rooster"]
                .iter()
                .map(|n| format!("session_{n}_1_a"))
                .collect(),
            ..Default::default()
        };
        let sessions = vec![
            session("session_fish_1_a", Some("c"), Some("ready")),
            session("session_fox_1_a", Some("c"), Some("completed")),
            session("session_hamster_1_a", Some("c"), Some("stopped")),
            session("session_pig_1_a", Some("c"), Some("failed")),
            session("session_rooster_1_a", Some("c"), Some("running")),
        ];
        let workers = workers_by_coordinator(&sessions, 1_000_000)
            .remove("c")
            .unwrap();
        let s = strip(&workers, Some(&state), 1_000_000).unwrap();
        assert_eq!(
            s.waiting.as_deref(),
            Some("Waiting for 1 worker: rooster (running 0s) · wakes when they report")
        );
        let mut done = workers.clone();
        done.iter_mut().for_each(|w| w.state = WorkerState::Stopped);
        assert!(strip(&done, Some(&state), 0).unwrap().waiting.is_none());
    }

    #[test]
    fn no_strip_without_workers_or_waits() {
        assert!(strip(&[], None, 0).is_none());
        assert!(strip(&[], Some(&AwaitState::default()), 0).is_none());
    }

    #[test]
    fn load_awaits_reads_directory_and_skips_garbage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.json"),
            r#"{"session_id":"c","requested_ids":["w"],"deadline_unix_ms":0}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("b.json"), "nope").unwrap();
        std::fs::write(dir.path().join("a.bak"), r#"{"session_id":"z"}"#).unwrap();
        let map = load_awaits(dir.path(), 5);
        assert_eq!(map.len(), 1);
        assert_eq!(map["c"].waiting_on, vec!["w"]);
    }

    /// Live acceptance regression: the coordinator's real pending await
    /// (copied from the daemon's `jcode-await-members`) lives beside the
    /// daemon socket, while the app's `TMPDIR` scratch dir holds only an old,
    /// resolved one. Reading `runtime_dir()` alone showed no strip.
    #[test]
    fn live_pending_await_beside_daemon_socket_shows_waiting_strip() {
        let daemon = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        for (base, name, body) in [
            (
                daemon.path(),
                "pending.json",
                include_str!("swarm_view_fixtures/live_pending_await.json"),
            ),
            (
                scratch.path(),
                "old.json",
                include_str!("swarm_view_fixtures/live_resolved_await.json"),
            ),
        ] {
            let dir = base.join(AWAIT_MEMBERS_DIR);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(name), body).unwrap();
        }
        let coordinator = "session_microbe_1794670947943_2648f3bbec7c7e5e";
        let now = 1_794_718_800_000;
        let before = load_awaits(&scratch.path().join(AWAIT_MEMBERS_DIR), now);
        assert!(
            before
                .get(coordinator)
                .is_none_or(|a| a.waiting_on.is_empty())
        );

        let dirs = await_dirs(Some(&daemon.path().join("jcode.sock")), scratch.path());
        let awaits = load_awaits_from(&dirs, now);
        let state = &awaits[coordinator];
        assert_eq!(state.waiting_on.len(), 3);
        let s = strip(&[], Some(state), now).unwrap();
        assert_eq!(
            s.waiting.as_deref(),
            Some("Waiting for 3 workers: fish, fox, hamster · wakes when they report")
        );
    }
}
