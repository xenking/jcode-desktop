//! What a swarm worker has spent, read from its own session files: the model
//! and provider it runs on and the total tokens it has consumed. The
//! coordinator's `swarm` tool call carries none of this, so the transcript
//! card reads it here instead of guessing from the call's own output size.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkerUsage {
    pub model: Option<String>,
    pub provider: Option<String>,
    /// Sum over every model call of the context it sent plus what it wrote,
    /// the same figure `swarm status` reports as "tok total".
    pub total_tokens: u64,
}

#[derive(serde::Deserialize, Default)]
struct Meta {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider_key: Option<String>,
}

#[derive(serde::Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

impl Usage {
    /// Providers that report `prompt_tokens` already folded the cache in.
    fn total(&self) -> u64 {
        let prompt = self.prompt_tokens.unwrap_or_else(|| {
            self.input_tokens
                .saturating_add(self.cache_read_input_tokens.unwrap_or(0))
                .saturating_add(self.cache_creation_input_tokens.unwrap_or(0))
        });
        prompt.saturating_add(self.output_tokens)
    }
}

#[derive(serde::Deserialize)]
struct Message {
    #[serde(default)]
    token_usage: Option<Usage>,
}

#[derive(serde::Deserialize, Default)]
struct Snapshot {
    #[serde(flatten)]
    meta: Meta,
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(serde::Deserialize, Default)]
struct JournalLine {
    #[serde(default)]
    meta: Meta,
    #[serde(default)]
    append_messages: Vec<Message>,
}

/// Usage of `session_id` under `home/sessions`: the snapshot plus every
/// journal line appended since. Newer model/provider metadata wins. `None`
/// when the session has no record at all.
pub fn worker_usage(home: &Path, session_id: &str) -> Option<WorkerUsage> {
    let dir = home.join("sessions");
    let snapshot: Option<Snapshot> = std::fs::read(dir.join(format!("{session_id}.json")))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let journal = std::fs::read_to_string(dir.join(format!("{session_id}.journal.jsonl")))
        .ok()
        .unwrap_or_default();
    let journal: Vec<JournalLine> = journal
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if snapshot.is_none() && journal.is_empty() {
        return None;
    }
    let snapshot = snapshot.unwrap_or_default();
    let mut usage = WorkerUsage {
        model: snapshot.meta.model.filter(|m| !m.is_empty()),
        provider: snapshot.meta.provider_key.filter(|p| !p.is_empty()),
        total_tokens: 0,
    };
    for message in &snapshot.messages {
        usage.total_tokens += message.token_usage.as_ref().map_or(0, Usage::total);
    }
    for line in journal {
        usage.model = line.meta.model.filter(|m| !m.is_empty()).or(usage.model);
        usage.provider = line.meta.provider_key.filter(|p| !p.is_empty()).or(usage.provider);
        for message in &line.append_messages {
            usage.total_tokens += message.token_usage.as_ref().map_or(0, Usage::total);
        }
    }
    Some(usage)
}

type Stamp = (u64, Option<SystemTime>, u64, Option<SystemTime>);

static CACHE: Mutex<Option<HashMap<PathBuf, (Stamp, WorkerUsage)>>> = Mutex::new(None);

fn stamp(home: &Path, session_id: &str) -> Stamp {
    let meta = |name: String| {
        std::fs::metadata(home.join("sessions").join(name))
            .map(|m| (m.len(), m.modified().ok()))
            .unwrap_or((0, None))
    };
    let (a, am) = meta(format!("{session_id}.json"));
    let (b, bm) = meta(format!("{session_id}.journal.jsonl"));
    (a, am, b, bm)
}

/// [`worker_usage`] for many sessions, re-reading only files that changed.
/// Transcripts reach hundreds of KB, and the sidebar refreshes every few
/// seconds, so an unchanged finished worker must cost a `stat`, not a parse.
pub fn usage_for(home: &Path, session_ids: &[String]) -> HashMap<String, WorkerUsage> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    let mut out = HashMap::new();
    for id in session_ids {
        let key = home.join(id);
        let now = stamp(home, id);
        if let Some((seen, usage)) = cache.get(&key).filter(|(seen, _)| *seen == now) {
            let _ = seen;
            out.insert(id.clone(), usage.clone());
            continue;
        }
        if let Some(usage) = worker_usage(home, id) {
            cache.insert(key, (now, usage.clone()));
            out.insert(id.clone(), usage);
        }
    }
    out
}

/// `7.7M`, `319k`, `842`: short enough for a one-line card.
pub fn format_tokens(count: u64) -> String {
    match count {
        1_000_000.. => format!("{:.1}M", count as f64 / 1e6),
        10_000.. => format!("{}k", count / 1000),
        1_000.. => format!("{:.1}k", count as f64 / 1e3),
        _ => count.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/usage_home")
    }

    /// Real token records copied from `session_rose_...`: 60 model calls whose
    /// `prompt_tokens + output_tokens` sum to 4,478,262. The coordinator's
    /// `swarm` call output is a few dozen tokens, which is what the old card
    /// showed as "~15 tok".
    #[test]
    fn total_is_the_workers_own_spend_not_the_tool_call_size() {
        let usage = worker_usage(&fixtures(), "session_rose_1794717290730_574aea8d9f6adcc3").unwrap();
        assert_eq!(usage.total_tokens, 4_478_262);
        assert_eq!(usage.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(usage.provider.as_deref(), Some("claude-oauth"));
    }

    /// The snapshot lags the journal. Messages appended since, and the model
    /// the worker switched to, both count. The snapshot's `claude-opus-5-5` is
    /// an edited stale value to prove the journal's metadata wins.
    #[test]
    fn journal_lines_extend_the_snapshot_and_override_its_model() {
        let usage = worker_usage(&fixtures(), "session_badger_1794720615830_8c546c8d2a5870c4").unwrap();
        assert_eq!(usage.total_tokens, 140_690 + 325_554);
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-5-5"));
    }

    #[test]
    fn unknown_sessions_have_no_usage() {
        assert_eq!(worker_usage(&fixtures(), "session_nobody_1_a"), None);
    }

    #[test]
    fn cache_rereads_a_file_only_when_it_changed() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("w.json");
        std::fs::write(&path, r#"{"model":"m","messages":[{"token_usage":{"prompt_tokens":10,"output_tokens":5}}]}"#).unwrap();
        let ids = vec!["w".to_string()];
        assert_eq!(usage_for(home.path(), &ids)["w"].total_tokens, 15);
        std::fs::write(&path, r#"{"model":"m","messages":[{"token_usage":{"prompt_tokens":10,"output_tokens":5}},{"token_usage":{"input_tokens":1,"cache_read_input_tokens":2,"output_tokens":3}}]}"#).unwrap();
        assert_eq!(usage_for(home.path(), &ids)["w"].total_tokens, 21);
    }

    #[test]
    fn short_token_labels() {
        assert_eq!(format_tokens(842), "842");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_tokens(319_512), "319k");
        assert_eq!(format_tokens(7_700_000), "7.7M");
    }
}
