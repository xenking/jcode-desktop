//! The values the prompt is made of: attachments, snapshots, suggestions
//! and its small state records.
use super::*;
use std::path::PathBuf;

/// What choosing a context row does: the verbs of the `@` and `/` pickers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Browse,
    Attach,
    File(PathBuf),
    Skill(String),
    /// Open the MCP submenu, and its way back.
    Mcp,
    McpBack,
    /// A configured MCP server. Listed for reference; choosing it does nothing.
    McpServer(String),
}

/// A file or skill the draft references through a chip.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) enum DraftContext {
    File {
        path: PathBuf,
        contents: Option<String>,
    },
    Skill(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PasteMode {
    Attachments,
    TextAttachments,
    InlineText,
}

/// Recent transcript turns for the completion request.
pub(super) type CompletionContext = Box<dyn Fn(&App) -> Vec<crate::prompt_completion::Turn>>;

pub(super) struct GhostState {
    pub(super) machine: crate::prompt_completion::GhostCompletion,
    pub(super) context: CompletionContext,
    /// Debounce plus request. Dropping it cancels both.
    pub(super) task: Option<gpui::Task<()>>,
    /// The backend refused (unsupported or signed out). Stop asking.
    pub(super) disabled: bool,
    /// Key includes draft, conversation and project, so results cannot leak across contexts.
    pub(super) cache: std::collections::VecDeque<(String, String)>,
    #[cfg(test)]
    pub(super) request_counter: Option<std::rc::Rc<std::cell::Cell<usize>>>,
}

/// Example prompts chosen for one seed, project and personalized catalog.
pub(super) struct PickedPrompts {
    pub(super) seed: usize,
    /// Identity of the personalized catalog they were picked from.
    pub(super) source: Option<usize>,
    pub(super) working_dir: Option<String>,
    pub(super) prompts: [SharedString; 2],
}

/// Placeholder typewriter and caret motion. Never snapshotted.
pub(super) struct MotionState {
    /// Last content or caret change. Motion settles relative to this.
    pub(super) epoch: Instant,
    pub(super) key: Option<(SharedString, usize)>,
    pub(super) seed: usize,
    /// Example prompts picked for `seed`, with the inputs they came from.
    pub(super) prompts: Option<PickedPrompts>,
    pub(super) glide: Option<motion::Glide>,
    /// Delay until the painted motion next changes. `None` once settled.
    pub(super) next: Option<Duration>,
    pub(super) ticker: Option<gpui::Task<()>>,
}

impl Default for MotionState {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos() as usize);
        Self {
            epoch: Instant::now(),
            key: None,
            seed,
            prompts: None,
            glide: None,
            next: None,
            ticker: None,
        }
    }
}

