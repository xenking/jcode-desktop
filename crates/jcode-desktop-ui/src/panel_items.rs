//! What a transcript is made of: its items, the cards they carry, and the
//! snapshot a reload keeps of a panel.
use super::*;

/// The transcript items, with a revision that advances on every mutable
/// access. Rendering derives rows, selectable text and pinned chrome from the
/// items; the revision lets it reuse them across frames that only move the
/// panel (camera travel, resizes) instead of rehashing the whole history.
/// Any `&mut` access counts as a change, so it can only over-invalidate.
/// Revisions come from one process-wide counter, so a transcript that is
/// replaced, taken or rebuilt never repeats a revision a cache has seen.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub(super) items: Vec<Item>,
    pub(super) revision: u64,
}

/// Last few user/assistant turns, newest last, for inline completion.
pub(super) fn completion_turns(items: &[Item]) -> Vec<crate::prompt_completion::Turn> {
    let mut turns: Vec<_> = items
        .iter()
        .rev()
        .filter_map(|item| match item {
            Item::User(text) => Some(("user", text.as_str())),
            _ => item.assistant_text().map(|text| ("assistant", text)),
        })
        .filter(|(_, text)| !text.trim().is_empty())
        .take(6)
        .map(|(role, text)| {
            // Keep the end of long turns: it is what the user is replying to.
            let start = text.len().saturating_sub(600);
            let start = (start..text.len())
                .find(|i| text.is_char_boundary(*i))
                .unwrap_or(0);
            crate::prompt_completion::Turn {
                role,
                text: text[start..].to_owned(),
            }
        })
        .collect();
    turns.reverse();
    turns
}

pub(super) fn next_transcript_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Default for Transcript {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl Transcript {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

impl std::ops::Deref for Transcript {
    type Target = Vec<Item>;
    fn deref(&self) -> &Vec<Item> {
        &self.items
    }
}

impl std::ops::DerefMut for Transcript {
    fn deref_mut(&mut self) -> &mut Vec<Item> {
        self.revision = next_transcript_revision();
        &mut self.items
    }
}

impl From<Vec<Item>> for Transcript {
    fn from(items: Vec<Item>) -> Self {
        Self {
            items,
            revision: next_transcript_revision(),
        }
    }
}

impl PartialEq<Vec<Item>> for Transcript {
    fn eq(&self, other: &Vec<Item>) -> bool {
        &self.items == other
    }
}

impl PartialEq for Transcript {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<'a> IntoIterator for &'a Transcript {
    type Item = &'a Item;
    type IntoIter = std::slice::Iter<'a, Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut Transcript {
    type Item = &'a mut Item;
    type IntoIter = std::slice::IterMut<'a, Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.revision = next_transcript_revision();
        self.items.iter_mut()
    }
}

/// One transcript entry, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Item {
    User(String),
    Image(TranscriptImage),
    Assistant(String),
    AssistantOutput {
        text: String,
        phase: AssistantOutputPhase,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
    },
    AgentMessage {
        from_session: String,
        from_name: Option<String>,
        message: String,
        /// Local arrival time for notifications, or the stored time for history.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timestamp_ms: Option<i64>,
    },
    SideQuestion(Box<side_question::inline::InlineQuestion>),
    ResponseStats(response_stats::ResponseStats),
    Reasoning(String),
    Tool {
        call_id: String,
        name: String,
        /// Raw tool arguments as streamed, used for the one-line summary and
        /// the expanded detail.
        input: String,
        output: String,
        done: bool,
        error: Option<String>,
    },
    /// Live state for work detached by a tool call. Unlike the originating
    /// `bg`/`bash` row, this continues to change while the agent waits.
    BackgroundTask {
        task_id: String,
        label: String,
        summary: String,
        #[serde(default)]
        detail: Option<String>,
        percent: Option<f32>,
        done: bool,
    },
    Todos(TodoCardPayload),
    /// A local UI/command failure, whose text is not a provider error.
    Error(String),
    /// A structured error reported by the runtime API.
    RuntimeError {
        code: jcode_sdk::api::ErrorCode,
        message: String,
    },
    Stopped(turn::StopNotice),
    SwarmWorkerNotice(String),
    ProviderGuardrailRetry {
        turn_id: String,
        text: String,
    },
    /// A completed or in-progress context compaction: one full-width rule with a
    /// centered label that expands to the summary and elided-message detail.
    Compaction(compaction::CompactionRow),
    /// An automatic memory injection: one full-width rule labelled
    /// `auto-recalled N memories` that expands to what was recalled. Live and
    /// reloaded rows share the body the daemon stores, so they cannot drift.
    Memory(compaction::CompactionRow),
    /// Daemon-reported KV (prompt) cache miss for a provider request.
    CacheMiss(cache_miss::CacheMissNotice),
    /// An applet instance placed in this transcript that is not anchored to
    /// a tool call. Only ever a derived render row, never stored in `items`.
    #[serde(skip)]
    Applet(String),
}

#[derive(Clone)]
pub(super) enum TranscriptRowSource {
    Settled(usize),
    // Almost every row only points into `Panel::items`. Keep the largest Item
    // variant out of every descriptor rebuilt during a frame. Only live text
    // and coalesced reasoning need owned storage.
    Owned(Box<Item>),
}

/// Everything a render derives from the transcript alone. Recomputing it
/// walks and hashes the whole history, which dominated frames that merely
/// move a panel, so it is reused while its key is unchanged.
pub(super) struct TranscriptDerived {
    pub(super) key: TranscriptDerivedKey,
    pub(super) rows: Arc<Vec<TranscriptRenderRow>>,
    pub(super) prompt_rows: Arc<Vec<(usize, usize)>>,
    pub(super) latest_todo: Option<Arc<TodoCardPayload>>,
    /// Ordinary and publish-tracker header fingerprints, computed on rebuild.
    pub(super) tail_headers: [u64; 2],
    pub(super) active_error: Option<usize>,
}

pub(super) fn transcript_tail_headers(payload: Option<&TodoCardPayload>) -> [u64; 2] {
    use std::hash::{Hash, Hasher};
    [false, true].map(|publish_tracker| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        publish_tracker.hash(&mut hasher);
        if !publish_tracker && let Some(payload) = payload.filter(|p| !p.todos.is_empty()) {
            payload.hash(&mut hasher);
        }
        hasher.finish()
    })
}

