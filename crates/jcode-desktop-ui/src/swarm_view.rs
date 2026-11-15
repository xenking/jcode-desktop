//! Coordinator view of its swarm: worker lifecycle states, the compact
//! "workers running" indicator and the agent activity icon. Data comes from
//! the daemon's durable state (swarm snapshots via `SessionInfo::swarm_status`
//! and session records), never from guesses based on transcript activity.

use jcode_sdk::SessionInfo;
use std::collections::HashMap;

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

    /// Still part of the swarm's live work: running, queued or paused on a
    /// dependency. These count toward the composer indicator.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Running | Self::Queued | Self::Blocked)
    }

    /// The status mark shown beside a worker. `None` when nothing is known,
    /// so an unrecorded session never claims to be running or waiting.
    pub fn glyph(self) -> Option<Glyph> {
        match self {
            Self::Running | Self::Queued => Some(Glyph::Spinner),
            Self::Ready | Self::Stopped => Some(Glyph::Check),
            Self::Failed => Some(Glyph::Cross),
            Self::Blocked => Some(Glyph::Pause),
            Self::Unknown => None,
        }
    }
}

/// Status marks: spinner = running, check = done, cross = failed, pause = waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Spinner,
    Check,
    Cross,
    Pause,
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

/// The swarm snapshot's status wins. Workers the snapshot no longer lists
/// fall back to their persisted lifecycle: a closed one finished, a crashed
/// one failed. An open-ended `active` record proves nothing, so it stays unknown.
pub fn worker_state(session: &SessionInfo) -> WorkerState {
    match WorkerState::parse(session.swarm_status.as_deref()) {
        WorkerState::Unknown => match session.status.trim().to_ascii_lowercase().as_str() {
            "closed" => WorkerState::Ready,
            "crashed" | "error" => WorkerState::Failed,
            _ => WorkerState::Unknown,
        },
        state => state,
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
            state: worker_state(session),
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

/// Finished workers kept visible under a coordinator, newest last. A
/// long-running coordinator has dozens, and the sidebar should show what is
/// happening, not an archive.
pub const FINISHED_VISIBLE: usize = 3;

/// Rows to show under a coordinator: every live worker, plus the most recent
/// finished ones, in the original order. Returns how many finished were
/// folded away.
pub fn visible_workers<T: Clone>(
    rows: &[T],
    state: impl Fn(&T) -> WorkerState,
) -> (Vec<T>, usize) {
    let finished = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| !state(row).is_live())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let hidden = finished.len().saturating_sub(FINISHED_VISIBLE);
    let folded = &finished[..hidden];
    let shown = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| !folded.contains(index))
        .map(|(_, row)| row.clone())
        .collect();
    (shown, hidden)
}

/// The composer's compact worker indicator: nothing unless some worker is
/// still live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indicator {
    /// Live workers, running first.
    pub workers: Vec<Worker>,
}

impl Indicator {
    pub fn from_workers(workers: &[Worker]) -> Option<Self> {
        let mut live = workers
            .iter()
            .filter(|w| w.state.is_live())
            .cloned()
            .collect::<Vec<_>>();
        if live.is_empty() {
            return None;
        }
        // Stable: catalog order is kept inside each group.
        live.sort_by_key(|w| !w.state.is_active());
        Some(Self { workers: live })
    }

    pub fn count(&self) -> usize {
        self.workers.len()
    }

    /// Spinning while anything is actually running, paused when every live
    /// worker waits on a dependency.
    pub fn glyph(&self) -> Glyph {
        if self.workers.iter().any(|w| w.state.is_active()) {
            Glyph::Spinner
        } else {
            Glyph::Pause
        }
    }

