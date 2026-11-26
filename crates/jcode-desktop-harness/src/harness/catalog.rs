//! The persisted session store: the sidebar catalog, unfinished work and live
//! sessions as read from the jcode home directory.

mod refresh;
mod stamp;

pub(super) use refresh::SessionRefresh;
pub(super) use stamp::EditWatch;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use jcode_sdk::SessionInfo;

use crate::harness::remote;

use crate::harness::protocol::{CatalogTodos, UnfinishedSession, UnfinishedTodo};
use crate::platform::jcode_home;

pub(super) const SIDEBAR_METADATA_WINDOW: usize = 64 * 1024;

#[derive(serde::Deserialize)]
pub(super) struct PersistedSession {
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    custom_title: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    pinned_skills: Vec<String>,
    #[serde(default)]
    pinned_skill: Option<String>,
    #[serde(default)]
    saved: bool,
    #[serde(default)]
    save_label: Option<String>,
    #[serde(default)]
    status: serde_json::Value,
    /// The process that last owned the session. An `Active` record whose
    /// owner is gone crashed without writing its final status.
    #[serde(default)]
    last_pid: Option<u32>,
    /// Session this one was created from: a swarm coordinator, or a user fork.
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    agent_started: bool,
    #[serde(default)]
    is_swarm_worker: bool,
    #[serde(default)]
    side_question_message_start: Option<usize>,
    /// Older headless workers used the debug flag before storing worker provenance.
    #[serde(default)]
    is_debug: bool,
    #[serde(default)]
    hidden: bool,
}

impl PersistedSession {
    fn catalog_context(&self) -> Option<jcode_sdk::api::SessionCatalogContext> {
        let modes = if self.pinned_skills.is_empty() {
            self.pinned_skill.iter().cloned().collect()
        } else {
            self.pinned_skills.clone()
        };
        let activity = (self.agent_started
            || self.is_swarm_worker
            || self.hidden
            || self.side_question_message_start.is_some())
        .then_some(jcode_sdk::api::SessionCatalogActivity {
            origin: jcode_sdk::api::SessionOrigin::Agent,
            last_agent_activity_at_ms: None,
            last_message_at_ms: None,
        });
        (self.model.is_some()
            || self.reasoning_effort.is_some()
            || !modes.is_empty()
            || activity.is_some())
        .then(|| jcode_sdk::api::SessionCatalogContext {
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            modes,
            cache_expires_at_ms: None,
            activity,
        })
    }

    /// The coordinator that owns this session. A transcript fork also has a
    /// `parent_id`, but only daemon-spawned workers are `is_debug`, and only
    /// swarm ownership may nest a session under another in the sidebar.
    fn swarm_parent(&self) -> Option<String> {
        self.parent_id
            .clone()
            .filter(|parent| self.is_debug && !self.hidden && !parent.trim().is_empty())
    }

    fn is_private_side_question(&self) -> bool {
        self.hidden && self.parent_id.is_some()
    }

    /// Lifecycle status as the TUI picker reports it, including crashes the
    /// dead process never had the chance to persist.
    fn lifecycle_status(&self) -> String {
        let status = persisted_session_status(&self.status);
        match self.last_pid {
            Some(pid) if status == "active" && !jcode_base::platform::is_process_running(pid) => {
                "crashed".into()
            }
            _ => status,
        }
    }
}

pub(super) fn persisted_session_status(status: &serde_json::Value) -> String {
    let name = status
        .as_str()
        .or_else(|| {
            status
                .as_object()
                .and_then(|status| status.keys().next().map(String::as_str))
        })
        .unwrap_or("idle");
    match name.to_ascii_lowercase().as_str() {
        "active" | "closed" | "crashed" | "reloaded" | "compacted" | "ratelimited"
        | "rate_limited" | "error" => name.to_ascii_lowercase(),
        _ => "idle".into(),
    }
}

#[derive(serde::Deserialize)]
pub(super) struct PersistedTodoTitleItem {
    content: String,
    status: String,
    #[serde(default)]
    group: Option<String>,
}