#[derive(Clone, PartialEq)]
pub(super) struct TranscriptDerivedKey {
    pub(super) items_revision: u64,
    pub(super) applet_rows_version: u64,
    /// Visible live text, reasoning, by pointer-free content hash.
    pub(super) streaming: u64,
    pub(super) expansions: u64,
    pub(super) display: crate::config::DisplayConfig,
    pub(super) clean_chat: bool,
}

#[derive(Clone)]
pub(super) struct TranscriptRenderRow {
    pub(super) index: usize,
    pub(super) source: TranscriptRowSource,
    #[allow(dead_code)]
    pub(super) role: Option<&'static str>,
    pub(super) show_label: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Hash, Serialize, Deserialize)]
pub struct TodoCardPayload {
    #[serde(default)]
    pub(super) todos: Vec<TodoCardItem>,
    #[serde(default)]
    pub(super) plan: TodoCardPlan,
    /// Goal-level quality assessments, one per todo group.
    #[serde(default)]
    pub(super) goals: Vec<jcode_base::todo::TodoGoal>,
}

impl From<jcode_sdk::todo::TodoSnapshot> for TodoCardPayload {
    fn from(snapshot: jcode_sdk::todo::TodoSnapshot) -> Self {
        Self {
            todos: snapshot
                .todos
                .into_iter()
                .map(|todo| TodoCardItem {
                    content: todo.content,
                    status: todo.status,
                    group: todo.group,
                    stage: todo.stage,
                    assigned_to: todo.assigned_to,
                    blocked_by: todo.blocked_by,
                    confidence: todo.confidence,
                    completion_confidence: todo.completion_confidence,
                })
                .collect(),
            plan: TodoCardPlan {
                user_intention: snapshot.plan.user_intention,
                understands_user_intent: snapshot.plan.understands_user_intent,
            },
            goals: snapshot.goals,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Hash, Serialize, Deserialize)]
pub(super) struct TodoCardPlan {
    #[serde(default)]
    pub(super) user_intention: Option<String>,
    #[serde(default, alias = "alignment_score", alias = "user_intention_alignment")]
    pub(super) understands_user_intent: Option<jcode_base::todo::IntentUnderstanding>,
}

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub(super) struct TodoCardItem {
    pub(super) content: String,
    pub(super) status: String,
    #[serde(default, alias = "lane")]
    pub(super) group: Option<String>,
    #[serde(
        default,
        alias = "phase",
        alias = "wave",
        deserialize_with = "deserialize_todo_stage"
    )]
    pub(super) stage: Option<u32>,
    #[serde(default)]
    pub(super) assigned_to: Option<String>,
    #[serde(default)]
    pub(super) blocked_by: Vec<String>,
    #[serde(default)]
    pub(super) confidence: Option<jcode_base::todo::ConfidenceState>,
    #[serde(default)]
    pub(super) completion_confidence: Option<jcode_base::todo::ConfidenceState>,
}