pub(super) fn motion_reduced(cx: &App) -> bool {
    cx.reduce_motion()
        || crate::config::get().appearance.reduce_motion
        || crate::harness::screenshot_mode()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandSuggestion {
    pub(super) value: String,
    pub(super) help: String,
    pub(super) active_choice: bool,
    pub(super) detail: Option<String>,
    pub(super) header: Option<String>,
    pub(super) toggle: Option<String>,
    /// What Enter or a click does inside the model picker, in place of
    /// submitting `value`.
    pub(super) action: Option<model_stage::StageStep>,
    pub(super) context_action: Option<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SubmissionKind {
    Message,
    Command,
    Skill(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandMenuClose {
    Completed,
    Literal,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct SubmittedFile {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub data: String,
}

#[derive(Clone)]
pub(super) struct Attachment {
    pub(super) label: SharedString,
    pub(super) payload: AttachmentPayload,
}

#[derive(Clone)]
pub(super) enum AttachmentPayload {
    Image {
        media_type: String,
        encoded: Arc<str>,
        preview: Arc<gpui::Image>,
    },
    Text(Arc<str>),
    Context(DraftContext),
    Queued {
        source: QueuedAttachment,
        preview: Option<FileImage>,
        replacement: String,
    },
    File {
        path: std::path::PathBuf,
        image: Option<FileImage>,
        file: SubmittedFile,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) enum QueuedAttachment {
    Image(String, String),
    File(jcode_sdk::api::Attachment),
}

impl QueuedAttachment {
    pub(super) fn preview(&self) -> Option<FileImage> {
        let (mime, data) = match self {
            Self::Image(mime, data) => (mime, data),
            Self::File(jcode_sdk::api::Attachment::File {
                mime,
                data: Some(data),
                ..
            }) => (mime, data),
            Self::File(_) => return None,
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        Some(FileImage {
            media_type: mime.clone(),
            encoded: data.clone().into(),
            preview: preview_image(mime, bytes)?,
        })
    }
}

#[derive(Clone)]
pub(super) struct FileImage {
    pub(super) media_type: String,
    pub(super) encoded: Arc<str>,
    pub(super) preview: Arc<gpui::Image>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedFile {
    pub(super) path: std::path::PathBuf,
    pub(super) image: Option<(String, String)>,
    pub(super) file: SubmittedFile,
}

pub(super) const FILE_MEDIA_TYPE: &str = "application/x-jcode-file-reference";

pub(super) const QUEUED_MEDIA_TYPE: &str = "application/x-jcode-queued-attachment";

pub(super) const SNIPPET_MEDIA_TYPE: &str = "application/x-jcode-pasted-text";
pub(super) const CONTEXT_MEDIA_TYPE: &str = "application/x-jcode-context";

impl Attachment {
    pub(super) fn snapshot(&self) -> AttachmentSnapshot {
        let (media_type, encoded) = match &self.payload {
            AttachmentPayload::Image {
                media_type,
                encoded,
                ..
            } => (media_type.clone(), encoded.to_string()),
            AttachmentPayload::Queued {
                source,
                replacement,
                ..
            } => (
                QUEUED_MEDIA_TYPE.to_string(),
                base64::engine::general_purpose::STANDARD.encode(
                    serde_json::to_vec(&(source, replacement)).expect("queued attachment snapshot"),
                ),
            ),
            AttachmentPayload::Text(text) => (
                SNIPPET_MEDIA_TYPE.to_string(),
                base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
            ),
            AttachmentPayload::Context(context) => (
                CONTEXT_MEDIA_TYPE.into(),
                base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(context).expect("context snapshot")),
            ),
            AttachmentPayload::File { path, image, file } => {
                let saved = SavedFile {
                    path: path.clone(),
                    image: image
                        .as_ref()
                        .map(|image| (image.media_type.clone(), image.encoded.to_string())),
                    file: file.clone(),
                };
                (
                    FILE_MEDIA_TYPE.to_string(),
                    base64::engine::general_purpose::STANDARD
                        .encode(serde_json::to_vec(&saved).expect("file attachment snapshot")),
                )
            }
        };
        AttachmentSnapshot {
            media_type,
            encoded,
            label: self.label.to_string(),
        }
    }
}

#[derive(Clone)]
pub(super) struct EditSnapshot {
    pub(super) content: String,
    pub(super) attachments: Vec<Attachment>,
}

pub(super) struct HistoryDraft {
    pub(super) content: String,
    pub(super) selection: Range<usize>,
    pub(super) selection_reversed: bool,
    pub(super) attachments: Vec<Attachment>,
}

pub(super) struct VerticalCaret {
    pub(super) offset: usize,
    pub(super) revision: u64,
    pub(super) goal_x: Pixels,
    // At a soft wrap the same offset can also be the start of the next row.
    pub(super) downstream: bool,
}

pub(super) fn preview_image(media_type: &str, bytes: Vec<u8>) -> Option<Arc<gpui::Image>> {
    Some(crate::image_cache::encoded(
        gpui::ImageFormat::from_mime_type(media_type)?,
        bytes,
    ))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PromptInputSnapshot {
    pub content: String,
    pub selection_start: usize,
    pub selection_end: usize,
    pub selection_reversed: bool,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    #[serde(default)]
    pub live_draft: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_draft: Option<HistoryDraftSnapshot>,
    pub attachments: Vec<AttachmentSnapshot>,
    #[serde(default)]
    pub stashed_prompt: Option<StashedPromptSnapshot>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HistoryDraftSnapshot {
    #[serde(flatten)]
    pub(super) draft: StashedPromptSnapshot,
    pub(super) selection_reversed: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StashedPromptSnapshot {
    pub content: String,
    pub selection_start: usize,
    pub selection_end: usize,
    pub attachments: Vec<AttachmentSnapshot>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AttachmentSnapshot {
    pub media_type: String,
    pub encoded: String,
    pub label: String,
}

pub(super) fn restore_attachments(attachments: Vec<AttachmentSnapshot>) -> Vec<Attachment> {
    attachments
        .into_iter()
        .filter_map(|attachment| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&attachment.encoded)
                .ok()?;
            let payload = if attachment.media_type == SNIPPET_MEDIA_TYPE {
                AttachmentPayload::Text(String::from_utf8(bytes).ok()?.into())
            } else if attachment.media_type == CONTEXT_MEDIA_TYPE {
                AttachmentPayload::Context(serde_json::from_slice(&bytes).ok()?)
            } else if attachment.media_type == QUEUED_MEDIA_TYPE {
                let (source, replacement): (QueuedAttachment, String) =
                    serde_json::from_slice(&bytes).ok()?;
                AttachmentPayload::Queued {
                    preview: source.preview(),
                    source,
                    replacement,
                }
            } else if attachment.media_type == FILE_MEDIA_TYPE {
                let saved: SavedFile = serde_json::from_slice(&bytes).ok()?;
                let image = match saved.image {
                    Some((media_type, encoded)) => {
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(&encoded)
                            .ok()?;
                        Some(FileImage {
                            preview: preview_image(&media_type, bytes)?,
                            media_type,
                            encoded: encoded.into(),
                        })
                    }
                    None => None,
                };
                AttachmentPayload::File {
                    path: saved.path,
                    image,
                    file: saved.file,
                }
            } else {
                let preview = preview_image(&attachment.media_type, bytes)?;
                AttachmentPayload::Image {
                    media_type: attachment.media_type,
                    encoded: attachment.encoded.into(),
                    preview,
                }
            };
            Some(Attachment {
                label: attachment.label.into(),
                payload,
            })
        })
        .collect()
}

/// Migrate pre-chip snapshots and preserve attachments when a draft is replaced.
pub(super) fn ensure_visible_chips(content: &mut String, attachments: &mut [Attachment]) {
    for attachment in attachments {
        let marker = attachment.label.as_ref();
        if content.contains(marker) {
            continue;
        }
        let marker = (1..)
            .map(|n| format!("[#image {n}]"))
            .find(|marker| !content.contains(marker))
            .unwrap();
        if !content.is_empty() && !content.ends_with(char::is_whitespace) {
            content.push(' ');
        }
        content.push_str(&marker);
        attachment.label = marker.into();
    }
}