    pub fn tooltip(&self) -> String {
        self.workers
            .iter()
            .map(|w| format!("{} · {}", w.name, w.status_text()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What the agent is doing, in one word for the composer's activity icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Thinking,
    RunningTool,
    WaitingOnWorkers,
}

/// The agent's own turn, as far as the icon cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Idle,
    /// Reasoning, streaming text, or waiting for the model.
    Thinking,
    Tools,
}

impl Activity {
    /// A running turn wins. An idle coordinator with live workers is
    /// waiting on them, not hung.
    pub fn resolve(turn: Turn, live_workers: usize) -> Self {
        match (turn, live_workers) {
            (Turn::Tools, _) => Self::RunningTool,
            (Turn::Thinking, _) => Self::Thinking,
            (Turn::Idle, 0) => Self::Idle,
            (Turn::Idle, _) => Self::WaitingOnWorkers,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Thinking => "Thinking",
            Self::RunningTool => "Running tool",
            Self::WaitingOnWorkers => "Waiting on workers",
        }
    }
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

    fn worker(name: &str, status: Option<&str>) -> Worker {
        let id = format!("session_{name}_1_a");
        Worker::from_session(&session(&id, Some("lead"), status), 1_000_000)
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
            session("session_rose_2_b", Some("session_microbe_1_a"), Some("running")),
            session("session_tulip_3_c", Some("session_microbe_1_a"), Some("failed")),
            session("session_tulip_3_c", Some("session_microbe_1_a"), Some("failed")),
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

    /// User spec: spinner = running, check = done, x = failed, pause = waiting.
    #[test]
    fn each_state_has_the_status_glyph_the_user_asked_for() {
        assert_eq!(worker("a", Some("running")).state.glyph(), Some(Glyph::Spinner));
        assert_eq!(worker("a", Some("spawned")).state.glyph(), Some(Glyph::Spinner));
        assert_eq!(worker("a", Some("ready")).state.glyph(), Some(Glyph::Check));
        assert_eq!(worker("a", Some("completed")).state.glyph(), Some(Glyph::Check));
        assert_eq!(worker("a", Some("stopped")).state.glyph(), Some(Glyph::Check));
        assert_eq!(worker("a", Some("failed")).state.glyph(), Some(Glyph::Cross));
        assert_eq!(worker("a", Some("crashed")).state.glyph(), Some(Glyph::Cross));
        assert_eq!(worker("a", Some("blocked")).state.glyph(), Some(Glyph::Pause));
        assert_eq!(worker("a", None).state.glyph(), None);
    }

    /// A worker missing from the swarm snapshot still shows how it ended,
    /// but an `active` record alone never claims it is running.
    #[test]
    fn unlisted_workers_fall_back_to_their_persisted_lifecycle() {
        let with_status = |status: &str| {
            let mut s = session("session_x_1_a", Some("lead"), None);
            s.status = status.into();
            worker_state(&s)
        };
        assert_eq!(with_status("closed"), WorkerState::Ready);
        assert_eq!(with_status("crashed"), WorkerState::Failed);
        assert_eq!(with_status("active"), WorkerState::Unknown);
        let mut listed = session("session_x_1_a", Some("lead"), Some("running"));
        listed.status = "closed".into();
        assert_eq!(worker_state(&listed), WorkerState::Running);
    }

    #[test]
    fn indicator_counts_live_workers_and_hides_when_none() {
        assert!(Indicator::from_workers(&[]).is_none());
        let done = [worker("a", Some("ready")), worker("b", Some("failed")), worker("c", None)];
        assert!(Indicator::from_workers(&done).is_none());

        let mixed = [
            worker("a", Some("ready")),
            worker("b", Some("blocked")),
            worker("c", Some("running")),
            worker("d", Some("spawned")),
        ];
        let indicator = Indicator::from_workers(&mixed).unwrap();
        assert_eq!(indicator.count(), 3);
        assert_eq!(indicator.glyph(), Glyph::Spinner);
        // Running first, then the paused one.
        let names = indicator.workers.iter().map(|w| w.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, ["c", "d", "b"]);
        assert_eq!(indicator.tooltip().lines().count(), 3);
        assert!(indicator.tooltip().starts_with("c · running"));

        let waiting = Indicator::from_workers(&[worker("a", Some("blocked"))]).unwrap();
        assert_eq!(waiting.glyph(), Glyph::Pause);
    }

    #[test]
    fn activity_covers_idle_thinking_tool_and_waiting_on_workers() {
        use Activity::*;
        assert_eq!(Activity::resolve(Turn::Idle, 0), Idle);
        assert_eq!(Activity::resolve(Turn::Thinking, 0), Thinking);
        assert_eq!(Activity::resolve(Turn::Tools, 0), RunningTool);
        // Idle with workers still running is waiting, not hung.
        assert_eq!(Activity::resolve(Turn::Idle, 3), WaitingOnWorkers);
        // The agent's own turn wins over its workers.
        assert_eq!(Activity::resolve(Turn::Thinking, 3), Thinking);
        assert_eq!(Activity::resolve(Turn::Tools, 3), RunningTool);
        assert_eq!(WaitingOnWorkers.label(), "Waiting on workers");
    }

    #[test]
    fn sidebar_keeps_live_workers_and_only_the_latest_finished() {
        let rows = ["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .zip([
                "ready", "running", "ready", "failed", "ready", "blocked",
            ])
            .map(|(name, status)| worker(name, Some(status)))
            .collect::<Vec<_>>();
        let (shown, hidden) = visible_workers(&rows, |w| w.state);
        let names = shown.iter().map(|w| w.name.as_str()).collect::<Vec<_>>();
        // a (oldest finished) folds away. Live b and f stay.
        assert_eq!(names, ["b", "c", "d", "e", "f"]);
        assert_eq!(hidden, 1);
        let (all, none) = visible_workers(&rows[1..], |w| w.state);
        assert_eq!((all.len(), none), (5, 0));
    }
}
