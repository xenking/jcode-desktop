//! Coordinator view of its swarm: worker lifecycle states, the compact
//! "workers running" indicator and the agent activity icon. Data comes from
//! ordered daemon events reconciled into `SessionInfo::swarm_status`
//! plus the worker's own record (`SessionInfo`'s worker table: model, effort,
//! tokens, and what its newest message says it is doing), never from guesses
//! based on the coordinator's transcript.

use jcode_sdk::{SessionInfo, WorkerMeta, WorkerPhase};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkerState {
    Running,
    Stalled,
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
            Some("running" | "working" | "active" | "busy" | "bg") => Self::Running,
            Some("stalled") => Self::Stalled,
            Some("ready" | "completed" | "done" | "idle") => Self::Ready,
            Some("failed" | "crashed" | "error") => Self::Failed,
            Some("stopped" | "cancelled" | "canceled") => Self::Stopped,
            Some("queued" | "spawned" | "spawning" | "starting" | "pending") => Self::Queued,
            Some("blocked") => Self::Blocked,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stalled => "stalled",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Queued => "queued",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    /// Ready, completed, stopped or failed: no longer worth waiting on.
    #[allow(dead_code)]
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Ready | Self::Failed | Self::Stopped)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Queued)
    }

    /// Still part of the swarm's live work: running, queued or paused on a
    /// dependency. These count toward the composer indicator.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Running | Self::Stalled | Self::Queued | Self::Blocked
        )
    }

    /// The status mark shown beside a worker. `None` when nothing is known,
    /// so an unrecorded session never claims to be running or waiting.
    pub fn glyph(self) -> Option<Glyph> {
        match self {
            Self::Running | Self::Queued => Some(Glyph::Spinner),
            Self::Ready | Self::Stopped => Some(Glyph::Check),
            Self::Failed => Some(Glyph::Cross),
            Self::Blocked | Self::Stalled => Some(Glyph::Pause),
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

/// Explicit daemon lifecycle statuses win over transcript activity. A worker
/// without a snapshot is idle/stopped when its persisted record is Active.
#[cfg(test)]
pub fn worker_state(session: &SessionInfo) -> WorkerState {
    resolve_worker_state(session, None, 0)
}

/// [`worker_state`] with worker metadata. Transcript activity cannot establish
/// liveness after a worker has been stopped or removed from the live swarm.
pub fn resolve_worker_state(
    session: &SessionInfo,
    meta: Option<&WorkerMeta>,
    _now_ms: u64,
) -> WorkerState {
    let state = match WorkerState::parse(session.swarm_status.as_deref()) {
        WorkerState::Unknown => match session.status.trim().to_ascii_lowercase().as_str() {
            "closed" => WorkerState::Ready,
            "crashed" | "error" => WorkerState::Failed,
            "active" | "idle" | "attached" if session.parent_session_id.is_some() => {
                WorkerState::Stopped
            }
            _ => WorkerState::Unknown,
        },
        state => state,
    };
    if state != WorkerState::Unknown {
        return state;
    }
    let Some(meta) = meta else {
        return state;
    };
    // No transcript phase proves a worker is still running. The daemon may
    // stop it while an unfinished tool or a final message remains in its log.
    // The snapshot forgot this worker (the daemon removed it) and its record
    // shows nothing in flight: it is not running, whatever an `active` record
    // says. Reading it as `unknown` was the sidebar's finished-worker bug.
    if state == WorkerState::Unknown && *meta != WorkerMeta::default() {
        return WorkerState::Stopped;
    }
    state
}

#[derive(Clone, Debug, PartialEq)]
pub struct Worker {
    pub session_id: String,
    pub name: String,
    pub label: Option<String>,
    title: Option<String>,
    pub state: WorkerState,
    /// Since the newest recorded activity, when known.
    pub age_ms: Option<u64>,
    /// Animal emoji beside the visible worker name.
    pub icon: Option<&'static str>,
    /// The worker's own record: model, effort, tokens and live phase.
    pub meta: WorkerMeta,
}

impl Worker {
    pub fn from_session(session: &SessionInfo, meta: Option<&WorkerMeta>, now_ms: u64) -> Self {
        let meta = meta.cloned().unwrap_or_default();
        // A worker's own newest message is fresher than the daemon's
        // last-active stamp, which only moves when a process starts.
        let last = meta.last_activity_ms.map(|ms| ms as u64).or(session
            .last_active_at_ms
            .or(session.updated_at_ms)
            .map(|ms| ms as u64));
        let name = display_name(session);
        Self {
            icon: friendly_name(&session.session_id)
                .map(jcode_core::id::session_icon)
                .filter(|icon| *icon != "💫"),
            label: session.agent_label.clone(),
            title: session.title.clone(),
            state: resolve_worker_state(session, Some(&meta), now_ms),
            age_ms: last
                .filter(|ms| *ms > 0)
                .map(|ms| now_ms.saturating_sub(ms)),
            session_id: session.session_id.clone(),
            name,
            meta,
        }
    }

    pub fn row_title(&self) -> &str {
        let title = self.title.as_deref().map(str::trim).filter(|title| {
            // Previous daemons stored this synthetic startup prompt as a title.
            *title != jcode_base::session_title::AUTO_LANE_START
        });
        [title, self.label.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|title| !title.is_empty())
            .unwrap_or(&self.name)
    }

    /// What the worker is doing right now. A finished or paused worker shows
    /// its lifecycle; a running one shows the tail of its own record, so a
    /// worker stopped on a tool result no longer reads "running" forever.
    pub fn state_text(&self) -> String {
        match self.state {
            WorkerState::Running => match self.meta.phase {
                Some(WorkerPhase::Tool) => self
                    .meta
                    .tool
                    .clone()
                    .filter(|tool| !tool.trim().is_empty())
                    .unwrap_or_else(|| "tool".to_owned()),
                Some(WorkerPhase::Thinking) => "thinking".to_owned(),
                Some(WorkerPhase::Waiting) => "waiting".to_owned(),
                Some(WorkerPhase::Idle) | None => "running".to_owned(),
            },
            WorkerState::Blocked => "waiting".to_owned(),
            state => state.label().to_owned(),
        }
    }

    /// `thinking 4m`, `bash 26m`, or just `ready`: a finished worker's clock
    /// stopped mattering, so it is not shown.
    pub fn status_text(&self) -> String {
        match (self.state.is_finished(), self.age_ms) {
            (false, Some(age)) => format!("{} {}", self.state_text(), format_age(age)),
            _ => self.state_text(),
        }
    }

    /// `ds-4.1f`, `opus 5.5`: the model, short enough for a one-line row.
    pub fn model_label(&self) -> Option<String> {
        self.meta
            .model
            .as_deref()
            .map(compact_model)
            .filter(|model| !model.is_empty())
    }

    /// The reasoning level a row draws, normalized (`" Low "` is `low`). The
    /// row paints it as the same drawn circle the composer pill uses, because
    /// a worker row has no room for the word. A record with no effort shows
    /// nothing rather than an empty circle.
    pub fn effort_level(&self) -> Option<String> {
        self.meta
            .effort
            .as_deref()
            .map(str::trim)
            .filter(|effort| !effort.is_empty())
            .map(str::to_ascii_lowercase)
    }

    /// `↑12k ↓3k`, or the half that is known.
    pub fn tokens_text(&self) -> Option<String> {
        let short = crate::swarm_view::short_tokens;
        match (self.meta.input_tokens, self.meta.output_tokens) {
            (Some(input), Some(output)) => Some(format!("↑{} ↓{}", short(input), short(output))),
            (Some(input), None) => Some(format!("↑{}", short(input))),
            (None, Some(output)) => Some(format!("↓{}", short(output))),
            (None, None) => None,
        }
    }

    /// `$0.42` on a metered route, nothing on a subscription: an OAuth seat
    /// has no per-token price, and "$0.00" would read as free.
    pub fn cost_text(&self) -> Option<String> {
        let cost = metered_cost(&self.meta)?;
        Some(format_cost(cost))
    }

    /// Everything the row had to leave out, for the hover tooltip.
    pub fn tooltip(&self) -> String {
        let mut parts = vec![self.name.clone()];
        let title = self.row_title();
        if title != self.name {
            parts.push(title.to_owned());
        }
        parts.push(self.status_text());
        let route = [
            self.meta.provider.as_deref().map(provider_label),
            self.meta.model.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("/");
        if !route.is_empty() {
            parts.push(route);
        }
        if let Some(effort) = self
            .meta
            .effort
            .as_deref()
            .filter(|effort| !effort.trim().is_empty())
        {
            parts.push(format!("{effort} effort"));
        }
        if let Some(tokens) = self.tokens_text() {
            parts.push(match self.cost_text() {
                Some(cost) => format!("{tokens} {cost}"),
                None => tokens,
            });
        }
        parts.join(" · ")
    }
}

/// `opencode-go` -> `opencode`: the auth suffix is noise on a one-line row.
fn provider_label(key: &str) -> &str {
    key.strip_suffix("-oauth")
        .or_else(|| key.strip_suffix("-api"))
        .unwrap_or(key)
}

/// Token counts as a row has room for them: `842`, `12k`, `4.5M`.
fn short_tokens(count: u64) -> String {
    let short = jcode_desktop_harness::swarm_usage::format_tokens(count);
    // `3.0k` wastes a character on nothing: the row says `3k`.
    short
        .strip_suffix(".0k")
        .map(|whole| format!("{whole}k"))
        .or_else(|| short.strip_suffix(".0M").map(|whole| format!("{whole}M")))
        .unwrap_or(short)
}

/// Estimated spend for the tokens the worker's record reports, at the price
/// of the route it runs on. `None` when the route has no per-token price
/// (subscription seats) or the model is not in any pricing catalog.
fn metered_cost(meta: &WorkerMeta) -> Option<f64> {
    let provider = meta.provider.as_deref()?.trim().to_ascii_lowercase();
    let model = meta
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())?;
    // A seat bills a flat subscription: any number here would be invented.
    if provider.contains("oauth")
        || [
            "copilot",
            "cursor",
            "jcode",
            "subscription",
            "claude-max",
            "chatgpt",
        ]
        .iter()
        .any(|seat| provider.contains(seat))
    {
        return None;
    }
    let cost = jcode_base::model_pricing::lookup(&provider, model)?;
    cost_for_worker(meta, cost)
}

fn cost_for_worker(meta: &WorkerMeta, cost: jcode_base::model_pricing::ModelCost) -> Option<f64> {
    let price = |per_mtok: f64| per_mtok / 1_000_000.0;
    let read = cost
        .cache_read_usd_per_mtok
        .unwrap_or(cost.input_usd_per_mtok);
    let write = cost
        .cache_write_usd_per_mtok
        .unwrap_or(cost.input_usd_per_mtok);
    let uncached = meta
        .input_tokens
        .unwrap_or(0)
        .saturating_sub(meta.cache_read_tokens.unwrap_or(0))
        .saturating_sub(meta.cache_creation_tokens.unwrap_or(0));
    let total = uncached as f64 * price(cost.input_usd_per_mtok)
        + meta.output_tokens.unwrap_or(0) as f64 * price(cost.output_usd_per_mtok)
        + meta.cache_read_tokens.unwrap_or(0) as f64 * price(read)
        + meta.cache_creation_tokens.unwrap_or(0) as f64 * price(write);
    (total > 0.0).then_some(total)
}

/// `$1.24`, `$0.043`, `<$0.01`: enough precision to see work happening,
/// never a fake zero.
fn format_cost(usd: f64) -> String {
    if usd >= 1.0 {
        format!("${usd:.2}")
    } else if usd >= 0.01 {
        format!("${usd:.3}")
    } else {
        "<$0.01".to_owned()
    }
}

/// A one-line model name: a long vendor keeps its product name, a short one
/// abbreviates and glues its variant on.
///
/// `deepseek-v4.1-flash` reads `ds-4.1f`, `claude-opus-5-5` reads `opus 5.5`,
/// `gpt-5.6` reads `gpt 5.6`.
pub fn compact_model(model: &str) -> String {
    let model = model
        .trim()
        .split_once('/')
        .map_or_else(|| model.trim(), |(_, bare)| bare.trim());
    let model = model
        .strip_prefix("openai:")
        .unwrap_or(model)
        .trim_start_matches("claude-")
        .to_ascii_lowercase();
    if let Some((version, variant)) = model.strip_prefix("gpt-").and_then(|s| s.split_once('-'))
        && is_version(version)
        && matches!(variant, "sol" | "luna" | "terra")
    {
        return format!("{variant} {version}");
    }
    let mut tokens = model.split('-').filter(|token| !token.is_empty());
    let Some(family) = tokens.next() else {
        return String::new();
    };
    let (short, abbreviated) = shorten_family(family);
    let mut version: Vec<&str> = Vec::new();
    let mut variants: Vec<&str> = Vec::new();
    for token in tokens {
        // Digits before any word are the version; anything after a word is a
        // variant (`flash`, `mini`, `preview`).
        if variants.is_empty() && is_version(token) {
            version.push(token);
        } else {
            variants.push(token);
        }
    }
    let version = version
        .iter()
        .map(|part| part.trim_start_matches('v'))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    let variant = variants
        .iter()
        .filter_map(|token| variant_letter(token))
        .collect::<String>();
    if abbreviated {
        return format!("{short}-{version}{variant}")
            .trim_end_matches('-')
            .to_owned();
    }
    // A short, readable variant word stays whole: `gpt 5.6 mini`.
    let suffix = variants
        .iter()
        .find(|token| token.chars().count() <= 5 && variant_letter(token).is_some())
        .filter(|_| !version.is_empty())
        .map(|token| format!(" {token}"))
        .unwrap_or_default();
    // A family with no version keeps its first word instead: `kimi-k2-0905`
    // reads `kimi k2`, not `kimi 0905`.
    let tail = if version.is_empty() {
        variants
            .iter()
            .find(|token| !token.chars().all(|c| c.is_ascii_digit()))
            .copied()
            .unwrap_or("")
            .to_owned()
    } else {
        version
    };
    format!("{short} {tail}{suffix}").trim().to_owned()
}

/// Families whose full name is too long for a row keep a recognised short
/// form; everything else reads as itself.
fn shorten_family(family: &str) -> (&str, bool) {
    match family {
        "deepseek" => ("ds", true),
        "gemini" => ("gem", true),
        "mistral" => ("mist", true),
        "llama" => ("llama", false),
        "moonshotai" | "kimi" => ("kimi", false),
        other => (other, false),
    }
}

fn is_version(token: &str) -> bool {
    let digits = token.trim_start_matches('v');
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// `flash` -> `f`, `mini` -> `m`, `thinking` -> `t`, `preview` -> nothing.
fn variant_letter(token: &str) -> Option<char> {
    match token {
        "preview" | "latest" | "exp" | "experimental" => None,
        other => other.chars().next().filter(|c| c.is_ascii_alphabetic()),
    }
}

/// Direct swarm workers per coordinator, in catalog order. Only swarm
/// ownership (`parent_session_id`) counts, never transcript forks.
pub fn workers_by_coordinator(
    sessions: &[SessionInfo],
    workers: &HashMap<String, WorkerMeta>,
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
        list.push(Worker::from_session(
            session,
            workers.get(&session.session_id),
            now_ms,
        ));
    }
    for workers in map.values_mut() {
        workers.sort_by(|a, b| spawn_order(&a.session_id).cmp(&spawn_order(&b.session_id)));
    }
    map
}

/// Spawn time is encoded in a daemon session id. Unlike activity timestamps,
/// tokens, status and map iteration, this key never changes while work runs.
pub fn spawn_order(session_id: &str) -> (u64, &str) {
    let mut parts = session_id.rsplit('_');
    let _nonce = parts.next();
    (
        parts
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(u64::MAX),
        session_id,
    )
}

/// Finished workers kept visible under a coordinator. A long-running
/// coordinator has dozens, and the sidebar should show what is happening, not
/// an archive.
pub const FINISHED_VISIBLE: usize = 3;

/// Preserve spawn order while keeping every live worker and the latest few
/// recently finished workers. Rows arrive oldest-spawn first.
pub fn visible_workers<T: Clone>(rows: &[T], worker: impl Fn(&T) -> &Worker) -> (Vec<T>, usize) {
    let recent_finished = rows
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, row)| {
            let worker = worker(row);
            !worker.state.is_live() && worker.age_ms.is_none_or(|age| age < 3_600_000)
        })
        .take(FINISHED_VISIBLE)
        .map(|(index, _)| index)
        .collect::<std::collections::HashSet<_>>();
    let shown = rows
        .iter()
        .enumerate()
        .filter(|(index, row)| worker(row).state.is_live() || recent_finished.contains(index))
        .map(|(_, row)| row.clone())
        .collect::<Vec<_>>();
    let hidden = rows.len().saturating_sub(shown.len());
    (shown, hidden)
}