pub(super) fn catalog_todos_in(home: &Path, sessions: &[SessionInfo]) -> CatalogTodos {
    let mut out = CatalogTodos::default();
    let mut unfinished = Vec::new();
    let todos_dir = home.join("todos");
    for session in sessions
        .iter()
        .filter(|s| !remote::is_remote(&s.session_id))
    {
        let Some(todos) = read_todo_items(&todos_dir, &session.session_id) else {
            continue;
        };
        out.rows.insert(
            session.session_id.clone(),
            todos
                .iter()
                .take(100)
                .map(|t| (t.content.clone(), t.status.clone()))
                .collect(),
        );
        if todos.is_empty() {
            continue;
        }
        let done = todos
            .iter()
            .filter(|todo| todo.status.eq_ignore_ascii_case("completed"))
            .count();
        out.progress
            .insert(session.session_id.clone(), (done, todos.len()));
        if let Some(entry) = unfinished_session(home, session, todos) {
            unfinished.push((session.last_active_at_ms.max(session.updated_at_ms), entry));
        }
    }
    unfinished.sort_by_key(|(recency, _)| std::cmp::Reverse(*recency));
    out.unfinished = unfinished.into_iter().map(|(_, entry)| entry).collect();
    out
}

pub(super) fn read_todo_items(dir: &Path, id: &str) -> Option<Vec<PersistedTodoTitleItem>> {
    let bytes = std::fs::read(dir.join(format!("{id}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(super) fn unfinished_session(
    home: &Path,
    session: &SessionInfo,
    todos: Vec<PersistedTodoTitleItem>,
) -> Option<UnfinishedSession> {
    if matches!(session.status.as_str(), "active" | "running" | "working") {
        return None;
    }
    let todos = todos
        .into_iter()
        .filter(|todo| !todo.status.eq_ignore_ascii_case("completed"))
        .map(|todo| UnfinishedTodo {
            content: todo.content,
            status: todo.status,
            group: todo.group,
        })
        .collect::<Vec<_>>();
    (!todos.is_empty()).then(|| UnfinishedSession {
        session_id: session.session_id.clone(),
        title: session
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .or_else(|| persisted_todo_title(home, &session.session_id))
            .unwrap_or_else(|| session.session_id.clone()),
        working_dir: session.working_dir.clone(),
        todos,
    })
}

/// Read the durable todo snapshots for sessions which are no longer open.
/// This intentionally avoids transcripts, so opening the dashboard remains cheap.
pub fn unfinished_sessions(sessions: &[SessionInfo]) -> Vec<UnfinishedSession> {
    let Some(home) = jcode_home() else {
        return Vec::new();
    };
    unfinished_sessions_in(&home, sessions)
}

pub fn unfinished_sessions_in(home: &Path, sessions: &[SessionInfo]) -> Vec<UnfinishedSession> {
    let todos_dir = home.join("todos");
    sessions
        .iter()
        .filter(|session| !remote::is_remote(&session.session_id))
        .filter(|session| !matches!(session.status.as_str(), "active" | "running" | "working"))
        .filter_map(|session| {
            unfinished_session(
                home,
                session,
                read_todo_items(&todos_dir, &session.session_id)?,
            )
        })
        .collect()
}

/// Completed and total todos per local session, from durable todo snapshots.
/// Sessions without todos are omitted. Cheap enough for a background refresh.
pub fn todo_progress_in(home: &Path, sessions: &[SessionInfo]) -> HashMap<String, (usize, usize)> {
    let todos_dir = home.join("todos");
    sessions
        .iter()
        .filter(|session| !remote::is_remote(&session.session_id))
        .filter_map(|session| {
            let todos = read_todo_items(&todos_dir, &session.session_id)?;
            (!todos.is_empty()).then(|| {
                let done = todos
                    .iter()
                    .filter(|todo| todo.status.eq_ignore_ascii_case("completed"))
                    .count();
                (session.session_id.clone(), (done, todos.len()))
            })
        })
        .collect()
}

pub(super) fn persisted_todo_snapshot(session_id: &str) -> Option<jcode_sdk::todo::TodoSnapshot> {
    use jcode_sdk::todo::store;
    if !store::is_safe_session_id(session_id) {
        return None;
    }
    let snapshot = store::load_snapshot(&jcode_home()?, session_id);
    (!snapshot.todos.is_empty()).then_some(snapshot)
}

/// One session a live process owns, for the orchestration panel. Built from
/// presence markers and durable todo snapshots only, never from a transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSession {
    pub session_id: String,
    pub title: String,
    pub working_dir: Option<String>,
    /// A model turn is in flight right now.
    pub running: bool,
    pub todos: Vec<UnfinishedTodo>,
}

impl LiveSession {
    pub fn completed_todos(&self) -> usize {
        self.todos
            .iter()
            .filter(|todo| todo.status.eq_ignore_ascii_case("completed"))
            .count()
    }

    pub fn as_unfinished(&self) -> UnfinishedSession {
        UnfinishedSession {
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            working_dir: self.working_dir.clone(),
            todos: self.todos.clone(),
        }
    }
}

/// Every local session whose owning process is alive. Running sessions sort
/// first, then the most recently created. Cheap enough to poll each second:
/// it reads presence markers, bounded session metadata and todo files.
pub fn live_sessions() -> Vec<LiveSession> {
    let Some(home) = jcode_home() else {
        return Vec::new();
    };
    let presence = jcode_base::session::session_presence()
        .into_iter()
        .map(|presence| (presence.session_id, presence.streaming));
    live_sessions_from(&home, presence)
}

pub fn live_sessions_from(
    home: &Path,
    presence: impl IntoIterator<Item = (String, bool)>,
) -> Vec<LiveSession> {
    let todos_dir = home.join("todos");
    let mut sessions = presence
        .into_iter()
        .filter(|(session_id, _)| !remote::is_remote(session_id))
        .map(|(session_id, running)| {
            let path = home.join("sessions").join(format!("{session_id}.json"));
            let record = std::fs::metadata(&path)
                .ok()
                .and_then(|metadata| read_persisted_session(&path, metadata.len()));
            let todos: Vec<PersistedTodoTitleItem> =
                std::fs::read(todos_dir.join(format!("{session_id}.json")))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default();
            let non_empty = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
            let title = record
                .as_ref()
                .and_then(|record| {
                    non_empty(record.custom_title.clone())
                        .or_else(|| non_empty(record.title.clone()))
                })
                .or_else(|| persisted_todo_title(home, &session_id))
                .unwrap_or_else(|| session_id.clone());
            LiveSession {
                title,
                working_dir: record.and_then(|record| record.working_dir),
                running,
                todos: todos
                    .into_iter()
                    .map(|todo| UnfinishedTodo {
                        content: todo.content,
                        status: todo.status,
                        group: todo.group,
                    })
                    .collect(),
                session_id,
            }
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then_with(|| session_recency_ms(&b.session_id).cmp(&session_recency_ms(&a.session_id)))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    sessions
}

#[derive(serde::Deserialize, Default)]
pub(super) struct PersistedTodoTitlePlan {
    #[serde(default)]
    user_intention: Option<String>,
}

/// Match the title precedence used by the TUI's `/resume` picker without
/// loading a transcript: current todo group, plan intention, then todo text.
pub(super) fn persisted_todo_title(home: &Path, session_id: &str) -> Option<String> {
    if remote::is_remote(session_id) {
        return None;
    }
    let todos_dir = home.join("todos");
    let todos: Vec<PersistedTodoTitleItem> =
        std::fs::read(todos_dir.join(format!("{session_id}.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
    let plan: PersistedTodoTitlePlan =
        std::fs::read(todos_dir.join(format!("{session_id}-plan.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
    let current = todos
        .iter()
        .rev()
        .find(|todo| todo.status.eq_ignore_ascii_case("in_progress"))
        .or_else(|| {
            todos
                .iter()
                .rev()
                .find(|todo| !todo.status.eq_ignore_ascii_case("completed"))
        })
        .or_else(|| todos.last());
    let non_empty = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    current
        .and_then(|todo| non_empty(todo.group.as_deref()))
        .or_else(|| non_empty(plan.user_intention.as_deref()))
        .or_else(|| current.and_then(|todo| non_empty(Some(&todo.content))))
}

pub(super) fn json_string_field(bytes: &[u8], field: &str, last: bool) -> Option<String> {
    let needle = format!("\"{field}\"");
    let mut starts = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle.as_bytes()).then_some(index));
    let index = if last {
        starts.next_back()?
    } else {
        starts.next()?
    };
    let mut value = &bytes[index + needle.len()..];
    value = value.strip_prefix(b":")?.trim_ascii_start();
    serde_json::Deserializer::from_slice(value)
        .into_iter::<Option<String>>()
        .next()?
        .ok()
        .flatten()
}

/// Session transcripts can be hundreds of megabytes, while the sidebar only
/// needs fields stored before and after the messages array. Read small windows
/// from both ends instead of asking serde to walk every message.
pub(super) fn read_persisted_session(path: &Path, bytes: u64) -> Option<PersistedSession> {
    use std::io::{Read, Seek, SeekFrom};

    if bytes <= (SIDEBAR_METADATA_WINDOW * 2) as u64 {
        return serde_json::from_reader(std::fs::File::open(path).ok()?).ok();
    }

    let mut file = std::fs::File::open(path).ok()?;
    let mut head = vec![0; SIDEBAR_METADATA_WINDOW];
    file.read_exact(&mut head).ok()?;
    file.seek(SeekFrom::End(-(SIDEBAR_METADATA_WINDOW as i64)))
        .ok()?;
    let mut tail = vec![0; SIDEBAR_METADATA_WINDOW];
    file.read_exact(&mut tail).ok()?;

    Some(PersistedSession {
        working_dir: json_string_field(&tail, "working_dir", true),
        title: json_string_field(&head, "title", false),
        custom_title: json_string_field(&head, "custom_title", false)
            .or_else(|| json_string_field(&tail, "custom_title", true)),
        model: json_string_field(&tail, "model", true),
        reasoning_effort: json_string_field(&tail, "reasoning_effort", true),
        pinned_skills: json_value_field(&tail, "pinned_skills", true)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default(),
        pinned_skill: json_string_field(&tail, "pinned_skill", true),
        saved: json_value_field(&tail, "saved", true)
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        save_label: json_string_field(&tail, "save_label", true)
            .or_else(|| json_string_field(&head, "save_label", false)),
        status: json_value_field(&tail, "status", true).unwrap_or_default(),
        last_pid: json_value_field(&tail, "last_pid", true)
            .and_then(|value| value.as_u64())
            .and_then(|pid| u32::try_from(pid).ok()),
        parent_id: json_string_field(&head, "parent_id", false),
        agent_started: json_value_field(&head, "agent_started", false)
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        is_swarm_worker: json_value_field(&tail, "is_swarm_worker", true)
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        side_question_message_start: json_value_field(&head, "side_question_message_start", false)
            .and_then(|value| value.as_u64())
            .and_then(|value| usize::try_from(value).ok()),
        is_debug: json_value_field(&tail, "is_debug", true)
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        hidden: json_value_field(&tail, "hidden", true)
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    })
}

pub(super) fn json_value_field(bytes: &[u8], field: &str, last: bool) -> Option<serde_json::Value> {
    let needle = format!("\"{field}\"");
    let mut starts = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle.as_bytes()).then_some(index));
    let index = if last {
        starts.next_back()?
    } else {
        starts.next()?
    };
    let mut value = &bytes[index + needle.len()..];
    value = value.strip_prefix(b":")?.trim_ascii_start();
    serde_json::Deserializer::from_slice(value)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

/// The sidebar is a recency view, not an archive browser. Session files include
/// the complete transcript, so parsing an unbounded store can mean reading many
/// gigabytes before the first row appears.
pub(super) const MAX_PERSISTED_SIDEBAR_SESSIONS: usize = 100;

pub(super) fn session_recency_ms(session_id: &str) -> Option<u128> {
    session_id
        .split('_')
        .filter_map(|part| part.parse::<u64>().ok())
        .find(|value| (1_000_000_000_000..10_000_000_000_000).contains(value))
        .map(u128::from)
}

pub(super) fn file_recency_ms(entry: &std::fs::DirEntry) -> u128 {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub(super) fn side_question_visibility(home: &Path) -> HashMap<String, bool> {
    // The marker overrides the immutable hidden flag on promoted transcripts.
    let dir = home
        .join("state")
        .join(jcode_session_types::SIDE_QUESTION_VISIBILITY_DIR);
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let hidden = match std::fs::read(&path).ok()?.as_slice() {
                b"true" => true,
                b"false" => false,
                _ => return None,
            };
            Some((path.file_stem()?.to_str()?.to_owned(), hidden))
        })
        .collect()
}

/// Merge the API's live view with records on disk. This deliberately makes the
/// desktop resilient to an older already-running bridge that only reports
/// sessions created during its lifetime.
pub fn merge_persisted_sessions(
    mut sessions: Vec<SessionInfo>,
    home: Option<&Path>,
) -> Vec<SessionInfo> {
    sessions.retain(|session| !session.archived);
    let Some(home) = home else {
        return sessions;
    };

    let archived = std::fs::read_to_string(home.join("sdk-archive.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("sessions").and_then(|v| v.as_object()).cloned())
        .map(|entries| {
            entries
                .into_iter()
                .map(|(id, _)| id)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let visibility = side_question_visibility(home);
    // The harness list API reports attachment state, not the durable lifecycle
    // state used by the TUI picker. Read the same bounded session records so a
    // crashed or errored session does not get flattened to a generic idle row.
    sessions.retain_mut(|session| {
        if archived.contains(&session.session_id) {
            return false;
        }
        if remote::is_remote(&session.session_id) {
            return true;
        }
        let hidden = visibility.get(&session.session_id).copied();
        if hidden == Some(true) {
            return false;
        }
        let path = home
            .join("sessions")
            .join(format!("{}.json", session.session_id));
        let bytes = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
        if let Some(record) = read_persisted_session(&path, bytes.unwrap_or_default()) {
            if hidden.is_none() && record.is_private_side_question() {
                return false;
            }
            session.status = record.lifecycle_status();
            if let Some(local) = record.catalog_context() {
                let context = session.context.get_or_insert_with(Default::default);
                if context.model.is_none()
                    && context.reasoning_effort.is_none()
                    && context.cache_expires_at_ms.is_none()
                    && context.modes.is_empty()
                {
                    context.model = local.model;
                    context.reasoning_effort = local.reasoning_effort;
                    context.modes = local.modes;
                }
                context.activity = context.activity.or(local.activity);
            }
            if session.parent_session_id.is_none() {
                session.parent_session_id = record.swarm_parent();
            }
        }
        if hidden == Some(false) {
            session.parent_session_id = None;
            session.agent_label = None;
            session.swarm_status = None;
        }
        if session.parent_session_id.is_some() && session.agent_label.is_none() {
            session.agent_label = persisted_todo_title(home, &session.session_id);
        }
        true
    });

    // A full limited API page is authoritative. Modern bridges source it from
    // the compact metadata index, so rescanning a 100k-file transcript directory
    // here would discard the entire latency win. Keep the disk walk only as a
    // compatibility fallback for old bridges that return a partial/empty list.
    if sessions.len() >= MAX_PERSISTED_SIDEBAR_SESSIONS {
        for session in &mut sessions {
            if session
                .title
                .as_ref()
                .is_none_or(|title| title.trim().is_empty())
            {
                session.title = persisted_todo_title(home, &session.session_id);
            }
        }
        sessions.reverse();
        return sessions;
    }

    let mut known = sessions
        .iter()
        .map(|session| session.session_id.clone())
        .collect::<HashSet<_>>();
    let mut modified_by_id = HashMap::new();
    let mut disk_candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join("sessions")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if archived.contains(id) || visibility.get(id) == Some(&true) {
                continue;
            }
            // A transcript is rewritten whenever the session receives new
            // activity, so its modification time represents the user's latest
            // interaction. The timestamp embedded in the id is only a fallback
            // for stores where file metadata is unavailable.
            let modified = file_recency_ms(&entry);
            let recency = if modified == 0 {
                session_recency_ms(id).unwrap_or_default()
            } else {
                modified
            };
            modified_by_id.insert(id.to_string(), recency);
            if known.contains(id) {
                continue;
            }
            disk_candidates.push((recency, id.to_string(), path));
        }
    }

    // Select by cheap filesystem metadata first. Deserializing a session also
    // walks its `messages` array even though PersistedSession ignores that field.
    disk_candidates.sort_by_key(|(recency, ..)| std::cmp::Reverse(*recency));
    disk_candidates.truncate(MAX_PERSISTED_SIDEBAR_SESSIONS);

    let mut disk_sessions = Vec::new();
    for (recency, id, path) in disk_candidates {
        if !known.insert(id.clone()) {
            continue;
        }
        let transcript_bytes = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
        let Some(record) = read_persisted_session(&path, transcript_bytes.unwrap_or_default())
        else {
            continue;
        };
        if !visibility.contains_key(&id) && record.is_private_side_question() {
            continue;
        }
        let status = record.lifecycle_status();
        let parent_session_id = record.swarm_parent();
        let todo_title = persisted_todo_title(home, &id);
        let agent_label = parent_session_id.as_ref().and(todo_title.clone());
        let context = record.catalog_context();
        let save_label = record
            .saved
            .then_some(record.save_label)
            .flatten()
            .filter(|label| !label.trim().is_empty());
        let title = record
            .custom_title
            .filter(|title| !title.trim().is_empty())
            .or_else(|| save_label.clone())
            .or(todo_title)
            .or_else(|| record.title.filter(|title| !title.trim().is_empty()));
        disk_sessions.push((
            recency,
            SessionInfo {
                user_connected: false,
                session_id: id,
                working_dir: record.working_dir,
                title,
                status,
                transcript_bytes,
                saved: record.saved,
                updated_at_ms: i64::try_from(recency).ok(),
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                save_label,
                parent_session_id,
                agent_label,
                swarm_status: None,
                edit_stats: None,
                context,
            },
        ));
    }
    sessions.extend(disk_sessions.into_iter().map(|(_, session)| session));
    for session in &mut sessions {
        if session
            .title
            .as_ref()
            .is_none_or(|title| title.trim().is_empty())
        {
            session.title = persisted_todo_title(home, &session.session_id);
        }
    }
    sessions.sort_by_key(|session| {
        modified_by_id
            .get(&session.session_id)
            .copied()
            .unwrap_or_else(|| session_recency_ms(&session.session_id).unwrap_or_default())
    });
    sessions
}

#[cfg(test)]
mod tests;