impl TodoCardItem {
    /// A todo the daemon made for one of the user's messages.
    pub(super) fn is_request(&self) -> bool {
        self.group.as_deref().map(str::trim) == Some(jcode_base::todo::REQUESTS_GROUP)
    }
}

pub(super) fn deserialize_todo_stage<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| match value {
            serde_json::Value::Number(number) => {
                number.as_u64().and_then(|n| u32::try_from(n).ok())
            }
            serde_json::Value::String(text) => text
                .trim()
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok(),
            _ => None,
        })
        .filter(|stage| *stage > 0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptImage {
    pub(super) media_type: String,
    pub(super) data: String,
    pub(super) label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) session_image: Option<jcode_sdk::api::SessionImageRef>,
    #[serde(skip)]
    pub(super) provisional_image_ordinal: Option<usize>,
    #[serde(skip)]
    pub(super) preview: Option<Arc<gpui::Image>>,
    pub(super) source: jcode_sdk::RenderedImageSource,
    pub(super) anchor: Option<jcode_sdk::RenderedImageAnchor>,
}

impl TranscriptImage {
    pub(super) fn lazy_key(&self) -> Option<&str> {
        self.data.strip_prefix("jcode-lazy:")
    }

    pub(super) fn new(media_type: String, data: String, label: Option<String>) -> Self {
        let preview = base64::engine::general_purpose::STANDARD
            .decode(&data)
            .ok()
            .and_then(|bytes| {
                Some(crate::image_cache::encoded(
                    gpui::ImageFormat::from_mime_type(&media_type)?,
                    bytes,
                ))
            });
        Self {
            media_type,
            data,
            label,
            preview,
            session_image: None,
            provisional_image_ordinal: None,
            source: jcode_sdk::RenderedImageSource::UserInput,
            anchor: None,
        }
    }

    pub(super) fn from_rendered(image: jcode_sdk::RenderedImage) -> Self {
        let mut transcript = Self::new(image.media_type, image.data, image.label);
        transcript.source = image.source;
        transcript.anchor = image.anchor;
        transcript.session_image = image.session_image;
        transcript
    }

    pub(super) fn has_image_id(&self, id: &str) -> bool {
        self.session_image.as_ref().is_some_and(|image| image.image_id == id)
            || self.data.strip_prefix("jcode-lazy:") == Some(id)
            || self.label.as_deref().is_some_and(|label| label.contains(&format!("[#{id}]")))
    }

    pub(super) fn remember_image_id(&mut self, id: &str) {
        let marker = format!("[#{id}]");
        let label = self.label.get_or_insert_with(String::new);
        if !label.contains(&marker) {
            if !label.is_empty() {
                label.push(' ');
            }
            label.push_str(&marker);
        }
    }

    pub(super) fn image_ordinal(&self) -> Option<usize> {
        self.session_image.as_ref().map(|image| image.ordinal).or(self.provisional_image_ordinal)
    }

    pub(super) fn display_label(&self, fallback: &str) -> String {
        self.image_ordinal()
            .map(|number| format!("Image #{number}"))
            .or_else(|| {
                self.label.clone().and_then(|mut label| {
                    for range in crate::input::image_marker_ranges(&label).into_iter().rev() {
                        if crate::image_chip::session_image_key(&label[range.clone()]).is_some() {
                            label.replace_range(range, "");
                        }
                    }
                    let label = label.trim();
                    (!label.is_empty()).then(|| label.to_owned())
                })
            })
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub(super) fn model_input_caption(&self) -> Option<&'static str> {
        matches!(
            self.source,
            jcode_sdk::RenderedImageSource::ToolResult { .. }
        )
        .then_some("Image provided to model")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PanelSnapshot {
    #[serde(default, skip_serializing_if = "DraftConfig::is_empty")]
    pub(crate) draft_config: DraftConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) clean_chat_override: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<TranscriptSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) turn_outcome: Option<turn::TurnOutcome>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) finished_unseen: bool,
    #[serde(default)]
    pub image_pane_open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_document: Option<SideDocumentSnapshot>,
    #[serde(default)]
    pub prompt_queue: queue::PromptQueue,
    pub session_id: String,
    pub title: String,
    pub working_dir: Option<String>,
    pub draft: PromptInputSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_draft: Option<ask::AskDraft>,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub stick_to_bottom: bool,
    pub terminal_resource_id: Option<u64>,
    #[serde(default)]
    pub terminal_output_cursor: Option<u64>,
    #[serde(default)]
    pub startup_layout: Option<StartupLayout>,
}