/// The composer's compact worker indicator: nothing unless some worker is
/// still live.
#[derive(Clone, Debug, PartialEq)]
pub struct Indicator {
    /// Live workers, running first.
    pub workers: Vec<Worker>,
}

/// Where the swarm's work stands, the three segments the composer pill draws:
/// running now, parked on a dependency, already reported. A finished worker
/// stays in the list until the workspace refreshes, so the done segment shows
/// what the turn produced before the pill disappears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwarmTally {
    pub running: usize,
    pub waiting: usize,
    pub done: usize,
}

impl SwarmTally {
    pub fn of(workers: &[Worker]) -> Self {
        let mut tally = Self::default();
        for worker in workers {
            match worker.state {
                WorkerState::Running => tally.running += 1,
                // Queued behind a dependency and blocked on one read the same
                // in the bar: neither is making progress yet.
                WorkerState::Queued | WorkerState::Blocked | WorkerState::Stalled => {
                    tally.waiting += 1
                }
                WorkerState::Ready | WorkerState::Failed | WorkerState::Stopped => tally.done += 1,
                WorkerState::Unknown => {}
            }
        }
        tally
    }

    pub fn total(&self) -> usize {
        self.running + self.waiting + self.done
    }

    /// `1 running · 2 waiting · 3 done`, with empty parts dropped so a uniform
    /// swarm reads as one phrase.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (count, word) in [
            (self.running, "running"),
            (self.waiting, "waiting"),
            (self.done, "done"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {word}"));
            }
        }
        parts.join(" · ")
    }
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
    /// worker waits on a dependency. Kept for the sidebar rows and tests; the
    /// composer pill draws its own segmented bar instead.
    #[allow(dead_code)]
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
        Worker::from_session(&session(&id, Some("lead"), status), None, 1_000_000)
    }

    #[test]
    fn auto_lane_worker_uses_catalog_title_then_live_label_then_name() {
        let mut session: SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": "session_puppy_1795159580000_abc",
            "status": "Active",
            "parent_session_id": "coordinator",
            "swarm_status": "running",
            "title": "btw-enter"
        }))
        .unwrap();
        let meta: WorkerMeta = serde_json::from_value(serde_json::json!({
            "model": "gpt-6.2-sol", "phase": "thinking",
            "input_tokens": 2_600_000, "output_tokens": 6_300
        }))
        .unwrap();
        let label = |session: &SessionInfo| {
            Worker::from_session(session, Some(&meta), 0)
                .row_title()
                .to_owned()
        };
        assert_eq!(label(&session), "btw-enter");
        session.agent_label = Some("btw-enter".into());
        session.title = Some("Open side questions".into());
        assert_eq!(label(&session), "Open side questions");
        session.title = Some(" \n ".into());
        assert_eq!(label(&session), "btw-enter");
        session.title = Some(jcode_base::session_title::AUTO_LANE_START.into());
        assert_eq!(label(&session), "btw-enter");
        session.agent_label = None;
        assert_eq!(label(&session), "puppy");
    }

    fn with_meta(name: &str, status: Option<&str>, meta: WorkerMeta) -> Worker {
        let id = format!("session_{name}_1_a");
        Worker::from_session(
            &session(&id, Some("lead"), status),
            Some(&meta),
            1_000_000 + 26 * 60 * 1_000,
        )
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
        let map = workers_by_coordinator(&sessions, &HashMap::new(), 1_240_000);
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
        assert_eq!(
            worker("a", Some("running")).state.glyph(),
            Some(Glyph::Spinner)
        );
        assert_eq!(
            worker("a", Some("spawned")).state.glyph(),
            Some(Glyph::Spinner)
        );
        assert_eq!(worker("a", Some("ready")).state.glyph(), Some(Glyph::Check));
        assert_eq!(
            worker("a", Some("completed")).state.glyph(),
            Some(Glyph::Check)
        );
        assert_eq!(
            worker("a", Some("stopped")).state.glyph(),
            Some(Glyph::Check)
        );
        assert_eq!(
            worker("a", Some("failed")).state.glyph(),
            Some(Glyph::Cross)
        );
        assert_eq!(
            worker("a", Some("crashed")).state.glyph(),
            Some(Glyph::Cross)
        );
        assert_eq!(
            worker("a", Some("blocked")).state.glyph(),
            Some(Glyph::Pause)
        );
        assert_eq!(worker("a", None).state.glyph(), Some(Glyph::Check));
        assert_eq!(WorkerState::Unknown.glyph(), None);
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
        assert_eq!(with_status("active"), WorkerState::Stopped);
        let mut listed = session("session_x_1_a", Some("lead"), Some("running"));
        listed.status = "closed".into();
        assert_eq!(worker_state(&listed), WorkerState::Running);
    }

    /// The daemon's explicit terminal lifecycle wins over an unfinished tool
    /// or a recent final message in the worker's transcript.
    #[test]
    fn terminal_snapshot_words_are_not_resurrected_by_worker_activity() {
        let mut s = session("session_kangaroo_1_a", Some("lead"), Some("ready"));
        let now = 1_000_000;
        let mut meta = WorkerMeta {
            phase: Some(WorkerPhase::Tool),
            tool: Some("bash".into()),
            last_activity_ms: Some((now - 3_000) as i64),
            ..WorkerMeta::default()
        };
        assert_eq!(
            resolve_worker_state(&s, Some(&meta), now),
            WorkerState::Ready,
            "an explicit terminal state wins over a recent transcript"
        );

        // The same stale word is honest again once the worker has been quiet:
        // a finished worker's final message must not read as running forever.
        meta.last_activity_ms = Some((now - 120_001) as i64);
        assert_eq!(
            resolve_worker_state(&s, Some(&meta), now),
            WorkerState::Ready
        );

        // A stopped worker stays stopped even if its final tool has no result.
        s.swarm_status = Some("stopped".into());
        meta.phase = Some(WorkerPhase::Tool);
        meta.last_activity_ms = Some((now - 1_000) as i64);
        assert_eq!(
            resolve_worker_state(&s, Some(&meta), now),
            WorkerState::Stopped
        );

        // No record at all: the snapshot is all there is.
        assert_eq!(resolve_worker_state(&s, None, now), WorkerState::Stopped);
    }

    /// A worker missing from the current swarm must not be resurrected by
    /// a recent transcript tail after it has been stopped.
    #[test]
    fn an_unlisted_worker_with_fresh_activity_is_not_running() {
        let s = session("session_otter_1_a", Some("lead"), None);
        let now = 1_000_000;
        let meta = WorkerMeta {
            phase: Some(WorkerPhase::Thinking),
            last_activity_ms: Some((now - 2_000) as i64),
            ..WorkerMeta::default()
        };
        assert_eq!(
            resolve_worker_state(&s, Some(&meta), now),
            WorkerState::Stopped
        );
        assert_eq!(worker_state(&s), WorkerState::Stopped);
    }

    /// The sidebar rows from the report: kangaroo and hedgehog finished, the
    /// daemon dropped them from the swarm, and their records still say
    /// `active` (the snapshot predates the journal's close). With a record
    /// that shows nothing in flight they are stopped, never `unknown`.
    #[test]
    fn a_forgotten_worker_with_a_quiet_record_reads_stopped_not_unknown() {
        let mut s = session("session_kangaroo_1_a", Some("lead"), None);
        s.status = "active".into();
        let now = 10_000_000;
        let meta = WorkerMeta {
            model: Some("deepseek-v4.1-flash".into()),
            provider: Some("opencode-go".into()),
            phase: Some(WorkerPhase::Idle),
            last_activity_ms: Some((now - 600_000) as i64),
            ..WorkerMeta::default()
        };
        assert_eq!(
            resolve_worker_state(&s, Some(&meta), now),
            WorkerState::Stopped
        );
        let row = Worker::from_session(&s, Some(&meta), now);
        assert_eq!(row.state_text(), "stopped");
        assert_eq!(row.model_label().as_deref(), Some("ds-4.1f"));
    }

    #[test]
    fn indicator_counts_live_workers_and_hides_when_none() {
        assert!(Indicator::from_workers(&[]).is_none());
        let done = [
            worker("a", Some("ready")),
            worker("b", Some("failed")),
            worker("c", None),
        ];
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
        let names = indicator
            .workers
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>();
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
        // Spawn order is stable, oldest first.
        let rows = ["oldest", "fourth", "third", "second", "fresh", "live"]
            .into_iter()
            .zip(["ready", "ready", "failed", "ready", "ready", "running"])
            .map(|(name, status)| worker(name, Some(status)))
            .collect::<Vec<_>>();
        let (shown, hidden) = visible_workers(&rows, |w| w);
        let names = shown.iter().map(|w| w.name.as_str()).collect::<Vec<_>>();
        // The two oldest finished workers fold away; the newest three and the
        // live worker stay, in sidebar order.
        assert_eq!(names, ["third", "second", "fresh", "live"]);
        assert_eq!(hidden, 2);
        // Every live worker is visible however recent the finished ones are.
        let (all, none) = visible_workers(&rows[..5], |w| w);
        assert_eq!((all.len(), none), (3, 2));
    }

    /// A coordinator with dozens of finished workers must show the ones that
    /// just finished, never an archive of the first ones it ever spawned.
    #[test]
    fn folded_finished_workers_are_the_oldest_not_the_newest() {
        let mut rows = (0..10)
            .map(|n| worker(&format!("w{n}"), Some("ready")))
            .collect::<Vec<_>>();
        // Oldest spawn first, the order the sidebar supplies.
        let (shown, hidden) = visible_workers(&rows, |w| w);
        let names = shown.iter().map(|w| w.name.as_str()).collect::<Vec<_>>();
        assert_eq!(
            names,
            ["w7", "w8", "w9"],
            "the newest finished workers stay"
        );
        assert_eq!(hidden, 7);
        // One running worker at the tail never displaces the newest finished.
        rows[9] = worker("live", Some("running"));
        let (shown, hidden) = visible_workers(&rows, |w| w);
        let names = shown.iter().map(|w| w.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, ["w6", "w7", "w8", "live"]);
        assert_eq!(hidden, 6);
    }

    fn record(model: &str, provider: &str, effort: Option<&str>) -> WorkerMeta {
        WorkerMeta {
            model: Some(model.into()),
            provider: Some(provider.into()),
            effort: effort.map(str::to_owned),
            input_tokens: Some(12_400),
            output_tokens: Some(3_100),
            phase: Some(WorkerPhase::Tool),
            tool: Some("bash".into()),
            last_activity_ms: Some(1_000_000),
            ..WorkerMeta::default()
        }
    }

    /// The row has to read at a glance: model, spend and what it is doing,
    /// with the name moved to the tooltip.
    #[test]
    fn a_worker_row_reads_model_tokens_and_live_state() {
        let worker = with_meta(
            "poodle",
            Some("running"),
            record("deepseek-v4.1-flash", "opencode-go", Some("high")),
        );
        assert_eq!(worker.model_label().as_deref(), Some("ds-4.1f"));
        assert_eq!(worker.tokens_text().as_deref(), Some("↑12k ↓3.1k"));
        assert_eq!(worker.icon, Some(jcode_core::id::session_icon("poodle")));
        // A running worker on a tool reads the tool, not "running".
        assert_eq!(worker.state_text(), "bash");
        assert_eq!(worker.status_text(), "bash 26m");
        let tooltip = worker.tooltip();
        assert!(tooltip.starts_with("poodle · "), "{tooltip}");
        assert!(
            tooltip.contains("opencode-go/deepseek-v4.1-flash"),
            "{tooltip}"
        );
        assert!(tooltip.contains("high effort"), "{tooltip}");
        assert!(tooltip.contains("↑12k ↓3.1k"), "{tooltip}");
    }

    /// A row draws the rung as the pill's circle, keyed by the normalized
    /// level. A record without effort claims nothing.
    #[test]
    fn a_worker_row_draws_the_effort_level() {
        let high = with_meta(
            "poodle",
            Some("running"),
            record("deepseek-v4.1-flash", "opencode-go", Some("high")),
        );
        assert_eq!(high.effort_level().as_deref(), Some("high"));

        let max = with_meta(
            "rose",
            Some("running"),
            record("gpt-5.6", "openai", Some("max")),
        );
        assert_eq!(max.effort_level().as_deref(), Some("max"));

        // Provider echoes carry stray case and whitespace.
        let echoed = with_meta(
            "ant",
            Some("running"),
            record("gpt-5.6", "openai", Some(" Low ")),
        );
        assert_eq!(echoed.effort_level().as_deref(), Some("low"));

        // A mode is orchestration, not a rung: it keeps its word.
        let swarm = with_meta(
            "bee",
            Some("running"),
            record("gpt-5.6", "openai", Some("swarm")),
        );
        assert_eq!(swarm.effort_level().as_deref(), Some("swarm"));

        // No record, or an empty one, shows no circle at all.
        assert_eq!(worker("rose", Some("running")).effort_level(), None);
        let blank = with_meta(
            "rose",
            Some("running"),
            record("gpt-5.6", "openai", Some(" ")),
        );
        assert_eq!(blank.effort_level(), None);
    }

    /// The report behind this change: `poodle compact-divider · running 26m`
    /// showed the same words for a worker that was only waiting.
    #[test]
    fn a_worker_waiting_on_the_model_does_not_read_as_running() {
        let mut meta = record("claude-opus-5-5", "claude-oauth", None);
        meta.phase = Some(WorkerPhase::Thinking);
        meta.tool = None;
        let thinking = with_meta("rose", Some("running"), meta);
        assert_eq!(thinking.model_label().as_deref(), Some("opus 5.5"));
        assert_eq!(thinking.status_text(), "thinking 26m");

        let ready = with_meta(
            "rose",
            Some("ready"),
            record("claude-opus-5-5", "claude-oauth", None),
        );
        assert_eq!(
            ready.status_text(),
            "ready",
            "a finished worker has no clock"
        );

        let blocked = with_meta("rose", Some("blocked"), WorkerMeta::default());
        assert_eq!(blocked.status_text(), "waiting 26m");

        // No record at all: the lifecycle word is all there is to say.
        assert_eq!(
            worker("rose", Some("running")).status_text(),
            "running 0s",
            "without a record the daemon's own stamp is the only clock"
        );
    }

    /// A subscription seat has no per-token price. Showing "$0.00" there
    /// would claim the work was free.
    #[test]
    fn opencode_go_cached_input_is_counted_once() {
        let mut meta = record("deepseek-v4-pro", "opencode-go", None);
        meta.input_tokens = Some(13_940_000);
        meta.output_tokens = Some(80_000);
        meta.cache_read_tokens = Some(13_570_000);
        let price = jcode_base::model_pricing::ModelCost {
            input_usd_per_mtok: 0.66,
            output_usd_per_mtok: 1.98,
            cache_read_usd_per_mtok: Some(0.0223),
            cache_write_usd_per_mtok: None,
        };
        let cost = cost_for_worker(&meta, price).unwrap();
        assert!((cost - 0.705211).abs() < 0.000001, "{cost}");
        let worker = with_meta("buffalo", Some("running"), meta);
        assert_eq!(worker.tokens_text().as_deref(), Some("↑13.9M ↓80k"));
    }

    #[test]
    fn cost_is_shown_only_where_tokens_have_a_price() {
        let oauth = with_meta(
            "rose",
            Some("running"),
            record("claude-opus-5-5", "claude-oauth", None),
        );
        assert_eq!(oauth.cost_text(), None);
        let copilot = with_meta("rose", Some("running"), record("gpt-5.6", "copilot", None));
        assert_eq!(copilot.cost_text(), None);
        assert_eq!(format_cost(1.234), "$1.23");
        assert_eq!(format_cost(0.0432), "$0.043");
        assert_eq!(format_cost(0.0004), "<$0.01");
    }

    #[test]
    fn compact_model_names_fit_a_one_line_row() {
        for (model, want) in [
            ("openai:gpt-6.1-sol", "sol 6.1"),
            ("openai:gpt-6-luna", "luna 6"),
            ("openai:gpt-6.1-terra", "terra 6.1"),
            ("gpt-6.1-sol", "sol 6.1"),
            ("deepseek-v4.1-flash", "ds-4.1f"),
            ("claude-opus-5-5", "opus 5.5"),
            ("opus-5-5", "opus 5.5"),
            ("gpt-5.6", "gpt 5.6"),
            ("gpt-5.6-mini", "gpt 5.6 mini"),
            ("claude-sonnet-4-5-thinking", "sonnet 4.5"),
            ("anthropic/claude-haiku-4-5", "haiku 4.5"),
            ("gemini-2.5-pro", "gem-2.5p"),
            ("kimi-k2-0905-preview", "kimi k2"),
            ("", ""),
        ] {
            assert_eq!(compact_model(model), want, "{model}");
        }
    }

    /// A worker's own newest message is fresher than the daemon's
    /// last-active stamp, so the row's clock follows the record.
    #[test]
    fn the_record_clock_wins_over_the_daemons_last_active_stamp() {
        let meta = WorkerMeta {
            last_activity_ms: Some(1_000_000 + 25 * 60 * 1_000),
            ..WorkerMeta::default()
        };
        let worker = with_meta("rose", Some("running"), meta);
        // `with_meta` ages against 1_000_000 + 26m, so the newer record is 1m.
        assert_eq!(worker.status_text(), "running 1m");
    }

    #[test]
    fn indicator_tooltip_lists_each_worker_with_its_own_state() {
        let mut meta = record("deepseek-v4.1-flash", "opencode-go", Some("high"));
        meta.phase = Some(WorkerPhase::Thinking);
        meta.last_activity_ms = None;
        let workers = vec![
            with_meta("poodle", Some("running"), meta),
            worker("rose", Some("blocked")),
        ];
        let indicator = Indicator::from_workers(&workers).unwrap();
        let lines = indicator.tooltip();
        assert!(lines.starts_with("poodle · "), "{lines}");
        assert!(lines.contains("thinking"), "{lines}");
        assert!(lines.contains("waiting"), "{lines}");
    }

    /// The composer pill's three segments: what runs now, what is parked, and
    /// what has reported. A finished worker still counts, so the bar can show
    /// the turn's result before it disappears.
    #[test]
    fn tally_splits_running_waiting_and_done() {
        let workers = vec![
            worker("rose", Some("running")),
            worker("fox", Some("blocked")),
            worker("otter", Some("queued")),
            worker("poodle", Some("ready")),
            worker("kangaroo", Some("failed")),
        ];
        let tally = SwarmTally::of(&workers);
        assert_eq!(
            tally,
            SwarmTally {
                running: 1,
                waiting: 2,
                done: 2
            }
        );
        assert_eq!(tally.total(), 5);
        assert_eq!(tally.summary(), "1 running · 2 waiting · 2 done");
        assert_eq!(
            SwarmTally::of(&[worker("rose", Some("running"))]).summary(),
            "1 running",
            "a uniform swarm reads as one phrase"
        );
        assert_eq!(SwarmTally::default().summary(), "");
    }
}
