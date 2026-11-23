//! Prompt input with full IME support, adapted from gpui's input example.
//! Long prompts soft-wrap in a bounded, scrollable editor. Enter submits via a callback.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    StyledImage, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, img, point,
    prelude::*, px, relative, size,
};
use jcode_desktop_ui_core::ime_geometry::ImeGeometry;
use jcode_provider_core::SpeedMode;
use serde::{Deserialize, Serialize};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

#[path = "input_popup.rs"]
pub(crate) mod popup;

#[path = "input_layout.rs"]
mod layout;
#[path = "input_live_dictation.rs"]
mod live_dictation;

#[path = "input_motion.rs"]
mod motion;
use layout::PromptLayout;

#[path = "input_file_drop.rs"]
pub(crate) mod file_drop;
#[path = "input_context_picker.rs"]
mod context_picker;
#[path = "input_image_paths.rs"]
mod image_paths;
#[path = "input_model_menu.rs"]
pub(crate) mod model_menu;
#[path = "input_model_picker.rs"]
mod model_picker;
#[path = "input_model_search.rs"]
mod model_search;
#[path = "input_model_stage.rs"]
mod model_stage;
pub(crate) use model_stage::parse_model_pick;
#[path = "input_paste_preview.rs"]
mod paste_preview;
#[path = "input_resize.rs"]
mod resize;
pub(crate) use resize::VerticalResize;

use crate::commands::registered_command_entries;
use crate::theme::{Theme, to_hsla};

const PENDING_COMMAND_NOTICE: &str =
    "Commands are available once connected. Your command is still in the editor.";

actions!(
    prompt_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        DecreasePickerSpeed,
        IncreasePickerSpeed,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        MoveWordLeft,
        MoveWordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordBack,
        DeleteWordForward,
        KillToStart,
        KillToEnd,
        Undo,
        Redo,
        HistoryPrev,
        HistoryNext,
        GrowEditor,
        ShrinkEditor,
        Clear,
        Stash,
        Submit,
        Queue,
        InsertNewline,
        ShiftSpace,
        AcceptCompletion,
        AlternateContext,
    ]
);

pub fn bind_keys(cx: &mut App) {
    crate::text_selection::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("alt-enter", AlternateContext, Some("PromptInput && ContextPicker")),
        KeyBinding::new("backspace", Backspace, Some("PromptInput")),
        // GPUI matches modifiers exactly, so Shift+Backspace needs its own
        // binding or it falls through to the platform and is dropped.
        KeyBinding::new("shift-backspace", Backspace, Some("PromptInput")),
        KeyBinding::new("delete", Delete, Some("PromptInput")),
        KeyBinding::new("left", Left, Some("PromptInput")),
        KeyBinding::new("right", Right, Some("PromptInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("PromptInput")),
        KeyBinding::new("shift-right", SelectRight, Some("PromptInput")),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-right", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("alt-left", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-right", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("alt-b", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-f", MoveWordRight, Some("PromptInput")),
        KeyBinding::new("ctrl-b", MoveWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-f", MoveWordRight, Some("PromptInput")),
    ]);
    // Must follow the composer's Alt+B/Ctrl+B word motions: later bindings win
    // at equal context depth, and the handler falls through when idle.
    cx.bind_keys(crate::ui_actions::background_tool_bindings());
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("PromptInput")),
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some("PromptInput")),
        KeyBinding::new("alt-shift-right", SelectWordRight, Some("PromptInput")),
        KeyBinding::new(
            "alt-shift-left",
            DecreasePickerSpeed,
            Some("PromptInput && ModelPicker"),
        ),
        KeyBinding::new(
            "alt-shift-right",
            IncreasePickerSpeed,
            Some("PromptInput && ModelPicker"),
        ),
        KeyBinding::new("ctrl-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("super-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-a", SelectAll, Some("PromptInput")),
        KeyBinding::new("ctrl-v", Paste, Some("PromptInput")),
        KeyBinding::new("cmd-v", Paste, Some("PromptInput")),
        KeyBinding::new("super-v", Paste, Some("PromptInput")),
        KeyBinding::new("alt-v", Paste, Some("PromptInput")),
        KeyBinding::new("ctrl-c", Copy, Some("PromptInput")),
        KeyBinding::new("cmd-c", Copy, Some("PromptInput")),
        KeyBinding::new("super-c", Copy, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-c", Copy, Some("PromptInput")),
        KeyBinding::new("ctrl-x", Cut, Some("PromptInput")),
        KeyBinding::new("cmd-x", Cut, Some("PromptInput")),
        KeyBinding::new("ctrl-z", Undo, Some("PromptInput")),
        KeyBinding::new("cmd-z", Undo, Some("PromptInput")),
        KeyBinding::new("super-z", Undo, Some("PromptInput")),
        KeyBinding::new("ctrl-shift-z", Redo, Some("PromptInput")),
        KeyBinding::new("cmd-shift-z", Redo, Some("PromptInput")),
        KeyBinding::new("ctrl-y", Redo, Some("PromptInput")),
        KeyBinding::new("ctrl-w", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("alt-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("alt-delete", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("cmd-backspace", DeleteWordBack, Some("PromptInput")),
        KeyBinding::new("ctrl-delete", DeleteWordForward, Some("PromptInput")),
        KeyBinding::new("alt-d", DeleteWordForward, Some("PromptInput")),
        KeyBinding::new("ctrl-u", KillToStart, Some("PromptInput")),
        KeyBinding::new("ctrl-e", End, Some("PromptInput")),
        KeyBinding::new("ctrl-k", HistoryPrev, Some("PromptInput")),
        KeyBinding::new("ctrl-j", HistoryNext, Some("PromptInput")),
        KeyBinding::new("ctrl-[", HistoryPrev, Some("PromptInput")),
        KeyBinding::new("ctrl-]", HistoryNext, Some("PromptInput")),
        KeyBinding::new("up", HistoryPrev, Some("PromptInput")),
        KeyBinding::new("down", HistoryNext, Some("PromptInput")),
        KeyBinding::new("alt-shift-up", GrowEditor, Some("PromptInput")),
        KeyBinding::new("alt-shift-down", ShrinkEditor, Some("PromptInput")),
        KeyBinding::new("escape", Clear, Some("PromptInput")),
        KeyBinding::new("ctrl-s", Stash, Some("PromptInput")),
        // Falls through (propagates) unless ghost text is showing.
        KeyBinding::new("tab", AcceptCompletion, Some("PromptInput")),
        KeyBinding::new("home", Home, Some("PromptInput")),
        KeyBinding::new("end", End, Some("PromptInput")),
        KeyBinding::new("ctrl-home", Home, Some("PromptInput")),
        KeyBinding::new("ctrl-end", End, Some("PromptInput")),
        KeyBinding::new("enter", Submit, Some("PromptInput")),
        KeyBinding::new("shift-enter", InsertNewline, Some("PromptInput")),
        // Outranks the ChatPanel shift-space jump. With a draft it types a
        // space (Shift is often still held mid-sentence), otherwise it
        // propagates so an empty composer still jumps to latest.
        KeyBinding::new("shift-space", ShiftSpace, Some("PromptInput")),
        KeyBinding::new("ctrl-enter", Queue, Some("PromptInput")),
    ]);
    // Registered after the composer keys so the effort chords outrank word
    // motion in a chat composer. See `effort::bind_keys` for the fallthrough.
    crate::effort::bind_keys(cx);
}

pub struct PromptInput {
    session_image_base: usize,
    pub focus_handle: FocusHandle,
    #[cfg(target_os = "macos")]
    ime_log_focus_subscriptions: Option<Vec<gpui::Subscription>>,
    pub content: SharedString,
    placeholder: SharedString,
    pub(crate) external_picker_open: bool,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<PromptLayout>,
    ime_geometry: Option<ImeGeometry>,
    last_layout_revision: u64,
    vertical_caret: Option<VerticalCaret>,
    last_bounds: Option<Bounds<Pixels>>,
    chip_positions: Vec<(usize, Bounds<Pixels>)>,
    /// Row height the layout was shaped and painted with. Event handlers run
    /// outside the element's text style, where `Window::line_height` differs.
    last_line_height: Option<Pixels>,
    voice_preview: Option<String>,
    live_dictation: Option<Range<usize>>,
    /// Says what stopping the recording will do to the draft. Comes from
    /// `[dictation] mode`, so the dim preview never promises a send the mode
    /// will not perform.
    voice_preview_hint: Option<SharedString>,
    voice_preview_scroll: gpui::ScrollHandle,
    composer_bounds: paste_preview::MeasuredBounds,
    visual_line_count: usize,
    is_selecting: bool,
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    history: Vec<String>,
    history_index: Option<usize>,
    live_draft: Option<HistoryDraft>,
    attachments: Vec<Attachment>,
    stashed_prompt: Option<(String, Range<usize>, Vec<Attachment>)>,
    model_menu_draft: Option<(SharedString, Vec<Attachment>)>,
    attachment_notice: Option<SharedString>,
    /// Short-lived feedback such as `Effort: High`, cleared by a timer.
    transient_notice: Option<SharedString>,
    transient_notice_generation: u64,
    /// The newest paste briefly appears at reading size, then flies into its
    /// thumbnail. The index keeps simultaneous attachments independent.
    attachment_preview: Option<paste_preview::Preview>,
    preview_close_focus: FocusHandle,
    preview_remove_focus: FocusHandle,
    preview_panel_bounds: paste_preview::MeasuredBounds,
    file_attachments_enabled: bool,
    text_only: bool,
    queue_editor: bool,
    #[allow(clippy::type_complexity)]
    on_submit: Box<
        dyn Fn(
            SubmissionKind,
            String,
            Vec<(String, String)>,
            Vec<SubmittedFile>,
            bool,
            &mut Window,
            &mut App,
        ),
    >,
    #[allow(clippy::type_complexity)]
    on_change: Option<Box<dyn Fn(&str, &mut App)>>,
    #[allow(clippy::type_complexity)]
    on_overlay_cancel: Option<Box<dyn Fn(&mut App) -> bool>>,
    on_picker_outside: Option<popup::Dismiss>,
    command_models: Vec<String>,
    command_skills: Vec<(String, String)>,
    context_picker: Option<context_picker::Picker>,
    /// A standalone picker owns plain query text rather than a slash command.
    picker_prefix: Option<&'static str>,
    command_completion: bool,
    standalone_btw: bool,
    command_menu_closed: Option<(SharedString, CommandMenuClose)>,
    command_selection_explicit: Option<String>,
    command_selection: usize,
    model_details: HashMap<String, model_menu::ModelDetails>,
    /// Inputs of the last applied `set_model_routes`. RuntimeInfo is
    /// rebroadcast on every usage tick with an unchanged catalog, and
    /// rebuilding plus re-ranking hundreds of routes each time showed up as a
    /// steady share of main-thread time in live profiles.
    applied_model_routes: Option<(Vec<String>, Vec<jcode_sdk::ModelRouteInfo>)>,
    expanded_model_groups: HashSet<String>,
    /// Which step of the two-step model picker is showing.
    model_stage: model_stage::ModelStage,
    current_model: Option<String>,
    /// Levels the serving model accepts, for the `/effort` menu.
    effort_ladder: Vec<&'static str>,
    current_effort: Option<String>,
    speed_picker: Option<crate::speed_icon::SpeedPicker>,
    on_model_speed: Option<OnModelSpeed>,
    current_modes: Vec<String>,
    mode_catalog: crate::mode_catalog::ModeCatalog,
    /// Level picked on a model row with the effort keys, sent with the
    /// switch. Cleared when the model menu closes.
    picker_effort: Option<String>,
    command_scroll: gpui::ListState,
    suggestions_revision: u64,
    control_revision: u64,
    suggestions_cache: RefCell<Option<model_picker::SuggestionsCache>>,
    command_layout_key: Option<(SharedString, usize, gpui::Size<Pixels>)>,
    command_entrance: jcode_desktop_motion::MenuEntrance,
    editor_scroll: gpui::ScrollHandle,
    editor_wheel_gesture: std::cell::Cell<bool>,
    editor_resize: VerticalResize,
    revealed_caret: Option<(usize, SharedString, gpui::Size<Pixels>, bool)>,
    submission_enabled: bool,
    host_handles_submission: bool,
    empty_submit: bool,
    pending_session: bool,
    spacious: bool,
    compact_padding: bool,
    /// Right padding reserved for a control the host floats inside the box
    /// (the chat composer's voice button).
    trailing_inset: f32,
    /// False when a host container draws the box (onboarding email tab).
    chrome: bool,
    /// Cycle example prompts behind an empty chat composer.
    example_prompts: bool,
    /// Project this composer works in. Scopes personalized example prompts.
    working_dir: Option<String>,
    /// Shared with paint and the ticker, outside the entity, so that caret
    /// and placeholder frames do not count as changes to this input: GPUI
    /// retains views between frames and builds again a view whose entities
    /// were updated.
    motion: std::rc::Rc<std::cell::RefCell<MotionState>>,
    /// Inline autocomplete. `None` for composers that never suggest.
    ghost: Option<GhostState>,
}

/// Recent transcript turns for the completion request.
type CompletionContext = Box<dyn Fn(&App) -> Vec<crate::prompt_completion::Turn>>;

struct GhostState {
    machine: crate::prompt_completion::GhostCompletion,
    context: CompletionContext,
    /// Debounce plus request. Dropping it cancels both.
    task: Option<gpui::Task<()>>,
    /// The backend refused (unsupported or signed out). Stop asking.
    disabled: bool,
    /// Key includes draft, conversation and project, so results cannot leak across contexts.
    cache: std::collections::VecDeque<(String, String)>,
    #[cfg(test)]
    request_counter: Option<std::rc::Rc<std::cell::Cell<usize>>>,
}

/// Example prompts chosen for one seed, project and personalized catalog.
struct PickedPrompts {
    seed: usize,
    /// Identity of the personalized catalog they were picked from.
    source: Option<usize>,
    working_dir: Option<String>,
    prompts: [SharedString; 2],
}

/// Placeholder typewriter and caret motion. Never snapshotted.
struct MotionState {
    /// Last content or caret change. Motion settles relative to this.
    epoch: Instant,
    key: Option<(SharedString, usize)>,
    seed: usize,
    /// Example prompts picked for `seed`, with the inputs they came from.
    prompts: Option<PickedPrompts>,
    glide: Option<motion::Glide>,
    /// Delay until the painted motion next changes. `None` once settled.
    next: Option<Duration>,
    ticker: Option<gpui::Task<()>>,
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

fn motion_reduced(cx: &App) -> bool {
    cx.reduce_motion()
        || crate::config::get().appearance.reduce_motion
        || crate::harness::screenshot_mode()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandSuggestion {
    value: String,
    help: String,
    active_choice: bool,
    detail: Option<String>,
    header: Option<String>,
    toggle: Option<String>,
    /// What Enter or a click does inside the model picker, in place of
    /// submitting `value`.
    action: Option<model_stage::StageStep>,
    context_action: Option<context_picker::Action>,
}

#[allow(dead_code)]
fn command_suggestions(input: &str, models: &[String]) -> Vec<CommandSuggestion> {
    command_suggestions_with_efforts(input, models, EffortMenu::default())
}

/// What the `/effort` menu offers: the serving model's ladder and its level.
#[derive(Clone, Copy, Default)]
struct EffortMenu<'a> {
    ladder: Option<&'a [&'static str]>,
    current: Option<&'a str>,
}

fn command_suggestions_with_efforts(
    input: &str,
    models: &[String],
    efforts: EffortMenu<'_>,
) -> Vec<CommandSuggestion> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains('\n') {
        return Vec::new();
    }
    // Keep the command in the composer, so typing `/models` remains possible.
    // Model rows replace the command rows in the same suggestion area.
    if trimmed == "/model" || trimmed.starts_with("/model ") {
        let query = trimmed
            .split_once(' ')
            .map(|(_, query)| query.trim().to_ascii_lowercase())
            .unwrap_or_default();
        return models
            .iter()
            .filter(|model| query.is_empty() || model.to_ascii_lowercase().contains(&query))
            .map(|model| CommandSuggestion {
                value: format!("/model {model}"),
                help: "Switch this session".into(),
                active_choice: false,
                detail: None,
                header: None,
                toggle: None,
                context_action: None,
                action: None,
            })
            .collect();
    }
    if trimmed == "/effort" || trimmed.starts_with("/effort ") {
        let query = trimmed
            .split_once(' ')
            .map(|(_, query)| query.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let fallback = crate::effort::ladder(None, None);
        let ladder = efforts.ladder.unwrap_or(&fallback);
        return ladder
            .iter()
            .copied()
            .filter(|effort| query.is_empty() || effort.contains(&query))
            .map(|effort| CommandSuggestion {
                value: format!("/effort {effort}"),
                help: if effort == "swarm" {
                    "Low-effort root that fans work out to workers".into()
                } else {
                    "Set reasoning effort".into()
                },
                active_choice: efforts.current == Some(effort),
                detail: None,
                header: None,
                toggle: None,
                context_action: None,
                action: None,
            })
            .collect();
    }
    let query = trimmed.to_ascii_lowercase();
    let mut commands = registered_command_entries()
        .filter_map(|(command, help)| {
            let command_lower = command.to_ascii_lowercase();
            let help_lower = help.to_ascii_lowercase();
            (command_lower.starts_with(&query)
                || command_lower.contains(query.trim_start_matches('/'))
                || help_lower.contains(query.trim_start_matches('/')))
            .then_some((command, help, command_lower.starts_with(&query)))
        })
        .collect::<Vec<_>>();
    commands.sort_by_key(|(name, _, prefix)| (name.to_ascii_lowercase() != query, !prefix));
    commands
        .into_iter()
        .map(|(value, help, _)| CommandSuggestion {
            value: value.into(),
            help: help.into(),
            active_choice: false,
            detail: None,
            header: None,
            toggle: None,
            context_action: None,
            action: None,
        })
        .collect()
}

fn mode_suggestions(
    input: &str,
    skills: &[(String, String)],
    active: &[String],
    catalog: &crate::mode_catalog::ModeCatalog,
) -> Vec<CommandSuggestion> {
    let query = input
        .strip_prefix("/mode")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut group = None;
    let mut push = |section: &'static str, name: &str, help: &str| {
        if !name.to_ascii_lowercase().contains(&query) {
            return;
        }
        let header = (group.replace(section) != Some(section)).then(|| section.into());
        rows.push(CommandSuggestion {
            active_choice: false,
            value: format!("/mode {name}"),
            help: help.to_owned(),
            detail: None,
            header,
            toggle: None,
            context_action: None,
            action: None,
        });
    };
    for name in active {
        let help = skills
            .iter()
            .find(|(skill, _)| skill == name)
            .map_or("Pinned in this session", |(_, help)| help);
        push("Active", name, help);
    }
    for (section, modes) in [("Modes", true), ("Skills", false)] {
        for (name, help) in skills {
            if name != "off"
                && !active.contains(name)
                && crate::mode_catalog::is_mode(name, catalog.get(name)) == modes
            {
                push(section, name, help);
            }
        }
    }
    if !active.is_empty() {
        push("Actions", "off", "Clear all active modes");
    }
    rows
}

fn accepted_command_submission(
    suggestions: &[CommandSuggestion],
    selection: usize,
) -> Option<String> {
    let suggestion = suggestions.get(selection.min(suggestions.len().saturating_sub(1)))?;
    // Matching includes descriptions, not only command-name prefixes. Enter
    // must select the highlighted result for either kind of search.
    (suggestion.toggle.is_none() && suggestion.action.is_none()).then(|| suggestion.value.clone())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SubmissionKind {
    Message,
    Command,
    Skill(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandMenuClose {
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
struct Attachment {
    label: SharedString,
    payload: AttachmentPayload,
}

#[derive(Clone)]
enum AttachmentPayload {
    Image {
        media_type: String,
        encoded: Arc<str>,
        preview: Arc<gpui::Image>,
    },
    Text(Arc<str>),
    Context(context_picker::DraftContext),
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
enum QueuedAttachment {
    Image(String, String),
    File(jcode_sdk::api::Attachment),
}

impl QueuedAttachment {
    fn preview(&self) -> Option<FileImage> {
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
struct FileImage {
    media_type: String,
    encoded: Arc<str>,
    preview: Arc<gpui::Image>,
}

#[derive(Serialize, Deserialize)]
struct SavedFile {
    path: std::path::PathBuf,
    image: Option<(String, String)>,
    file: SubmittedFile,
}

const FILE_MEDIA_TYPE: &str = "application/x-jcode-file-reference";

const QUEUED_MEDIA_TYPE: &str = "application/x-jcode-queued-attachment";

const SNIPPET_MEDIA_TYPE: &str = "application/x-jcode-pasted-text";
const CONTEXT_MEDIA_TYPE: &str = "application/x-jcode-context";

impl Attachment {
    fn snapshot(&self) -> AttachmentSnapshot {
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
struct EditSnapshot {
    content: String,
    attachments: Vec<Attachment>,
}

struct HistoryDraft {
    content: String,
    selection: Range<usize>,
    selection_reversed: bool,
    attachments: Vec<Attachment>,
}

struct VerticalCaret {
    offset: usize,
    revision: u64,
    goal_x: Pixels,
    // At a soft wrap the same offset can also be the start of the next row.
    downstream: bool,
}

fn preview_image(media_type: &str, bytes: Vec<u8>) -> Option<Arc<gpui::Image>> {
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
    draft: StashedPromptSnapshot,
    selection_reversed: bool,
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

fn restore_attachments(attachments: Vec<AttachmentSnapshot>) -> Vec<Attachment> {
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
fn ensure_visible_chips(content: &mut String, attachments: &mut [Attachment]) {
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

pub(crate) fn image_marker_ranges(content: &str) -> Vec<Range<usize>> {
    content
        .match_indices("[#")
        .filter_map(|(start, _)| {
            let rest = &content[start..];
            let end = rest.find(']')?;
            let marker = &rest[2..end];
            let valid = marker.strip_prefix("image ").is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            }) || marker.strip_prefix("img_").is_some_and(|hash| {
                hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            });
            valid.then_some(start..start + end + 1)
        })
        .collect()
}

pub(crate) struct PickerSettings {
    models: Vec<String>,
    details: HashMap<String, model_menu::ModelDetails>,
    current_model: Option<String>,
    effort_ladder: Vec<&'static str>,
    current_effort: Option<String>,
    skills: Vec<(String, String)>,
    current_modes: Vec<String>,
    mode_catalog: crate::mode_catalog::ModeCatalog,
}

pub(crate) struct ModelSpeedChoice<'a> {
    pub spec: &'a str,
    pub effort: Option<&'static str>,
    pub current: bool,
    pub mode: SpeedMode,
}

type OnModelSpeed = Box<dyn Fn(ModelSpeedChoice<'_>, &mut App)>;

impl PromptInput {
    pub(crate) fn composer_proof_state(&self) -> serde_json::Value {
        let bounds = self.editor_scroll.bounds();
        serde_json::json!({
            "command_menu": !self.command_suggestions().is_empty(),
            "attachments": self.attachments.len(),
            "editor_height": f32::from(bounds.size.height),
            "editor_y": f32::from(self.editor_scroll.offset().y),
        })
    }

    pub(crate) fn composer_wheel_position(&self) -> Point<Pixels> {
        self.editor_scroll.bounds().center()
    }

    pub(crate) fn has_draft_images(&self) -> bool {
        self.attachments
            .iter()
            .chain(self.live_draft.iter().flat_map(|draft| &draft.attachments))
            .chain(
                self.stashed_prompt
                    .iter()
                    .flat_map(|(_, _, attachments)| attachments),
            )
            .any(|attachment| {
                matches!(
                    attachment.payload,
                    AttachmentPayload::Image { .. }
                        | AttachmentPayload::File { image: Some(_), .. }
                )
            })
    }

    pub(crate) fn editor_wheel_ownership(
        &self,
        event: &gpui::ScrollWheelEvent,
        in_composer: bool,
    ) -> (bool, bool) {
        let over_editor = in_composer && self.editor_scroll.bounds().contains(&event.position);
        let owned_gesture = event.delta.precise()
            && match event.touch_phase {
                gpui::TouchPhase::Started => {
                    self.editor_wheel_gesture.set(over_editor);
                    over_editor
                }
                gpui::TouchPhase::Ended | gpui::TouchPhase::Cancelled => {
                    self.editor_wheel_gesture.replace(false)
                }
                gpui::TouchPhase::Moved => self.editor_wheel_gesture.get(),
            };
        (over_editor, owned_gesture)
    }

    #[cfg(test)]
    pub(crate) fn test_editor_scroll(&self) -> gpui::ScrollHandle {
        self.editor_scroll.clone()
    }
    pub(crate) fn picker_settings(&self) -> PickerSettings {
        PickerSettings {
            models: self.command_models.clone(),
            details: self.model_details.clone(),
            current_model: self.current_model.clone(),
            effort_ladder: self.effort_ladder.clone(),
            current_effort: self.current_effort.clone(),
            skills: self.command_skills.clone(),
            current_modes: self.current_modes.clone(),
            mode_catalog: self.mode_catalog.clone(),
        }
    }

    pub(crate) fn apply_picker_settings(
        &mut self,
        settings: PickerSettings,
        cx: &mut Context<Self>,
    ) {
        self.command_models = settings.models;
        self.model_details = settings.details;
        self.current_model = settings.current_model;
        self.effort_ladder = settings.effort_ladder;
        self.current_effort = settings.current_effort;
        self.command_skills = settings.skills;
        self.current_modes = settings.current_modes;
        self.mode_catalog = settings.mode_catalog;
        self.suggestions_revision += 1;
        cx.notify();
    }

    pub fn snapshot(&self) -> PromptInputSnapshot {
        // A transient picker must never replace the user's persisted draft.
        let (content, selection, attachments) = match &self.model_menu_draft {
            Some((content, attachments)) => (
                std::borrow::Cow::Borrowed(content.as_ref()),
                content.len()..content.len(),
                attachments,
            ),
            None => {
                let (content, selection) = self.draft_without_live_dictation();
                (content, selection, &self.attachments)
            }
        };
        PromptInputSnapshot {
            content: content.to_string(),
            selection_start: selection.start,
            selection_end: selection.end,
            selection_reversed: self.selection_reversed,
            history: self.history.clone(),
            history_index: self.history_index,
            live_draft: String::new(),
            history_draft: self.live_draft.as_ref().map(|draft| HistoryDraftSnapshot {
                draft: StashedPromptSnapshot {
                    content: draft.content.clone(),
                    selection_start: draft.selection.start,
                    selection_end: draft.selection.end,
                    attachments: draft.attachments.iter().map(Attachment::snapshot).collect(),
                },
                selection_reversed: draft.selection_reversed,
            }),
            attachments: attachments.iter().map(Attachment::snapshot).collect(),
            stashed_prompt: self.stashed_prompt.as_ref().map(
                |(content, selection, attachments)| StashedPromptSnapshot {
                    content: content.clone(),
                    selection_start: selection.start,
                    selection_end: selection.end,
                    attachments: attachments.iter().map(Attachment::snapshot).collect(),
                },
            ),
        }
    }

    pub fn restore(&mut self, snapshot: PromptInputSnapshot, cx: &mut Context<Self>) {
        self.live_dictation = None;
        self.model_menu_draft = None;
        self.stashed_prompt = snapshot.stashed_prompt.map(|stash| {
            let mut content = stash.content;
            let mut attachments = restore_attachments(stash.attachments);
            ensure_visible_chips(&mut content, &mut attachments);
            let len = content.len();
            let start = stash.selection_start.min(len);
            let end = stash.selection_end.min(len);
            (content, start.min(end)..start.max(end), attachments)
        });
        let mut content = snapshot.content;
        self.attachments = restore_attachments(snapshot.attachments);
        ensure_visible_chips(&mut content, &mut self.attachments);
        let len = content.len();
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        self.selected_range = snapshot.selection_start.min(len)..snapshot.selection_end.min(len);
        if self.selected_range.end < self.selected_range.start {
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.selection_reversed = snapshot.selection_reversed;
        self.history = snapshot.history;
        self.history_index = snapshot
            .history_index
            .filter(|index| *index < self.history.len());
        self.live_draft = snapshot
            .history_draft
            .map(|snapshot| {
                let mut content = snapshot.draft.content;
                let mut attachments = restore_attachments(snapshot.draft.attachments);
                ensure_visible_chips(&mut content, &mut attachments);
                let boundary = |offset| content.floor_char_boundary(offset);
                let start = boundary(snapshot.draft.selection_start);
                let end = boundary(snapshot.draft.selection_end);
                HistoryDraft {
                    content,
                    selection: start.min(end)..start.max(end),
                    selection_reversed: snapshot.selection_reversed,
                    attachments,
                }
            })
            .or_else(|| {
                self.history_index.map(|_| {
                    let len = snapshot.live_draft.len();
                    HistoryDraft {
                        content: snapshot.live_draft,
                        selection: len..len,
                        selection_reversed: false,
                        attachments: Vec::new(),
                    }
                })
            });
        if self.history_index.is_some() {
            self.close_command_menu(CommandMenuClose::Literal);
        }
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.marked_range = None;
        self.undo.clear();
        self.redo.clear();
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.attachments.len() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let attachment = self.attachments.remove(index);
        if let Some(range) = self.chip_range(&attachment) {
            let mut content = self.content.to_string();
            content.replace_range(range.clone(), "");
            self.rebase_live_dictation_after_edit(&range, 0);
            self.control_revision = self.control_revision.wrapping_add(1);
            self.content = content.into();
            self.selected_range = range.start..range.start;
        }
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn edit_snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            content: self.draft_without_live_dictation().0.into_owned(),
            attachments: self.attachments.clone(),
        }
    }

    fn chip_range(&self, attachment: &Attachment) -> Option<Range<usize>> {
        let marker = attachment.label.as_ref();
        let start = self.content.find(marker)?;
        Some(start..start + marker.len())
    }

    fn chip_ranges(&self) -> Vec<(usize, Range<usize>)> {
        self.attachments
            .iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                self.chip_range(attachment).map(|range| (index, range))
            })
            .collect()
    }

    fn image_chip_numbers(&self) -> Vec<Option<usize>> {
        let mut ranges = self.chip_ranges();
        ranges.sort_unstable_by_key(|(_, range)| range.start);
        let mut numbers = vec![None; self.attachments.len()];
        let mut number = self.session_image_base;
        for (index, _) in ranges {
            if matches!(
                self.attachments[index].payload,
                AttachmentPayload::Image { .. }
                    | AttachmentPayload::Queued {
                        source: QueuedAttachment::Image(..),
                        ..
                    }
            ) {
                number += 1;
                numbers[index] = Some(number);
            }
        }
        numbers
    }

    #[cfg(test)]
    pub(super) fn session_image_base(&self) -> usize {
        self.session_image_base
    }

    pub(super) fn set_session_image_base(&mut self, base: usize, cx: &mut Context<Self>) {
        if self.session_image_base < base {
            self.session_image_base = base;
            cx.notify();
        }
    }

    fn expanded_chip_range(&self, mut edit: Range<usize>) -> Range<usize> {
        for (_, chip) in self.chip_ranges() {
            if (edit.start < chip.end && edit.end > chip.start)
                || (edit.is_empty() && edit.start > chip.start && edit.start < chip.end)
            {
                edit.start = edit.start.min(chip.start);
                edit.end = edit.end.max(chip.end);
            }
        }
        edit
    }

    fn remove_chips_in_range(&mut self, range: &Range<usize>) {
        let remove: Vec<usize> = self
            .chip_ranges()
            .into_iter()
            .filter_map(|(index, chip)| {
                (range.start < chip.end && range.end > chip.start).then_some(index)
            })
            .collect();
        for index in remove.into_iter().rev() {
            self.attachments.remove(index);
        }
        self.attachment_preview = None;
    }

    pub fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(String, Vec<(String, String)>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self::new_with_queue(cx, placeholder, move |text, images, _, window, cx| {
            on_submit(text, images, window, cx)
        })
    }

    pub(crate) fn new_with_queue(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(String, Vec<(String, String)>, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut input = Self::new_with_queue_files(
            cx,
            placeholder,
            move |_, text, images, _, queued, window, cx| {
                on_submit(text, images, queued, window, cx)
            },
        );
        input.file_attachments_enabled = false;
        input
    }

    pub(crate) fn new_with_queue_files(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_submit: impl Fn(
            SubmissionKind,
            String,
            Vec<(String, String)>,
            Vec<SubmittedFile>,
            bool,
            &mut Window,
            &mut App,
        ) + 'static,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            session_image_base: 0,
            #[cfg(target_os = "macos")]
            ime_log_focus_subscriptions: None,
            content: "".into(),
            placeholder: placeholder.into(),
            external_picker_open: false,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            ime_geometry: None,
            last_layout_revision: 0,
            vertical_caret: None,
            last_bounds: None,
            chip_positions: Vec::new(),
            last_line_height: None,
            voice_preview: None,
            live_dictation: None,
            voice_preview_hint: None,
            voice_preview_scroll: gpui::ScrollHandle::new(),
            composer_bounds: Default::default(),
            visual_line_count: 1,
            is_selecting: false,
            undo: Vec::new(),
            redo: Vec::new(),
            history: Vec::new(),
            history_index: None,
            live_draft: None,
            attachments: Vec::new(),
            stashed_prompt: None,
            model_menu_draft: None,
            attachment_notice: None,
            transient_notice: None,
            transient_notice_generation: 0,
            attachment_preview: None,
            preview_close_focus: cx.focus_handle(),
            preview_remove_focus: cx.focus_handle(),
            preview_panel_bounds: Default::default(),
            file_attachments_enabled: true,
            text_only: false,
            queue_editor: false,
            on_submit: Box::new(on_submit),
            on_change: None,
            on_overlay_cancel: None,
            on_picker_outside: None,
            command_models: Vec::new(),
            command_skills: Vec::new(),
            context_picker: None,
            picker_prefix: None,
            command_completion: true,
            standalone_btw: false,
            command_menu_closed: None,
            command_selection_explicit: None,
            command_selection: 0,
            model_details: HashMap::new(),
            applied_model_routes: None,
            expanded_model_groups: HashSet::new(),
            model_stage: Default::default(),
            current_model: None,
            effort_ladder: crate::effort::ladder(None, None),
            current_effort: None,
            speed_picker: None,
            on_model_speed: None,
            current_modes: Vec::new(),
            mode_catalog: Default::default(),
            picker_effort: None,
            command_scroll: gpui::ListState::new(0, gpui::ListAlignment::Top, px(48.)),
            suggestions_revision: 0,
            control_revision: 0,
            suggestions_cache: RefCell::new(None),
            command_layout_key: None,
            command_entrance: jcode_desktop_motion::MenuEntrance::default(),
            editor_scroll: gpui::ScrollHandle::new(),
            editor_wheel_gesture: Default::default(),
            editor_resize: Default::default(),
            revealed_caret: None,
            submission_enabled: true,
            host_handles_submission: false,
            empty_submit: false,
            pending_session: false,
            spacious: false,
            compact_padding: false,
            trailing_inset: 0.,
            chrome: true,
            example_prompts: false,
            working_dir: None,
            motion: Default::default(),
            ghost: None,
        }
    }

    /// An editor for one queued message, embedded in its queue row. It shows
    /// the message exactly as queued, with its images, files and pasted text
    /// as chips, and never submits by itself: the queue saves it in place.
    pub(crate) fn queued_editor(
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input =
            Self::new_with_queue_files(cx, "Edit queued message…", |_, _, _, _, _, _, _| {});
        input.queue_editor = true;
        input.command_completion = false;
        let mut text = content;
        let markers: Vec<String> = image_marker_ranges(&text)
            .into_iter()
            .map(|range| text[range].to_string())
            .collect();
        let mut chips: Vec<(String, QueuedAttachment, String)> = Vec::new();
        let mut appended: Vec<(String, QueuedAttachment)> = Vec::new();
        for (index, (mime, data)) in images.into_iter().enumerate() {
            let source = QueuedAttachment::Image(mime, data);
            match markers.get(index) {
                Some(marker) if !chips.iter().any(|(label, ..)| label == marker) => {
                    chips.push((marker.clone(), source, marker.clone()));
                }
                _ => appended.push(("[#image 1]".into(), source)),
            }
        }
        for attachment in attachments {
            let base = match &attachment {
                jcode_sdk::api::Attachment::File { name, size, .. } => {
                    format!("[#{}, {size} B]", name.replace(['[', ']', '\n', '\r'], " "))
                }
                jcode_sdk::api::Attachment::PastedText { text } => {
                    format!("[Pasted text, {} chars]", text.encode_utf16().count())
                }
                jcode_sdk::api::Attachment::Unknown => "[#attachment]".into(),
            };
            appended.push((base, QueuedAttachment::File(attachment)));
        }
        // Attachments the message text does not name get a chip at the end.
        // Saving removes that chip and the space before it again, so an
        // untouched message round-trips byte for byte.
        for (base, source) in appended {
            let label = (1..)
                .map(|n| match (n, base.strip_prefix("[#image ")) {
                    (_, Some(_)) => format!("[#image {n}]"),
                    (1, None) => base.clone(),
                    (n, None) => format!("{} #{n}]", &base[..base.len() - 1]),
                })
                .find(|label| {
                    !text.contains(label.as_str()) && !chips.iter().any(|(used, ..)| used == label)
                })
                .expect("an unused chip label");
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&label);
            chips.push((label, source, String::new()));
        }
        input.attachments = chips
            .into_iter()
            .map(|(label, source, replacement)| Attachment {
                label: label.into(),
                payload: AttachmentPayload::Queued {
                    preview: source.preview(),
                    source,
                    replacement,
                },
            })
            .collect();
        input.set_content(text, cx);
        input
    }

    /// What a queue editor saves: its text, then every image and attachment it
    /// still shows. Attachments the user did not touch go back exactly as they
    /// were queued, whether inline bytes or a path on the runtime's machine.
    pub(crate) fn queued_edit_payload(
        &self,
    ) -> (
        String,
        Vec<(String, String)>,
        Vec<jcode_sdk::api::Attachment>,
    ) {
        let mut located: Vec<_> = self
            .attachments
            .iter()
            .map(|attachment| (self.chip_range(attachment), attachment))
            .collect();
        located.sort_by_key(|(range, _)| range.as_ref().map_or(usize::MAX, |range| range.start));
        let mut images = Vec::new();
        let mut files = Vec::new();
        let mut edits = Vec::new();
        for (range, attachment) in located {
            let replacement = match &attachment.payload {
                AttachmentPayload::Queued {
                    source,
                    replacement,
                    ..
                } => {
                    match source {
                        QueuedAttachment::Image(mime, data) => {
                            images.push((mime.clone(), data.clone()))
                        }
                        QueuedAttachment::File(file) => files.push(file.clone()),
                    }
                    (replacement.as_str() != attachment.label.as_ref()).then(|| replacement.clone())
                }
                AttachmentPayload::Image {
                    media_type,
                    encoded,
                    ..
                } => {
                    images.push((media_type.clone(), encoded.to_string()));
                    None
                }
                AttachmentPayload::Text(text) => Some(text.to_string()),
                AttachmentPayload::Context(context) => Some(context.prompt_text().unwrap_or_default()),
                AttachmentPayload::File { path, file, .. } => {
                    files.push(jcode_sdk::api::Attachment::File {
                        name: file.name.clone(),
                        mime: file.mime.clone(),
                        size: file.size,
                        data: Some(file.data.clone()),
                        path: None,
                    });
                    Some(if path.as_os_str().is_empty() {
                        String::new()
                    } else {
                        file_drop::path_text(std::slice::from_ref(path))
                    })
                }
            };
            if let (Some(range), Some(replacement)) = (range, replacement) {
                edits.push((range, replacement));
            }
        }
        let mut content = self.content.to_string();
        for (range, replacement) in edits.into_iter().rev() {
            let start = if replacement.is_empty() && content[..range.start].ends_with(' ') {
                range.start - 1
            } else {
                range.start
            };
            content.replace_range(start..range.end, &replacement);
        }
        (content, images, files)
    }

    /// A preview dialog is open over this field, so Esc closes it first.
    pub(crate) fn has_open_preview(&self) -> bool {
        self.attachment_preview.is_some()
    }

    /// Test hook: laid-out line count and how far the editor can scroll.
    #[cfg(test)]
    pub(crate) fn editor_layout(&self) -> (usize, Pixels) {
        (self.visual_line_count, self.editor_scroll.max_offset().y)
    }

    pub(crate) fn with_empty_submit(mut self) -> Self {
        self.empty_submit = true;
        self
    }

    /// Suggest inline completions (ghost text, Tab accepts) when enabled in
    /// `[desktop.completions]`. `context` supplies recent transcript turns.
    pub(crate) fn with_ghost_completions(
        mut self,
        context: impl Fn(&App) -> Vec<crate::prompt_completion::Turn> + 'static,
    ) -> Self {
        if crate::config::get().completions.enabled {
            self.ghost = Some(GhostState {
                machine: Default::default(),
                context: Box::new(context),
                task: None,
                disabled: false,
                cache: Default::default(),
                #[cfg(test)]
                request_counter: None,
            });
        }
        self
    }

    fn cursor_at_end(&self) -> bool {
        self.selected_range.is_empty() && self.cursor_offset() == self.content.len()
    }

    /// Put the caret at a byte offset for dictation fixtures.
    pub(crate) fn set_caret(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.move_to(offset, cx);
    }

    /// Test hook: current caret byte offset.
    #[cfg(test)]
    pub(crate) fn caret_offset(&self) -> usize {
        self.cursor_offset()
    }

    /// Suggested remainder to paint after the caret.
    pub(crate) fn ghost_text(&self) -> Option<&str> {
        if self.marked_range.is_some() || self.content.trim_start().starts_with('/') {
            return None;
        }
        self.ghost
            .as_ref()?
            .machine
            .visible(&self.content, self.cursor_at_end())
    }

    fn dismiss_ghost(&mut self) -> bool {
        let Some(ghost) = self.ghost.as_mut() else {
            return false;
        };
        ghost.task = None;
        ghost.machine.dismiss()
    }

    /// The draft changed: drop stale work and schedule a debounced request.
    fn ghost_on_edit(&mut self, cx: &mut Context<Self>) {
        if self.standalone_context() || self.mention_token().is_some() {
            self.dismiss_ghost();
            return;
        }
        let at_end = self.cursor_at_end();
        let draft = self.content.to_string();
        let working_dir = self.working_dir.clone();
        let Some(ghost) = self.ghost.as_mut() else {
            return;
        };
        ghost.task = None;
        let Some(generation) = ghost.machine.on_edit(&draft, at_end) else {
            return;
        };
        if ghost.disabled {
            return;
        }
        // GPUI's deterministic scheduler cannot be woken by Tokio's real
        // network thread. Tests explicitly opt into a fake request counter.
        #[cfg(test)]
        if ghost.request_counter.is_none() {
            return;
        }
        let context = (ghost.context)(cx);
        let settings = &crate::config::get().completions;
        let debounce = Duration::from_millis(settings.debounce_ms.max(400));
        let model = settings.model.clone();
        #[cfg(test)]
        let request_counter = ghost.request_counter.clone();
        let project = working_dir.as_deref().and_then(|dir| {
            std::path::Path::new(dir)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        let request = crate::prompt_completion::Request {
            draft,
            context,
            cwd: working_dir,
            project,
            model,
        };
        let cache_key = serde_json::to_string(&request).unwrap_or_default();
        if let Some((_, completion)) = ghost.cache.iter().find(|(key, _)| key == &cache_key) {
            if ghost.machine.on_response(generation, completion) {
                cx.notify();
            }
            return;
        }
        ghost.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(debounce).await;
            #[cfg(test)]
            if let Some(counter) = request_counter {
                counter.set(counter.get() + 1);
                return;
            }
            let Some(key) = jcode_base::subscription_catalog::configured_api_key()
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
            else {
                return;
            };
            let base = jcode_base::subscription_api::configured_api_base();
            let reserved = cx
                .background_executor()
                .spawn(async { crate::prompt_completion::reserve_daily_request() })
                .await;
            if !reserved {
                return;
            }
            let result = match crate::prompt_completion::fetch(base, key, request) {
                Ok(mut task) => task.result().await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |input, cx| {
                let Some(ghost) = input.ghost.as_mut() else {
                    return;
                };
                match result {
                    Ok(completion) => {
                        if ghost.machine.is_current(generation) {
                            if ghost.cache.len() >= 32 {
                                ghost.cache.pop_front();
                            }
                            ghost.cache.push_back((cache_key, completion.clone()));
                        }
                        if ghost.machine.on_response(generation, &completion) {
                            cx.notify();
                        }
                    }
                    Err(crate::prompt_completion::FetchError::Unsupported) => {
                        ghost.disabled = true;
                    }
                    Err(crate::prompt_completion::FetchError::Other(error)) => {
                        eprintln!("jcode desktop: prompt completion failed: {error}");
                    }
                }
            });
        }));
    }

    fn accept_completion(
        &mut self,
        _: &AcceptCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.attachments.is_empty()
            && let Some(name) = self.selected_mode_name()
        {
            self.toggle_mode_row(&name, window, cx);
            return;
        }
        if self.accept_context_row(false, window, cx) {
            return;
        }
        if self.complete_slash_command(cx) {
            return;
        }
        let at_end = self.cursor_at_end() && self.marked_range.is_none();
        let accepted = self
            .ghost
            .as_mut()
            .and_then(|ghost| ghost.machine.accept(&self.content, at_end));
        match accepted {
            Some(suffix) => {
                self.replace_text_in_range(None, &suffix, window, cx);
                self.dismiss_ghost();
            }
            None => cx.propagate(),
        }
    }

    /// Tab on a visible slash popup accepts its highlighted row.
    fn complete_slash_command(&mut self, cx: &mut Context<Self>) -> bool {
        if self.picker_prefix.is_some() {
            return false;
        }
        if !self.command_completion || !self.attachments.is_empty() {
            return false;
        }
        let suggestions = self.command_suggestions();
        let Some(row) = suggestions.get(self.command_selection) else {
            return false;
        };
        if row.toggle.is_some()
            || (row.action.is_some()
                && !(self.model_stage == model_stage::ModelStage::Models
                    && row.value.starts_with("/model ")))
        {
            return false;
        }
        let value = row.value.clone();
        let embedded = self.embedded_command_token();
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        if let Some((range, _)) = embedded {
            let caret = range.start + value.len();
            let mut draft = self.content.to_string();
            draft.replace_range(range, &value);
            self.set_content(draft, cx);
            self.selected_range = caret..caret;
            self.close_command_menu(CommandMenuClose::Literal);
        } else {
            self.set_content(value, cx);
            self.close_command_menu(CommandMenuClose::Completed);
        }
        cx.notify();
        true
    }

    /// Chat composers show animated example prompts while empty.
    pub(crate) fn with_example_prompts(mut self) -> Self {
        self.example_prompts = true;
        self
    }

    /// Placeholder shown at `now`, the example it belongs to, and whether it
    /// is still animating.
    fn placeholder_text(
        &self,
        now: Instant,
        cx: &App,
    ) -> (SharedString, Option<SharedString>, bool) {
        if !self.example_prompts {
            return (self.placeholder.clone(), None, false);
        }
        if self.external_picker_open {
            return (SharedString::default(), None, false);
        }
        let reduced = motion_reduced(cx);
        let mut state = self.motion.borrow_mut();
        let source = cx
            .try_global::<crate::example_prompts::ExamplePrompts>()
            .map(|global| Arc::as_ptr(&global.0) as usize);
        let fresh = state.prompts.as_ref().is_some_and(|picked| {
            picked.seed == state.seed
                && picked.source == source
                && picked.working_dir == self.working_dir
        });
        if !fresh {
            // A shared project is not a session boundary: another session's
            // unfinished todos must never become this composer's examples.
            let prompts =
                motion::pick_prompts(&[] as &[SharedString], state.seed).map(SharedString::from);
            if state
                .prompts
                .as_ref()
                .is_some_and(|picked| picked.prompts != prompts)
            {
                // Sessions loaded after the composer appeared. Start the new
                // example from its first letter rather than mid-word.
                state.epoch = now;
            }
            state.prompts = Some(PickedPrompts {
                seed: state.seed,
                source,
                working_dir: self.working_dir.clone(),
                prompts,
            });
        }
        let picked = state
            .prompts
            .as_ref()
            .expect("example prompts were just picked");
        let elapsed = now.saturating_duration_since(state.epoch);
        let prompts = [picked.prompts[0].as_ref(), picked.prompts[1].as_ref()];
        let (shown, full, live) = motion::placeholder_at(elapsed, prompts, reduced);
        let full = picked
            .prompts
            .iter()
            .find(|prompt| prompt.as_ref() == full)
            .cloned();
        let shown = if full.as_ref().is_some_and(|full| full.len() == shown.len()) {
            full.clone().unwrap_or_default()
        } else {
            SharedString::from(shown.to_owned())
        };
        (shown, full, live)
    }

    /// Scope personalized example prompts to this project.
    pub(crate) fn set_working_dir(&mut self, working_dir: Option<&str>) {
        if self.working_dir.as_deref() != working_dir {
            self.working_dir = working_dir.map(str::to_owned);
        }
    }

    pub(crate) fn has_user_draft(&self) -> bool {
        !self.content.trim().is_empty()
            || !self.attachments.is_empty()
            || self
                .stashed_prompt
                .as_ref()
                .is_some_and(|(text, _, attachments)| {
                    !text.trim().is_empty() || !attachments.is_empty()
                })
    }

    pub(crate) fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref()
    }

    /// Whether `set_spacious` or `set_trailing_inset` with these values would
    /// change anything. Checked before updating while a frame is drawn.
    pub(crate) fn needs_layout_update(&self, spacious: bool, trailing_inset: f32) -> bool {
        self.spacious != spacious || self.trailing_inset != trailing_inset
    }

    pub(crate) fn set_spacious(&mut self, spacious: bool, cx: &mut Context<Self>) {
        if self.spacious != spacious {
            self.spacious = spacious;
            cx.notify();
        }
    }

    /// Reserve room at the right edge for a host-drawn control.
    pub(crate) fn set_trailing_inset(&mut self, inset: f32, cx: &mut Context<Self>) {
        if self.trailing_inset != inset {
            self.trailing_inset = inset;
            cx.notify();
        }
    }

    /// The host draws the fill, border and corners around this field.
    pub(crate) fn without_chrome(mut self) -> Self {
        self.chrome = false;
        self
    }

    pub(crate) fn with_compact_padding(mut self) -> Self {
        self.compact_padding = true;
        self
    }

    pub(crate) fn embedded_editor(cx: &mut Context<Self>, placeholder: &'static str) -> Self {
        let mut editor = Self::new(cx, placeholder, |_, _, _, _| {})
            .without_command_completion()
            .without_chrome()
            .with_compact_padding();
        editor.host_handles_submission = true;
        editor
    }

    /// Plain metadata editors should not interpret titles as slash commands.
    pub(crate) fn without_command_completion(mut self) -> Self {
        self.command_completion = false;
        self
    }

    pub(crate) fn with_standalone_btw(mut self) -> Self {
        self.standalone_btw = true;
        self
    }

    pub(crate) fn text_only(mut self) -> Self {
        self.text_only = true;
        self
    }

    pub fn with_on_change(mut self, on_change: impl Fn(&str, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    /// Explicitly disable submission without disabling editing or focus.
    /// Startup panels instead queue prompts and separately gate slash commands.
    pub fn set_submission_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.submission_enabled = enabled;
        cx.notify();
    }

    pub(crate) fn set_pending_session(&mut self, pending: bool, cx: &mut Context<Self>) {
        self.pending_session = pending;
        if !pending && self.attachment_notice.as_deref() == Some(PENDING_COMMAND_NOTICE) {
            self.attachment_notice = None;
        }
        cx.notify();
    }

    pub fn with_on_overlay_cancel(mut self, cancel: impl Fn(&mut App) -> bool + 'static) -> Self {
        self.on_overlay_cancel = Some(Box::new(cancel));
        self
    }

    pub(crate) fn with_on_model_speed(
        mut self,
        change: impl Fn(ModelSpeedChoice<'_>, &mut App) + 'static,
    ) -> Self {
        self.on_model_speed = Some(Box::new(change));
        self
    }

    pub(crate) fn with_on_picker_outside(
        mut self,
        dismiss: impl Fn(&FocusHandle, &mut Window, &mut App) + 'static,
    ) -> Self {
        let focus = self.focus_handle.clone();
        self.on_picker_outside = Some(std::rc::Rc::new(move |window, cx| {
            dismiss(&focus, window, cx);
        }));
        self
    }

    pub fn set_command_models(&mut self, models: Vec<String>, cx: &mut Context<Self>) {
        if self.command_models != models {
            self.command_models = models;
            self.applied_model_routes = None;
            self.suggestions_revision += 1;
            self.command_selection = 0;
            cx.notify();
        }
    }

    pub fn set_model_routes(
        &mut self,
        mut models: Vec<String>,
        routes: &[jcode_sdk::ModelRouteInfo],
        current_model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self
            .applied_model_routes
            .as_ref()
            .is_some_and(|(applied_models, applied_routes)| {
                *applied_models == models && applied_routes.as_slice() == routes
            })
        {
            self.set_current_model(current_model, cx);
            return;
        }
        let applied = (models.clone(), routes.to_vec());
        let selected = self
            .command_suggestions()
            .get(self.command_selection)
            .cloned();
        self.model_details = model_menu::from_routes(routes);
        self.suggestions_revision += 1;
        self.current_model = current_model;
        if !routes.is_empty() {
            models = self.model_details.keys().cloned().collect();
        }
        model_menu::rank(&mut models, &self.model_details);
        self.set_command_models(models, cx);
        self.applied_model_routes = Some(applied);
        if let Some(model) = selected
            .as_ref()
            .and_then(|row| row.value.strip_prefix("/model "))
            && !self
                .command_suggestions()
                .iter()
                .any(|row| row.value == format!("/model {model}"))
            && self
                .command_models
                .iter()
                .any(|candidate| candidate == model)
        {
            self.expanded_model_groups
                .insert(model_menu::group_key(model, &self.model_details));
            self.suggestions_revision += 1;
        }
        // A background usage update must not move Enter onto a different model.
        if let Some(index) = selected.and_then(|selected| {
            self.command_suggestions().iter().position(|row| {
                if selected.toggle.is_some() {
                    row.toggle == selected.toggle
                } else {
                    row.toggle.is_none() && row.value == selected.value
                }
            })
        }) {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    pub fn set_current_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        if self.current_model != model {
            self.current_model = model;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    pub(crate) fn set_serving_model(
        &mut self,
        model: Option<&str>,
        provider: Option<&str>,
        auth: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let spec = model.and_then(|model| {
            let mut matches = self.model_details.iter().filter(|(_, route)| {
                route.model == model
                    && provider.is_none_or(|provider| route.provider == provider)
                    && auth.is_none_or(|auth| {
                        crate::panel::human_auth_method(&route.api_method) == auth
                    })
            });
            let (spec, _) = matches.next()?;
            matches.next().is_none().then(|| spec.clone())
        });
        self.set_current_model(spec.or_else(|| model.map(str::to_owned)), cx);
    }

    #[cfg(test)]
    pub fn model_picker_rows(&self) -> Vec<(String, bool)> {
        self.command_suggestions()
            .iter()
            .enumerate()
            .filter_map(|(index, suggestion)| {
                suggestion.value.strip_prefix("/model ").map(|model| {
                    (
                        self.model_details
                            .get(model)
                            .map_or_else(|| model.to_string(), |detail| detail.model.clone()),
                        index == self.command_selection,
                    )
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn command_models(&self) -> &[String] {
        &self.command_models
    }

    /// An available catalog spec for a signed-in provider, preferring the current model.
    pub(crate) fn provider_route_spec(
        &self,
        provider: &str,
        current: Option<&str>,
    ) -> Option<String> {
        provider_route_spec_in(&self.model_details, provider, current)
            .or_else(|| provider_fallback_spec(provider))
    }

    /// A picker accepts either its search argument or the full slash command.
    /// Both the rows and Enter must interpret that text identically.
    fn command_content(&self) -> String {
        let Some(prefix) = self.picker_prefix else {
            return self.content.to_string();
        };
        let content = self.content.trim_start();
        let strip = |command: &str| {
            content
                .strip_prefix(command)
                .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        };
        let query = strip(prefix)
            .or_else(|| {
                (prefix == "/model"
                    && matches!(self.model_stage, model_stage::ModelStage::Efforts { .. }))
                .then(|| strip("/effort"))
                .flatten()
            })
            .unwrap_or(content);
        format!("{prefix} {}", query.trim_start())
    }

    fn build_command_suggestions(&self) -> Vec<CommandSuggestion> {
        if !self.command_completion {
            return Vec::new();
        }
        if let Some(rows) = self.context_rows() {
            return rows;
        }
        let command_content = self.embedded_command_token().map_or_else(
            || self.command_content(),
            |(range, cursor)| self.content[range.start..cursor].to_owned(),
        );
        if !command_content.starts_with('/') || command_content.contains(['\n', '\r']) {
            return Vec::new();
        }
        let trimmed = command_content.trim_end();
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        if first == "/speed" {
            let query = trimmed.strip_prefix("/speed").unwrap_or_default().trim().to_ascii_lowercase();
            return self.speed_picker.iter().flat_map(|picker| {
                picker.options.iter().filter_map(|(tier, detail)| {
                    let value = crate::speed_icon::command_value(*tier);
                    value.trim_start_matches("/speed ").contains(&query).then(|| CommandSuggestion {
                        value,
                        help: String::new(),
                        active_choice: *tier == picker.active,
                        detail: Some(detail.clone()),
                        header: None,
                        toggle: None,
                        context_action: None,
                        action: None,
                    })
                })
            }).collect();
        }
        let now = model_menu::now_unix_secs();
        if first == "/mode" {
            let modes = mode_suggestions(
                trimmed,
                &self.command_skills,
                &self.current_modes,
                &self.mode_catalog,
            );
            if !modes.is_empty() || self.picker_prefix.is_some() {
                return modes;
            }
        }
        let mut suggestions = if first == "/model" {
            let query = trimmed.strip_prefix("/model").unwrap_or_default();
            self.model_stage_rows(query)
        } else {
            command_suggestions_with_efforts(
                &command_content,
                &self.command_models,
                EffortMenu {
                    ladder: Some(&self.effort_ladder),
                    current: self.current_effort.as_deref(),
                },
            )
        };
        if self.picker_prefix.is_none()
            && first.len() > 1
            && first != "/model"
            && (first != "/effort" || suggestions.is_empty())
            && (first != trimmed || suggestions.is_empty())
            && let Some(help) = crate::commands::registered_command(first)
                .map(|command| command.help.to_owned())
                .or_else(|| {
                    self.command_skills.iter().find_map(|(name, help)| {
                        (first.strip_prefix('/') == Some(name)).then(|| help.clone())
                    })
                })
        {
            suggestions.insert(
                0,
                CommandSuggestion {
                    value: trimmed.to_owned(),
                    help,
                    active_choice: false,
                    detail: None,
                    header: None,
                    toggle: None,
                    context_action: None,
                    action: None,
                },
            );
        }
        if self.picker_prefix.is_none()
            && trimmed.starts_with('/')
            && !trimmed.contains(char::is_whitespace)
        {
            let query = trimmed.to_ascii_lowercase();
            for (name, description) in &self.command_skills {
                let value = format!("/{name}");
                if value.to_ascii_lowercase().starts_with(&query)
                    && !suggestions.iter().any(|row| row.value == value)
                {
                    suggestions.push(CommandSuggestion {
                        value,
                        help: description.clone(),
                        active_choice: false,
                        detail: Some("Skill".into()),
                        header: None,
                        toggle: None,
                        context_action: None,
                        action: None,
                    });
                }
            }
            if trimmed != "/model" && trimmed != "/effort" {
                suggestions.sort_by_key(|row| {
                    let value = row.value.to_ascii_lowercase();
                    (value != query, !value.starts_with(&query))
                });
            }
        }
        suggestions
            .into_iter()
            .map(|mut suggestion| {
                if let Some(model) = suggestion.value.strip_prefix("/model ") {
                    suggestion.detail = Some(
                        self.model_details
                            .get(model)
                            .map(|details| details.label(now))
                            .unwrap_or_else(|| model_menu::usage_label(None, now)),
                    );
                }
                suggestion
            })
            .collect()
    }

    fn embedded_command_token(&self) -> Option<(Range<usize>, usize)> {
        if self.picker_prefix.is_some() || self.content.starts_with('/') {
            return None;
        }
        let (start, cursor) =
            jcode_slash_command::active_token_before_cursor(&self.content, self.cursor_offset())?;
        if self.content[..start].trim().is_empty() {
            return None;
        }
        let end = self.content[cursor..]
            .find(char::is_whitespace)
            .map_or(self.content.len(), |offset| cursor + offset);
        Some((start..end, cursor))
    }

    fn toggle_model_group(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded_model_groups.remove(&key) {
            self.expanded_model_groups.insert(key.clone());
        }
        self.suggestions_revision += 1;
        // Keep focus on the disclosure when rows appear or disappear above it.
        if let Some(index) = self
            .command_suggestions()
            .iter()
            .position(|row| row.toggle.as_ref() == Some(&key))
        {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.accept_context_row(false, window, cx) {
            return;
        }
        if self.picker_prefix == Some("/mode") {
            if let Some(close) = &self.on_overlay_cancel {
                close(cx);
            }
            return;
        }
        self.submit_prompt(false, window, cx);
    }

    fn stash(&mut self, _: &Stash, _: &mut Window, cx: &mut Context<Self>) {
        let (draft, selection) = self.draft_without_live_dictation();
        let draft = draft.into_owned();
        let current = (!draft.is_empty() || !self.attachments.is_empty()).then(|| {
            (draft, selection, std::mem::take(&mut self.attachments))
        });
        let previous = std::mem::replace(&mut self.stashed_prompt, current);
        let (text, selection, attachments) = previous.unwrap_or_default();
        self.set_content(text, cx);
        self.selected_range = selection;
        self.attachments = attachments;
        self.marked_range = None;
        self.attachment_notice = None;
        self.attachment_preview = None;
        self.undo.clear();
        self.redo.clear();
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn shift_space(&mut self, _: &ShiftSpace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            cx.propagate();
        } else {
            self.replace_text_in_range(None, " ", window, cx);
        }
    }

    fn queue(&mut self, _: &Queue, window: &mut Window, cx: &mut Context<Self>) {
        if self.standalone_context() {
            return;
        }
        self.submit_prompt(true, window, cx);
    }

    pub(crate) fn control_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.live_dictation.is_some()
            || !self.submission_enabled
            || self.queue_editor
            || self.content.trim().is_empty()
        {
            return false;
        }
        self.dispatch_submission(
            SubmissionKind::Message,
            Self::literal_submission(&self.content),
            false,
            window,
            cx,
        );
        self.content.is_empty()
    }

    fn submit_prompt(&mut self, queued: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.live_dictation.is_some() {
            self.set_transient_notice("Stop voice recording before sending".into(), cx);
            return;
        }
        if !self.submission_enabled || self.queue_editor {
            return;
        }
        let raw_content = self.command_content();
        let content = raw_content.trim().to_string();
        if self.attachments.is_empty() && content.split_whitespace().next() == Some("/speed") {
            if crate::speed_icon::command_tier(&content).is_some() {
                self.submit_command(content, queued, window, cx);
            } else if let Some(command) = self
                .command_suggestions()
                .get(self.command_selection)
                .map(|row| row.value.clone())
            {
                self.submit_command(command, queued, window, cx);
            } else {
                self.set_transient_notice(
                    "Use /speed standard, /speed fast, or /speed ultrafast".into(),
                    cx,
                );
            }
            return;
        }
        if self.standalone_btw
            && !queued
            && self.picker_prefix.is_none()
            && self.attachments.is_empty()
            && self.content.trim() == "btw"
        {
            self.dispatch_submission(SubmissionKind::Command, content, false, window, cx);
            return;
        }
        if content.is_empty() && self.attachments.is_empty() {
            if self.empty_submit && !queued {
                (self.on_submit)(
                    SubmissionKind::Message,
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    false,
                    window,
                    cx,
                );
            }
            return;
        }
        if self.command_completion && self.attachments.is_empty() {
            if self.current_command_menu_close() == Some(CommandMenuClose::Completed) {
                self.submit_command(content, queued, window, cx);
                return;
            }
            let suggestions = self.command_suggestions();
            let embedded = self.embedded_command_token();
            let picker_command = raw_content.starts_with('/')
                && !raw_content.contains(['\n', '\r'])
                && matches!(
                    content.split_whitespace().next(),
                    Some("/model" | "/mode" | "/effort")
                );
            if let Some(row) = suggestions.get(self.command_selection)
                && (self.picker_prefix.is_some()
                    || ((!queued || embedded.is_none())
                        && self.command_selection_explicit.as_ref() == Some(&row.value))
                    || row.value == content
                    || picker_command)
            {
                if let Some(key) = row.toggle.clone() {
                    self.toggle_model_group(key, cx);
                } else if let Some(step) = row.action.clone() {
                    self.run_stage_step(step, queued, window, cx);
                } else if let Some(command) =
                    accepted_command_submission(&suggestions, self.command_selection)
                {
                    self.submit_command(command, queued, window, cx);
                }
                return;
            }
            if picker_command
                && content.split_whitespace().next() == Some("/model")
                && self.current_command_menu_close().is_none()
            {
                self.set_transient_notice("No matching models".into(), cx);
                return;
            }
        }
        if self.picker_prefix.is_none() {
            self.dispatch_submission(
                SubmissionKind::Message,
                Self::literal_submission(&raw_content),
                queued,
                window,
                cx,
            );
        }
    }

    fn literal_submission(text: &str) -> String {
        if text.contains('/') {
            text.to_owned()
        } else {
            text.trim().to_owned()
        }
    }

    fn submit_command(
        &mut self,
        command: String,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.submission_enabled || self.queue_editor {
            return;
        }
        if let Some(crate::panel::workspace_commands::parse::WorkspaceCommand::Invalid(usage)) =
            crate::panel::workspace_commands::parse::parse(&command)
        {
            self.set_transient_notice(usage.into(), cx);
            return;
        }
        if self.pending_session
            && !(crate::commands::is_resume_command(&command)
                || crate::commands::is_accounts_command(&command)
                || crate::commands::reset_command(&command).is_some()
                || command == "/btw"
                || crate::panel::is_draft_config_command(&command))
        {
            self.attachment_notice = Some(PENDING_COMMAND_NOTICE.into());
            cx.notify();
            return;
        }
        self.dispatch_submission(SubmissionKind::Command, command, queued, window, cx);
    }

    /// Run what Enter or a click on a picker row means.
    fn run_stage_step(
        &mut self,
        step: model_stage::StageStep,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match step {
            model_stage::StageStep::Goto(stage) => self.go_to_model_stage(stage, cx),
            model_stage::StageStep::ToggleHidden(spec) => self.toggle_model_hidden(&spec, cx),
            model_stage::StageStep::Submit(command) => {
                self.submit_command(command, queued, window, cx)
            }
        }
    }

    /// Clear the composer and hand `content` to the host.
    fn dispatch_submission(
        &mut self,
        mut kind: SubmissionKind,
        content: String,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = if content.is_empty() {
            "[image]".to_string()
        } else {
            content
        };
        let remaining_draft = (kind == SubmissionKind::Command)
            .then(|| self.embedded_command_token())
            .flatten()
            .map(|(token, _)| {
                let mut draft = self.content.to_string();
                let caret = token.start;
                draft.replace_range(token, "");
                (draft, caret)
            });
        let mut attachments = if remaining_draft.is_none() {
            std::mem::take(&mut self.attachments)
        } else {
            Vec::new()
        };
        attachments.sort_by_key(|a| content.find(a.label.as_ref()).unwrap_or(usize::MAX));
        let mut images = Vec::new();
        let mut files = Vec::new();
        let mut replacements = Vec::new();
        let mut history_content = content.clone();
        for attachment in attachments {
            let range = content
                .find(attachment.label.as_ref())
                .map(|start| start..start + attachment.label.len());
            match attachment.payload {
                AttachmentPayload::Image {
                    media_type,
                    encoded,
                    ..
                } => {
                    images.push((media_type, encoded.to_string()));
                    if let Some(range) = range {
                        // Draft markers are editor keys. The wire marker indexes
                        // the reordered payload, not the draft's insertion order.
                        let output = format!("[#image {}]", images.len());
                        replacements.push((range, Some(output), String::new()));
                    }
                }
                // Only queue editors hold these, and they never submit.
                AttachmentPayload::Queued { .. } => {}
                AttachmentPayload::Text(text) => {
                    if let Some(range) = range {
                        replacements.push((range, Some(text.to_string()), text.to_string()));
                    }
                }
                AttachmentPayload::Context(context) => {
                    if let Some(range) = range {
                        let history = match &context {
                            context_picker::DraftContext::File { path, .. } => file_drop::path_text(std::slice::from_ref(path)),
                            context_picker::DraftContext::Skill(name) => {
                                kind = SubmissionKind::Skill(name.clone());
                                String::new()
                            }
                        };
                        replacements.push((range, Some(context.prompt_text().unwrap_or_default()), history));
                    }
                }
                AttachmentPayload::File { path, image, file } => {
                    let (reference, history) = if let Some(image) = image {
                        images.push((image.media_type, image.encoded.to_string()));
                        (format!("[#image {}]", images.len()), String::new())
                    } else {
                        if self.file_attachments_enabled {
                            files.push(file);
                        }
                        let path = file_drop::path_text(&[path]);
                        (path.clone(), path)
                    };
                    if let Some(range) = range {
                        replacements.push((range, Some(reference), history));
                    }
                }
            }
        }
        let mut content = content;
        for (range, output, history) in replacements.into_iter().rev() {
            if let Some(output) = output {
                content.replace_range(range.clone(), &output);
            }
            history_content.replace_range(range, &history);
        }
        self.attachment_notice = None;
        self.transient_notice = None;
        self.attachment_preview = None;
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = "".into();
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.expanded_model_groups.clear();
        self.model_stage = Default::default();
        self.picker_effort = None;
        self.suggestions_revision += 1;
        // The cleared editor is one line immediately. Waiting for its next
        // paint leaves a tall, empty composer after a wrapped prompt submits.
        self.visual_line_count = 1;
        self.selected_range = 0..0;
        self.marked_range = None;
        if !history_content.trim().is_empty() {
            self.history.push(history_content);
        }
        if remaining_draft.is_none() {
            self.history_index = None;
        }
        self.command_selection = 0;
        (self.on_submit)(kind, content, images, files, queued, window, cx);
        if let Some((text, caret)) = remaining_draft {
            self.set_content(text, cx);
            self.selected_range = caret..caret;
        } else if let Some(draft) = self.live_draft.take() {
            self.restore_history_draft(draft, cx);
        } else if let Some((text, selection, attachments)) = self.stashed_prompt.take() {
            self.set_content(text, cx);
            self.selected_range = selection;
            self.attachments = attachments;
        }
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        // On a model row, ←/→ pick the level the switch will use.
        if self.step_picker_effort(-1, cx) {
            return;
        }
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.step_picker_effort(1, cx) {
            return;
        }
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    pub(crate) fn attach_image(
        &mut self,
        image: crate::clipboard_image::ClipboardImage,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = preview_image(&image.media_type, image.bytes.clone()) else {
            self.attachment_notice = Some("could not preview pasted image".into());
            cx.notify();
            return;
        };
        let mut number = 1;
        let marker = loop {
            let marker = format!("[#image {number}]");
            if !self.content.contains(&marker)
                && !self.attachments.iter().any(|a| a.label.as_ref() == marker)
            {
                break marker;
            }
            number += 1;
        };
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let range = self.expanded_chip_range(self.clamp_byte_range(self.selected_range.clone()));
        self.remove_chips_in_range(&range);
        let mut content = self.content.to_string();
        content.replace_range(range.clone(), &marker);
        self.rebase_live_dictation_after_edit(&range, marker.len());
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        let cursor = range.start + marker.len();
        self.selected_range = cursor..cursor;
        self.attachments.push(Attachment {
            label: marker.into(),
            payload: AttachmentPayload::Image {
                media_type: image.media_type,
                encoded: base64::engine::general_purpose::STANDARD
                    .encode(image.bytes)
                    .into(),
                preview,
            },
        });
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.marked_range = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_only {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.replace_text_in_range(None, &text, window, cx);
            }
            return;
        }
        match crate::clipboard_image::read_paste(cx) {
            Ok(crate::clipboard_image::ClipboardContent::Files(paths)) => {
                self.attach_pasted_files(&paths, window, cx);
                return;
            }
            Ok(crate::clipboard_image::ClipboardContent::Image(image)) => {
                self.attach_image(image, cx);
                return;
            }
            Ok(crate::clipboard_image::ClipboardContent::Empty) => {}
            Err(error) => {
                self.attachment_notice = Some(format!("image paste unavailable: {error}").into());
            }
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste_text_or_image_paths(&text, window, cx);
        }
    }

    fn paste_text_or_image_paths(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(paths) = image_paths::pasted_image_paths(text) {
            self.drop_files(&paths, window, cx);
        } else {
            self.insert_pasted_text(text, window, cx);
        }
    }

    fn insert_pasted_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if is_large_paste(text) {
            self.attach_pasted_text(text, cx);
        } else {
            self.replace_text_in_range(None, text, window, cx);
        }
        if self.picker_prefix.is_none() && self.content.starts_with('/') {
            self.close_command_menu(CommandMenuClose::Literal);
        }
        self.dismiss_ghost();
    }

    /// Files copied in the file manager. Only images are read, so the model
    /// can see them. Any other file, a folder or an oversized image is named
    /// by path: pasting a file points at it, it does not upload it.
    fn attach_pasted_files(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (paths, referenced): (Vec<_>, Vec<_>) = paths.iter().cloned().partition(|path| {
            file_drop::image_media_type(path).is_some()
                && std::fs::metadata(path)
                    .is_ok_and(|meta| meta.is_file() && meta.len() <= 10 * 1024 * 1024)
        });
        self.reference_paths(&referenced, paths.is_empty(), window, cx);
        let paths = &paths;
        let mut attached_size: u64 = self
            .attachments
            .iter()
            .filter_map(|attachment| match &attachment.payload {
                AttachmentPayload::File { file, .. } => Some(file.size),
                AttachmentPayload::Queued {
                    source: QueuedAttachment::File(jcode_sdk::api::Attachment::File { size, .. }),
                    ..
                } => Some(*size),
                _ => None,
            })
            .sum();
        for path in paths {
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() && metadata.len() <= 10 * 1024 * 1024 => {
                    metadata
                }
                Ok(_) => {
                    self.attachment_notice =
                        Some("File must be at most 10 MiB and not a directory".into());
                    continue;
                }
                Err(error) => {
                    self.attachment_notice = Some(format!("Cannot read file: {error}").into());
                    continue;
                }
            };
            if attached_size.saturating_add(metadata.len()) > 20 * 1024 * 1024 {
                self.attachment_notice = Some("File attachments exceed 20 MiB total".into());
                continue;
            }
            let bytes = match std::fs::File::open(path).and_then(|file| {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(
                    &mut std::io::Read::take(file, 10 * 1024 * 1024 + 1),
                    &mut bytes,
                )?;
                Ok(bytes)
            }) {
                Ok(bytes) if bytes.len() as u64 == metadata.len() => bytes,
                Ok(_) => {
                    self.attachment_notice = Some("File changed while attaching".into());
                    continue;
                }
                Err(error) => {
                    self.attachment_notice = Some(format!("Cannot read file: {error}").into());
                    continue;
                }
            };
            attached_size += metadata.len();
            let size = Some(metadata.len());
            let file = SubmittedFile {
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                mime: mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string(),
                size: metadata.len(),
                data: base64::engine::general_purpose::STANDARD.encode(&bytes),
            };
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .replace(['[', ']', '\n', '\r'], " ");
            let size_label =
                size.map_or_else(|| "unknown size".to_string(), |size| format!("{size} B"));
            let description = format!("{name}, {size_label}");
            let mut marker = format!("[#{description}]");
            let mut n = 2;
            while self.content.contains(&marker) {
                marker = format!("[#{description} #{n}]");
                n += 1;
            }
            let image = file_drop::image_media_type(path).and_then(|media_type| {
                if !size.is_some_and(|size| size <= 20 * 1024 * 1024) {
                    return None;
                }
                let image =
                    crate::clipboard_image::from_bytes(media_type.to_owned(), bytes.clone())
                        .ok()?;
                let preview = preview_image(&image.media_type, image.bytes.clone())?;
                Some(FileImage {
                    media_type: image.media_type,
                    encoded: base64::engine::general_purpose::STANDARD
                        .encode(image.bytes)
                        .into(),
                    preview,
                })
            });
            self.undo.push(self.edit_snapshot());
            self.redo.clear();
            let range =
                self.expanded_chip_range(self.clamp_byte_range(self.selected_range.clone()));
            self.remove_chips_in_range(&range);
            let mut content = self.content.to_string();
            let mut inserted = String::new();
            if content[..range.start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            {
                inserted.push(' ');
            }
            inserted.push_str(&marker);
            inserted.push(' ');
            content.replace_range(range.clone(), &inserted);
            self.rebase_live_dictation_after_edit(&range, inserted.len());
            self.content = content.into();
            self.control_revision = self.control_revision.wrapping_add(1);
            let cursor = range.start + inserted.len();
            self.selected_range = cursor..cursor;
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::File {
                    path: path.clone(),
                    image,
                    file,
                },
            });
        }
        self.attachment_preview = None;
        self.marked_range = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn attach_pasted_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let lines = text.split('\n').count();
        let description = if lines > 10 {
            format!("Pasted text, {lines} lines")
        } else {
            format!("Pasted text, {} chars", text.encode_utf16().count())
        };
        let mut marker = format!("[{description}]");
        let mut n = 2;
        while self.content.contains(&marker)
            || self.attachments.iter().any(|a| a.label.as_ref() == marker)
        {
            marker = format!("[{description} #{n}]");
            n += 1;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let range = self.expanded_chip_range(self.clamp_byte_range(self.selected_range.clone()));
        self.remove_chips_in_range(&range);
        let mut content = self.content.to_string();
        content.replace_range(range.clone(), &marker);
        self.rebase_live_dictation_after_edit(&range, marker.len());
        self.content = content.into();
        self.control_revision = self.control_revision.wrapping_add(1);
        let cursor = range.start + marker.len();
        self.selected_range = cursor..cursor;
        self.attachments.push(Attachment {
            label: marker.into(),
            payload: AttachmentPayload::Text(text.into()),
        });
        self.attachment_preview = None;
        self.attachment_notice = None;
        self.marked_range = None;
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn delete_word_back(
        &mut self,
        _: &DeleteWordBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn kill_to_start(&mut self, _: &KillToStart, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn kill_to_end(&mut self, _: &KillToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.edit_snapshot());
            self.attachments = previous.attachments;
            self.set_content(previous.content, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.edit_snapshot());
            self.attachments = next.attachments;
            self.set_content(next.content, cx);
        }
    }

    fn history_prev(&mut self, _: &HistoryPrev, _: &mut Window, cx: &mut Context<Self>) {
        let suggestions = self.command_suggestions();
        if !suggestions.is_empty() {
            self.command_selection = self
                .command_selection
                .saturating_sub(1)
                .min(suggestions.len() - 1);
            self.command_selection_explicit =
                Some(suggestions[self.command_selection].value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
            return;
        }
        if self.move_visual_row(-1, cx) || self.history.is_empty() {
            return;
        }
        if self.history_index.is_some() && !self.attachments.is_empty() {
            self.set_transient_notice("Remove or send images before browsing history".into(), cx);
            return;
        }
        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                let (content, selection) = self.draft_without_live_dictation();
                self.live_draft = Some(HistoryDraft {
                    content: content.into_owned(),
                    selection,
                    selection_reversed: self.selection_reversed,
                    attachments: std::mem::take(&mut self.attachments),
                });
                self.attachment_preview = None;
                self.attachment_notice = None;
                self.history.len() - 1
            }
        };
        self.show_history_entry(index, cx);
    }

    fn history_next(&mut self, _: &HistoryNext, _: &mut Window, cx: &mut Context<Self>) {
        let suggestions = self.command_suggestions();
        if !suggestions.is_empty() {
            self.command_selection = (self.command_selection + 1).min(suggestions.len() - 1);
            self.command_selection_explicit =
                Some(suggestions[self.command_selection].value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
            return;
        }
        if self.move_visual_row(1, cx) {
            return;
        }
        let Some(index) = self.history_index else {
            return;
        };
        if !self.attachments.is_empty() {
            self.set_transient_notice("Remove or send images before browsing history".into(), cx);
            return;
        }
        if index + 1 < self.history.len() {
            self.show_history_entry(index + 1, cx);
        } else {
            self.history_index = None;
            if let Some(draft) = self.live_draft.take() {
                self.restore_history_draft(draft, cx);
            }
        }
    }

    fn show_history_entry(&mut self, index: usize, cx: &mut Context<Self>) {
        self.history_index = Some(index);
        self.marked_range = None;
        self.set_content(self.history[index].clone(), cx);
        self.close_command_menu(CommandMenuClose::Literal);
    }

    fn restore_history_draft(&mut self, draft: HistoryDraft, cx: &mut Context<Self>) {
        self.attachments = draft.attachments;
        self.set_content(draft.content, cx);
        self.selected_range = draft.selection;
        self.selection_reversed = draft.selection_reversed;
    }

    fn clear(&mut self, _: &Clear, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachment_preview.is_some() {
            self.close_attachment_preview(window, cx);
            return;
        }
        if self.ghost_text().is_some() {
            self.dismiss_ghost();
            cx.notify();
            return;
        }
        // Inside the picker, Esc steps back before it closes.
        let in_model_menu = self.picker_prefix == Some("/model")
            || self.content.trim() == "/model"
            || self.content.starts_with("/model ");
        if in_model_menu && self.model_stage_back(cx) {
            return;
        }
        if self.picker_prefix.is_none()
            && self.current_command_menu_close() != Some(CommandMenuClose::Literal)
            && (in_model_menu
                || self.mention_token().is_some()
                || self.current_command_menu_close() == Some(CommandMenuClose::Completed)
                || !self.command_suggestions().is_empty())
            && !self.standalone_context()
        {
            self.close_command_menu(CommandMenuClose::Literal);
            cx.notify();
            return;
        }
        if let Some(cancel) = &self.on_overlay_cancel
            && cancel(cx)
        {
            return;
        }
        if !self.content.is_empty() {
            self.undo.push(self.edit_snapshot());
            self.redo.clear();
            self.attachments.clear();
            self.set_content(String::new(), cx);
        }
    }

    /// Last painted composer bounds in window coordinates, if available.
    /// Fall back to the editor viewport, then text bounds before the root is measured.
    pub(crate) fn voice_bounds(&self) -> Option<Bounds<Pixels>> {
        let viewport = self.editor_scroll.bounds();
        let valid = |bounds: &Bounds<Pixels>| {
            f32::from(bounds.origin.x).is_finite()
                && f32::from(bounds.origin.y).is_finite()
                && f32::from(bounds.size.width).is_finite()
                && f32::from(bounds.size.height).is_finite()
                && bounds.size.width > px(0.0)
                && bounds.size.height > px(0.0)
        };
        if let Some(bounds) = self.composer_bounds.get().filter(valid) {
            Some(bounds)
        } else if valid(&viewport) {
            Some(viewport)
        } else {
            self.last_bounds.filter(valid)
        }
    }

    /// Display recognition partials without changing the draft or edit history.
    pub(crate) fn set_voice_preview(&mut self, preview: Option<String>, cx: &mut Context<Self>) {
        if self.voice_preview != preview {
            if preview.is_some() {
                self.voice_preview_scroll.scroll_to_bottom();
            }
            self.voice_preview = preview;
            cx.notify();
        }
    }

    /// Test hook: the dim recognition partial currently shown, if any.
    #[cfg(test)]
    pub(crate) fn voice_preview(&self) -> Option<&str> {
        self.voice_preview.as_deref()
    }

    /// One dim line above the partials saying where the words will land.
    pub(crate) fn set_voice_preview_hint(&mut self, hint: Option<&str>, cx: &mut Context<Self>) {
        let hint = hint.filter(|hint| !hint.is_empty()).map(SharedString::from);
        if self.voice_preview_hint != hint {
            self.voice_preview_hint = hint;
            cx.notify();
        }
    }

    /// Insert dictated text at the caret as one undoable edit. This is what
    /// `[dictation] mode = "insert"` means: the words arrive where the user was
    /// typing (replacing a selection, like typing over it), the caret lands
    /// after them, and nothing is submitted. Words are separated from adjacent
    /// text so two dictated phrases do not fuse into one token.
    pub(crate) fn insert_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let range = self.expanded_chip_range(self.clamp_byte_range(self.selected_range.clone()));
        let mut insert = String::with_capacity(text.len() + 2);
        if self.content[..range.start]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace())
        {
            insert.push(' ');
        }
        insert.push_str(text);
        if self.content[range.end..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
        {
            insert.push(' ');
        }
        let caret = range.start + insert.len();
        let edited = self.content[..range.start].to_owned() + &insert + &self.content[range.end..];
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.rebase_live_dictation_after_edit(&range, insert.len());
        self.content = edited.into();
        self.marked_range = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.reset_model_groups_after_exit();
        self.command_selection = 0;
        self.reveal_command(0);
        self.dismiss_ghost();
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
        cx.notify();
    }

    /// Append asynchronous dictation on a new line as one undoable edit,
    /// without replacing newer edits or attachments.
    pub(crate) fn append_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let mut draft = self.content.to_string();
        if !draft.is_empty() && !draft.ends_with('\n') {
            draft.push('\n');
        }
        draft.push_str(text);
        self.marked_range = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        self.set_content(draft, cx);
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
    }

    /// Replace the whole draft with dictated text as one undoable edit. This is
    /// what `[dictation] mode = "replace"` means: the utterance stands where the
    /// draft was, the discarded draft stays one undo away, and redo is cleared
    /// because the new content is not a step back toward the old future.
    pub(crate) fn replace_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.marked_range = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        let mut content = text.to_owned();
        ensure_visible_chips(&mut content, &mut self.attachments);
        self.set_content(content, cx);
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
    }

    fn reset_model_groups_after_exit(&mut self) {
        if self.picker_prefix == Some("/model") {
            return;
        }
        let content = self.content.trim_start();
        if content.contains('\n') || !(content == "/model" || content.starts_with("/model ")) {
            self.expanded_model_groups.clear();
            self.model_stage = Default::default();
            self.picker_effort = None;
            self.suggestions_revision += 1;
        }
    }

    pub(crate) fn start_picker_search(
        &mut self,
        prefix: &'static str,
        fallback_models: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.command_models.is_empty() {
            self.set_command_models(fallback_models, cx);
        }
        self.picker_prefix = Some(prefix);
        self.model_stage = Default::default();
        self.picker_effort = None;
        self.suggestions_revision += 1;
        self.set_content(String::new(), cx);
        if prefix == "/effort"
            && let Some(index) = self
                .command_suggestions()
                .iter()
                .position(|row| row.active_choice)
        {
            self.command_selection = index;
            self.reveal_command(index);
        }
    }

    pub(crate) fn set_speed_picker(
        &mut self,
        picker: crate::speed_icon::SpeedPicker,
        cx: &mut Context<Self>,
    ) {
        if self.speed_picker.as_ref() == Some(&picker) {
            return;
        }
        self.speed_picker = Some(picker);
        self.suggestions_revision += 1;
        cx.notify();
    }

    pub(crate) fn focus_next_picker_row_for_screenshot(&mut self, cx: &mut Context<Self>) {
        let rows = self.command_suggestions();
        let Some(index) = rows.iter().position(|row| row.active_choice) else {
            return;
        };
        if let Some(next) = rows.get(index + 1) {
            self.command_selection = index + 1;
            self.command_selection_explicit = Some(next.value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn open_model_menu(&mut self, fallback_models: Vec<String>, cx: &mut Context<Self>) {
        // Offline/reconnected inputs may not have received route metadata yet.
        // Never replace the canonical grouped routes once they are available.
        if self.command_models.is_empty() {
            self.set_command_models(fallback_models, cx);
        }
        if self.model_menu_draft.is_none()
            && !(self.content.trim() == "/model"
                || self.content.trim_start().starts_with("/model "))
        {
            let draft = self.draft_without_live_dictation().0.into_owned();
            self.model_menu_draft = Some((draft.into(), std::mem::take(&mut self.attachments)));
            self.attachment_preview = None;
        }
        self.model_stage = Default::default();
        self.suggestions_revision += 1;
        self.set_content("/model ".into(), cx);
    }

    /// Short feedback under the composer that clears itself, e.g. after an
    /// effort key. A newer notice restarts the timer.
    pub fn set_transient_notice(&mut self, notice: String, cx: &mut Context<Self>) {
        self.transient_notice_generation += 1;
        let generation = self.transient_notice_generation;
        self.transient_notice = Some(notice.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2_500))
                .await;
            let _ = this.update(cx, |input, cx| {
                if input.transient_notice_generation == generation {
                    input.transient_notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(crate) fn transient_notice(&self) -> Option<&str> {
        self.transient_notice.as_deref()
    }

    /// The serving model's effort ladder and level, for the `/effort` menu.
    pub fn set_effort_state(
        &mut self,
        ladder: Vec<&'static str>,
        current: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.effort_ladder != ladder || self.current_effort != current {
            self.effort_ladder = ladder;
            self.current_effort = current;
            self.suggestions_revision += 1;
            cx.notify();
        }
    }

    pub(crate) fn effort_menu_open(&self) -> bool {
        let content = self.content.trim_start();
        content == "/effort" || content.starts_with("/effort ")
    }

    /// Restore the stashed draft after the effort menu closes or submits.
    pub(crate) fn close_effort_menu(&mut self, cx: &mut Context<Self>) {
        if let Some((draft, attachments)) = self.model_menu_draft.take() {
            self.attachments = attachments;
            self.set_content(draft.to_string(), cx);
        } else if self.effort_menu_open() {
            self.set_content(String::new(), cx);
        }
    }

    pub(crate) fn close_model_menu(&mut self, cx: &mut Context<Self>) {
        if let Some((draft, attachments)) = self.model_menu_draft.take() {
            self.attachments = attachments;
            self.set_content(draft.to_string(), cx);
        } else if self.content.trim() == "/model"
            || self.content.trim_start().starts_with("/model ")
        {
            self.set_content(String::new(), cx);
        }
    }

    pub(crate) fn append_queued_files(
        &mut self,
        files: Vec<SubmittedFile>,
        cx: &mut Context<Self>,
    ) {
        let mut content = self.content.to_string();
        for file in files {
            let name = file.name.replace(['[', ']', '\n', '\r'], " ");
            let mut marker = format!("[#{name}, {} B]", file.size);
            let mut index = 2;
            while content.contains(&marker) {
                marker = format!("[#{name}, {} B #{index}]", file.size);
                index += 1;
            }
            let image = base64::engine::general_purpose::STANDARD
                .decode(&file.data)
                .ok()
                .and_then(|bytes| preview_image(&file.mime, bytes))
                .map(|preview| FileImage {
                    media_type: file.mime.clone(),
                    encoded: file.data.clone().into(),
                    preview,
                });
            if !content.is_empty() && !content.ends_with(char::is_whitespace) {
                content.push(' ');
            }
            content.push_str(&marker);
            // The recalled prompt already contains the original path.
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::File {
                    path: std::path::PathBuf::new(),
                    image,
                    file,
                },
            });
        }
        self.set_content(content, cx);
    }

    /// Put a recalled queued prompt back into the composer for editing. Any
    /// existing draft is kept ahead of it so nothing typed is lost.
    pub(crate) fn recall_prompt(
        &mut self,
        content: &str,
        images: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        let draft = self.content.trim_end().to_string();
        let ranges = image_marker_ranges(content);
        let mut recalled = content.to_string();
        let mut replacements = Vec::new();
        let mut used: HashSet<String> = image_marker_ranges(&draft)
            .into_iter()
            .map(|range| draft[range].to_string())
            .collect();
        for (index, (media_type, encoded)) in images.into_iter().enumerate() {
            let Some(bytes) = base64::engine::general_purpose::STANDARD
                .decode(&encoded)
                .ok()
            else {
                continue;
            };
            let Some(preview) = preview_image(&media_type, bytes) else {
                continue;
            };
            let original = ranges
                .get(index)
                .map(|range| content[range.clone()].to_string());
            let marker = original
                .clone()
                .filter(|marker| !used.contains(marker))
                .unwrap_or_else(|| {
                    (1..)
                        .map(|n| format!("[#image {n}]"))
                        .find(|marker| !used.contains(marker) && !content.contains(marker))
                        .unwrap()
                });
            used.insert(marker.clone());
            if let Some(range) = ranges.get(index) {
                if original.as_deref() != Some(marker.as_str()) {
                    replacements.push((range.clone(), marker.clone()));
                }
            } else {
                replacements.push((content.len()..content.len(), format!(" {marker}")));
            }
            self.attachments.push(Attachment {
                label: marker.into(),
                payload: AttachmentPayload::Image {
                    media_type,
                    encoded: encoded.into(),
                    preview,
                },
            });
        }
        for (range, marker) in replacements.into_iter().rev() {
            recalled.replace_range(range, &marker);
        }
        let merged = if draft.is_empty() {
            recalled
        } else {
            format!("{draft}\n\n{recalled}")
        };
        self.set_content(merged, cx);
    }

    pub(crate) fn control_revision(&self) -> u64 {
        self.control_revision
    }

    pub(crate) fn set_content(&mut self, content: String, cx: &mut Context<Self>) {
        self.live_dictation = None;
        self.dismiss_ghost();
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.control_revision = self.control_revision.wrapping_add(1);
        self.content = content.into();
        self.reset_model_groups_after_exit();
        self.command_selection = 0;
        self.reveal_command(0);
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    pub(crate) fn set_skills(&mut self, skills: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.command_skills = skills;
        self.suggestions_revision += 1;
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn command_suggestion_values(&self) -> Vec<String> {
        self.command_suggestions()
            .iter()
            .map(|row| row.value.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn selected_command_value(&self) -> Option<String> {
        self.command_suggestions()
            .get(self.command_selection)
            .map(|row| row.value.clone())
    }

    pub(crate) fn set_current_modes(
        &mut self,
        modes: Vec<String>,
        catalog: crate::mode_catalog::ModeCatalog,
        cx: &mut Context<Self>,
    ) {
        if self.current_modes == modes && self.mode_catalog == catalog {
            return;
        }
        let selected = self.selected_mode_name();
        self.current_modes = modes;
        self.mode_catalog = catalog;
        self.suggestions_revision += 1;
        if let Some(index) = selected.and_then(|name| {
            self.command_suggestions()
                .iter()
                .position(|row| row.value.strip_prefix("/mode ") == Some(&name))
        }) {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    pub(crate) fn mode_rows_for_control(&self) -> Vec<serde_json::Value> {
        self.command_suggestions()
            .iter()
            .filter_map(|row| {
                let name = row.value.strip_prefix("/mode ")?;
                Some(serde_json::json!({
                    "name": name, "section": row.header,
                    "active": self.current_modes.iter().any(|active| active == name),
                    "icon": crate::mode_catalog::icon(name, self.mode_catalog.get(name)).name(),
                }))
            })
            .collect()
    }

    fn selected_mode_name(&self) -> Option<String> {
        self.command_suggestions()
            .get(self.command_selection)?
            .value
            .strip_prefix("/mode ")
            .map(str::to_owned)
    }

    fn toggle_mode_row(&self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let command = if name == "off" {
            "/mode off".to_owned()
        } else {
            format!("/mode toggle {name}")
        };
        (self.on_submit)(
            SubmissionKind::Command,
            command,
            Vec::new(),
            Vec::new(),
            false,
            window,
            cx,
        );
    }

    fn current_command_menu_close(&self) -> Option<CommandMenuClose> {
        self.command_menu_closed
            .as_ref()
            .filter(|(draft, _)| draft == &self.content)
            .map(|(_, close)| *close)
    }

    fn close_command_menu(&mut self, close: CommandMenuClose) {
        self.command_menu_closed = Some((self.content.clone(), close));
        self.command_selection_explicit = None;
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The editor owns this gesture, including repeated clicks. Do not let
        // it reach a caption-background listener behind a header-area composer.
        // Focus explicitly because stopping propagation also skips track_focus.
        window.focus(&self.focus_handle, cx);
        cx.stop_propagation();
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position, window);
        let clicked_chip = self.last_bounds.as_ref().and_then(|text_bounds| {
            let relative = event.position - text_bounds.origin;
            self.chip_positions
                .iter()
                .find_map(|(index, bounds)| bounds.contains(&relative).then_some(*index))
        });
        if !event.modifiers.shift
            && let Some(index) = clicked_chip.or_else(|| {
                self.chip_ranges()
                    .into_iter()
                    .find_map(|(index, range)| range.contains(&offset).then_some(index))
            })
        {
            self.is_selecting = false;
            self.activate_attachment(index, window, cx);
            return;
        }
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else if event.click_count == 2 {
            self.selected_range = clicked_word_range(&self.content, offset);
            self.selection_reversed = false;
            cx.notify();
        } else {
            self.move_to(offset, cx)
        }
    }

    fn activate_attachment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attachment) = self.attachments.get(index) else {
            return;
        };
        match &attachment.payload {
            AttachmentPayload::Text(text) => {
                if let Some(range) = self.chip_range(attachment) {
                    let text = text.clone();
                    window.focus(&self.focus_handle, cx);
                    self.selected_range = range;
                    self.marked_range = None;
                    self.replace_text_in_range(None, &text, window, cx);
                }
            }
            AttachmentPayload::Image { .. } | AttachmentPayload::File { image: Some(_), .. } => {
                self.attachment_preview = Some(paste_preview::Preview::new(index));
                window.focus(&self.preview_close_focus, cx);
            }
            AttachmentPayload::Context(context_picker::DraftContext::File { path, .. }) => {
                let action = crate::ui_actions::OpenDocumentFile { path: path.clone(), fragment: None };
                window.dispatch_action(Box::new(action), cx);
            }
            _ => return,
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position, window), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.vertical_caret = None;
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify()
    }

    fn vertical_caret(&self) -> Option<&VerticalCaret> {
        self.vertical_caret.as_ref().filter(|caret| {
            caret.revision == self.control_revision
                && self.selected_range.is_empty()
                && caret.offset == self.cursor_offset()
        })
    }

    fn caret_position(&self, layout: &PromptLayout, line_height: Pixels) -> Option<Point<Pixels>> {
        layout.position_for_caret(
            self.cursor_offset(),
            line_height,
            self.vertical_caret().is_some_and(|caret| caret.downstream),
        )
    }

    /// False means the caret is already at the edge in this direction.
    fn move_visual_row(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        if self.content.is_empty() {
            return false;
        }
        let (Some(layout), Some(height)) = (&self.last_layout, self.last_line_height) else {
            return true;
        };
        if self.last_layout_revision != self.control_revision {
            return true;
        }
        let Some(caret) = self.caret_position(layout, height) else {
            return true;
        };
        let Some(end) = layout.position_for_index(self.content.len(), height) else {
            return true;
        };
        let row = |y: Pixels| (y / height).round() as isize;
        let goal_x = self.vertical_caret().map_or(caret.x, |caret| caret.goal_x);
        let mut target = row(caret.y) + step;
        let last = row(end.y);
        let chips = self.chip_ranges();
        while (0..=last).contains(&target) {
            let offset = layout
                .closest_index_for_position(point(goal_x, height * (target as f32 + 0.5)), height)
                .unwrap_or_else(|offset| offset)
                .min(self.content.len());
            let mut grapheme = GraphemeCursor::new(offset, self.content.len(), true);
            let candidates = if let Some((_, chip)) = chips
                .iter()
                .find(|(_, range)| range.start < offset && offset < range.end)
            {
                [chip.start, chip.end]
            } else if grapheme.is_boundary(&self.content, 0).unwrap_or(false) {
                [offset, offset]
            } else {
                let before = grapheme.prev_boundary(&self.content, 0).ok().flatten();
                let mut grapheme = GraphemeCursor::new(offset, self.content.len(), true);
                let after = grapheme.next_boundary(&self.content, 0).ok().flatten();
                [
                    before.unwrap_or_default(),
                    after.unwrap_or(self.content.len()),
                ]
            };
            let nearest = candidates
                .into_iter()
                .flat_map(|offset| [(offset, false), (offset, true)])
                .filter_map(|(offset, downstream)| {
                    let position = layout.position_for_caret(offset, height, downstream)?;
                    (row(position.y) == target).then_some((
                        offset,
                        downstream,
                        f32::from((position.x - goal_x).abs()),
                    ))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((offset, downstream, _)) = nearest {
                self.move_to(offset, cx);
                self.vertical_caret = Some(VerticalCaret {
                    offset,
                    revision: self.control_revision,
                    goal_x,
                    downstream,
                });
                return true;
            }
            target += step;
        }
        false
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn painted_line_height(&self, window: &Window) -> Pixels {
        self.last_line_height
            .unwrap_or_else(|| window.line_height())
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>, window: &Window) -> usize {
        let line_height = self.painted_line_height(window);
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_position(position - bounds.origin, line_height)
            .unwrap_or_else(|index| index)
            // The layout includes ghost text past the draft.
            .min(self.content.len())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.vertical_caret = None;
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    /// `range` ordered, inside `content`, and on character boundaries. Marked
    /// and selected ranges can go stale when content changes outside the IME
    /// (history recall, dictation, a session switch), so IME edits clamp them
    /// rather than slicing with them blindly.
    fn clamp_byte_range(&self, range: Range<usize>) -> Range<usize> {
        let floor = |mut offset: usize| {
            offset = offset.min(self.content.len());
            while !self.content.is_char_boundary(offset) {
                offset -= 1;
            }
            offset
        };
        let start = floor(range.start.min(range.end));
        start..floor(range.start.max(range.end)).max(start)
    }

    /// Byte range for a platform UTF-16 range, always ordered and on character
    /// boundaries so slicing `content` with it cannot panic.
    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range_utf16.start);
        start..self.offset_from_utf16(range_utf16.end).max(start)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        if let Some((_, range)) = self
            .chip_ranges()
            .into_iter()
            .find(|(_, range)| offset > range.start && offset <= range.end)
        {
            return range.start;
        }
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        if let Some((_, range)) = self
            .chip_ranges()
            .into_iter()
            .find(|(_, range)| offset >= range.start && offset < range.end)
        {
            return range.end;
        }
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        previous_word_boundary(&self.content, offset)
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        next_word_boundary(&self.content, offset)
    }
}

fn clicked_word_range(text: &str, offset: usize) -> Range<usize> {
    // Hit testing returns UTF-8 boundaries. At the end of a line/document,
    // select the final segment rather than an empty word after the text.
    let probe = offset.min(text.len().saturating_sub(1));
    text.split_word_bound_indices()
        .find_map(|(start, word)| {
            let end = start + word.len();
            (start <= probe && probe < end).then_some(start..end)
        })
        .unwrap_or(0..0)
}

fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let prefix = &text[..offset];
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    trimmed
        .char_indices()
        .rev()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index + ch.len_utf8()))
        .unwrap_or(0)
}

/// Byte offset of a UTF-16 offset within `text`, clamped to `text.len()`. An
/// offset that splits a surrogate pair rounds down to the character start.
fn utf16_to_byte_offset(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, ch) in text.char_indices() {
        if utf16 + ch.len_utf16() > offset {
            return index;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let suffix = &text[offset..];
    let word_end = suffix
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(suffix.len());
    let rest = &suffix[word_end..];
    offset
        + word_end
        + rest
            .char_indices()
            .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
            .unwrap_or(rest.len())
}

impl EntityInputHandler for PromptInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .map(|range| self.clamp_byte_range(range))
            .unwrap_or_else(|| self.clamp_byte_range(self.selected_range.clone()));

        let range = self.expanded_chip_range(range);
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.rebase_live_dictation_after_edit(&range, new_text.len());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.reset_model_groups_after_exit();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.command_selection = 0;
        self.reveal_command(0);
        self.ghost_on_edit(cx);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .map(|range| self.clamp_byte_range(range))
            .unwrap_or_else(|| self.clamp_byte_range(self.selected_range.clone()));

        let range = self.expanded_chip_range(range);
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.command_menu_closed = None;
        self.command_selection_explicit = None;
        self.rebase_live_dictation_after_edit(&range, new_text.len());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.reset_model_groups_after_exit();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        // The IME's selection is UTF-16 offsets into `new_text`, not into the
        // whole document. Converting it against the document (as this once
        // did) lands past the end or inside a multi-byte character as soon as
        // CJK text precedes the composition, and the next slice panics. On
        // macOS that panic aborts the app from inside `setMarkedText:` (#6).
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selected| {
                let start = utf16_to_byte_offset(new_text, selected.start);
                let end = utf16_to_byte_offset(new_text, selected.end).max(start);
                range.start + start..range.start + end
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        self.dismiss_ghost();

        self.command_selection = 0;
        self.reveal_command(0);
        if let Some(on_change) = &self.on_change {
            on_change(&self.content, cx);
        }

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let geometry = self.ime_geometry.as_ref()?;
        let chips = self.chip_ranges();
        let display_text = if chips.is_empty() {
            self.content.clone()
        } else {
            crate::image_chip::mask_markers(
                &self.content,
                chips.into_iter().map(|(_, range)| range),
            )
            .into()
        };
        geometry.bounds_for_range(
            display_text,
            self.range_from_utf16(&range_utf16),
            bounds.origin,
            window,
        )
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout
            .closest_index_for_position(local, self.painted_line_height(_window))
            .unwrap_or_else(|index| index)
            .min(self.content.len());
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<PromptInput>,
}

struct PrepaintState {
    ime_geometry: ImeGeometry,
    line: Option<PromptLayout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    text_bounds: Bounds<Pixels>,
    /// Horizontal gap between the caret and placeholder text, so the caret
    /// never overlaps the first placeholder glyph.
    text_indent: Pixels,
    visual_line_count: usize,
}

/// Space kept between the caret and the example placeholder.
const PLACEHOLDER_INDENT: Pixels = px(6.);
/// Fresh-session composer height: one 16px line plus its 16px padding.
pub(crate) const SPACIOUS_MIN_HEIGHT: f32 = 58.;

fn selection_quads(
    line: &PromptLayout,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<PaintQuad> {
    range_quads(line, range, bounds, line_height, Theme::global().SELECTION)
}

fn range_quads(
    line: &PromptLayout,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    color: gpui::Rgba,
) -> Vec<PaintQuad> {
    let Some(start) = line.position_for_index(range.start, line_height) else {
        return Vec::new();
    };
    let Some(end) = line.position_for_index(range.end, line_height) else {
        return Vec::new();
    };
    let first_row = (start.y / line_height) as usize;
    let last_row = (end.y / line_height) as usize;
    (first_row..=last_row)
        .map(|row| {
            let left = if row == first_row { start.x } else { px(0.) };
            let right = if row == last_row {
                end.x
            } else {
                bounds.size.width
            };
            fill(
                Bounds::new(
                    point(bounds.left() + left, bounds.top() + line_height * row),
                    size((right - left).max(px(0.)), line_height),
                ),
                to_hsla(color),
            )
        })
        .collect()
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let line_count = self.input.read(cx).visual_line_count.max(1);
        style.size.height = (window.line_height() * line_count).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // The executor clock, so motion and its sleeping ticker share one
        // time source (real time in production, simulated in tests).
        let now = cx.background_executor().now();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let mut placeholder_full = None;
        let mut placeholder_live = false;
        let text_indent = if content.is_empty() {
            PLACEHOLDER_INDENT
        } else {
            px(0.)
        };
        let wrap_width = (bounds.size.width - text_indent).max(px(0.));
        let (display_text, text_color) = if content.is_empty() {
            let (mut text, full, live) = input.placeholder_text(now, cx);
            placeholder_full = full.clone();
            // Only an active window animates. Background windows show the
            // whole example rather than freezing mid-word.
            placeholder_live = live && window.is_window_active();
            if live
                && !placeholder_live
                && let Some(full) = &full
            {
                text = full.clone();
            }
            (text, to_hsla(Theme::global().composer_secondary()))
        } else {
            (content, style.color)
        };
        let ghost = (!input.content.is_empty())
            .then(|| input.ghost_text())
            .flatten()
            .map(str::to_owned);
        let draft_len = display_text.len();
        let display_text: SharedString = match &ghost {
            Some(ghost) => format!("{display_text}{ghost}").into(),
            None => display_text,
        };

        let run = TextRun {
            len: draft_len,
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Only underline a composition that actually lies in the shown text.
        // A placeholder or stale range would otherwise underflow the run math.
        let marked_range = input.marked_range.as_ref().filter(|marked| {
            !input.content.is_empty()
                && marked.start <= marked.end
                && marked.end <= display_text.len()
                && display_text.is_char_boundary(marked.start)
                && display_text.is_char_boundary(marked.end)
        });
        let ghost_run = run.clone();
        let runs = if let Some(marked_range) = marked_range {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: draft_len - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let mut runs: Vec<TextRun> = runs;
        // Preserve source byte offsets for editing and submission, but never
        // paint marker glyphs. The positioned pill paints the whole marker.
        if !input.content.is_empty() {
            let hidden: Vec<_> = input
                .chip_ranges()
                .into_iter()
                .map(|(_, range)| range)
                .collect();
            let mut offset = 0;
            runs = runs
                .into_iter()
                .flat_map(|run| {
                    let start = offset;
                    let end = start + run.len;
                    offset = end;
                    let mut cuts = vec![start, end];
                    for range in &hidden {
                        if range.start > start && range.start < end {
                            cuts.push(range.start);
                        }
                        if range.end > start && range.end < end {
                            cuts.push(range.end);
                        }
                    }
                    cuts.sort_unstable();
                    cuts.dedup();
                    cuts.windows(2)
                        .map(|pair| {
                            let mut part = run.clone();
                            part.len = pair[1] - pair[0];
                            if hidden
                                .iter()
                                .any(|range| range.start <= pair[0] && pair[1] <= range.end)
                            {
                                part.color.a = 0.;
                            }
                            part
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
        }
        if let Some(ghost) = &ghost {
            runs.push(TextRun {
                len: ghost.len(),
                color: to_hsla(Theme::global().composer_secondary()),
                ..ghost_run
            });
        }

        // Shape a same-length word in place of each ASCII marker. GPUI's
        // wrapper breaks on punctuation and even non-breaking spaces, but not
        // inside this word. Byte offsets stay identical to the stored text.
        let shaped = crate::image_chip::mask_markers(
            &display_text,
            input.chip_ranges().into_iter().map(|(_, range)| range),
        );

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let ime_geometry = ImeGeometry::new(
            style.font(),
            font_size,
            line_height,
            Some(bounds.size.width),
        );
        let line = PromptLayout::new(
            window
                .text_system()
                .shape_text(shaped.into(), font_size, &runs, Some(wrap_width), None)
                .expect("prompt text should shape"),
        );
        let target = input.caret_position(&line, line_height).unwrap_or_default();
        let mut visual_line_count = line.visual_line_count();
        if let Some(full) = placeholder_full.filter(|full| !full.is_empty()) {
            // Reserve the fully typed prompt's height so typing never resizes
            // the composer mid-word at narrow widths.
            let full_lines = window
                .text_system()
                .shape_text(
                    full.clone(),
                    font_size,
                    &[TextRun {
                        len: full.len(),
                        font: style.font(),
                        color: text_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    Some(wrap_width),
                    None,
                )
                .map(|lines| PromptLayout::new(lines).visual_line_count())
                .unwrap_or(1);
            visual_line_count = visual_line_count.max(full_lines);
        }
        let text_bounds = Bounds::new(
            bounds.origin,
            size(bounds.size.width, line_height * visual_line_count),
        );
        let focused = self.input.read(cx).focus_handle.is_focused(window);
        let reduced = motion_reduced(cx);
        let (cursor_pos, caret_alpha, caret_next, caret_gliding) = {
            let input = self.input.read(cx);
            let mut state = input.motion.borrow_mut();
            let state = &mut *state;
            let key = (input.content.clone(), cursor);
            let mut edited = false;
            if state.key.as_ref() != Some(&key) {
                edited = state.key.as_ref().is_some_and(|(old, _)| *old != key.0);
                if key.0.is_empty() && state.key.as_ref().is_some_and(|(old, _)| !old.is_empty()) {
                    // Emptying the composer starts a fresh example.
                    state.seed = state.seed.wrapping_add(1);
                }
                state.key = Some(key);
                state.epoch = now;
            }
            let epoch = state.epoch;
            let glide = state
                .glide
                .get_or_insert_with(|| motion::Glide::new(target, now));
            if edited {
                // Typed text lands this frame, so the caret must too. A
                // glide here trails the new character and forces extra
                // full-panel frames between keystrokes.
                glide.snap(target, now);
            } else {
                glide.retarget(target, line_height, now, reduced);
            }
            let (position, gliding) = glide.position(now);
            let (alpha, next) = motion::caret_alpha(now.saturating_duration_since(epoch), reduced);
            (
                position,
                alpha,
                next.filter(|_| focused),
                focused && gliding,
            )
        };
        if caret_gliding {
            // The 33ms ticker is too coarse for a 55ms glide. Draw every frame.
            window.request_animation_frame();
        }
        let motion_next = if placeholder_live {
            Some(Duration::ZERO)
        } else {
            caret_next
        };
        self.input.read(cx).motion.borrow_mut().next = motion_next;
        let (selection, cursor) = if selected_range.is_empty() {
            let mut color = to_hsla(Theme::global().CURSOR);
            color.a *= caret_alpha;
            (
                Vec::new(),
                Some(gpui::quad(
                    Bounds::new(text_bounds.origin + cursor_pos, size(px(2.), line_height)),
                    px(1.),
                    color,
                    px(0.),
                    gpui::transparent_black(),
                    gpui::BorderStyle::default(),
                )),
            )
        } else {
            (
                selection_quads(&line, selected_range, text_bounds, line_height),
                None,
            )
        };
        PrepaintState {
            ime_geometry,
            line: Some(line),
            cursor,
            selection,
            text_bounds,
            text_indent,
            visual_line_count,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        #[cfg(target_os = "macos")]
        if focus_handle.is_focused(window) {
            crate::ime_log::record(format_args!(
                "composer-handler-register entity={:?}",
                self.input.entity_id()
            ));
        }
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            prepaint.text_bounds.origin + point(prepaint.text_indent, px(0.)),
            window.line_height(),
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            let positions: Vec<_> = input
                .chip_ranges()
                .into_iter()
                .filter_map(|(index, range)| {
                    let mut start = line.position_for_index(range.start, window.line_height())?;
                    let end = line.position_for_index(range.end, window.line_height())?;
                    // GPUI assigns a boundary index to the preceding row even
                    // when its first glyph is on the next visual row.
                    if start.y != end.y {
                        let next =
                            line.position_for_index(range.start + 1, window.line_height())?;
                        if next.y == end.y {
                            start = point(px(0.), next.y);
                        }
                    }
                    (start.y == end.y).then_some((
                        index,
                        Bounds::new(start, size(end.x - start.x, window.line_height())),
                    ))
                })
                .collect();
            if input.chip_positions != positions {
                input.chip_positions = positions;
                let entity = _cx.entity();
                _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
            }
            // Reveal edits and keyboard navigation, but do not undo wheel scrolling
            // on every paint. Include viewport size so resizing reveals the caret.
            let viewport = input.editor_scroll.bounds();
            let caret_key = (
                input.cursor_offset(),
                input.content.clone(),
                viewport.size,
                input.vertical_caret().is_some_and(|caret| caret.downstream),
            );
            if input.revealed_caret.as_ref() != Some(&caret_key) {
                if let Some(caret) = input.caret_position(&line, window.line_height()) {
                    let top = prepaint.text_bounds.top() + caret.y;
                    let bottom = top + window.line_height();
                    let adjustment = if top < viewport.top() {
                        viewport.top() - top
                    } else if bottom > viewport.bottom() {
                        viewport.bottom() - bottom
                    } else {
                        px(0.)
                    };
                    if adjustment != px(0.) {
                        let offset = input.editor_scroll.offset();
                        input
                            .editor_scroll
                            .set_offset(point(px(0.), (offset.y + adjustment).min(px(0.))));
                        let entity = _cx.entity();
                        _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                    }
                }
                input.revealed_caret = Some(caret_key);
            }
            input.last_layout = Some(line);
            input.ime_geometry = Some(prepaint.ime_geometry.clone());
            input.last_layout_revision = input.control_revision;
            input.last_bounds = Some(prepaint.text_bounds);
            input.last_line_height = Some(window.line_height());
            let needs_ticker = {
                let state = input.motion.borrow();
                state.next.is_some() && state.ticker.is_none()
            };
            if needs_ticker {
                // One bounded ticker drives placeholder typing and caret
                // breathing. It sleeps through the solid caret phase and exits
                // as soon as motion settles. It notifies this input by id and
                // never updates it, so the panel reading the input is not
                // built again for a caret frame.
                let shared = input.motion.clone();
                let id = _cx.entity_id();
                let weak = _cx.entity().downgrade();
                let ticker = _cx.spawn(async move |_, cx| {
                    loop {
                        let idle = shared.borrow().next.unwrap_or(Duration::ZERO);
                        if idle > motion::TICK {
                            cx.background_executor().timer(idle - motion::TICK).await;
                        }
                        crate::animation_clock::next_tick(cx.background_executor(), motion::TICK)
                            .await;
                        if weak.upgrade().is_none() {
                            break;
                        }
                        let keep = {
                            let mut state = shared.borrow_mut();
                            if state.next.take().is_none() {
                                state.ticker = None;
                                false
                            } else {
                                true
                            }
                        };
                        if !keep {
                            break;
                        }
                        cx.update(|cx| cx.notify(id));
                    }
                });
                input.motion.borrow_mut().ticker = Some(ticker);
            }
            if input.visual_line_count != prepaint.visual_line_count {
                // Layout used the previous height this frame. Retry revealing the
                // caret after the new scroll extent exists, even at the height cap.
                input.revealed_caret = None;
                input.visual_line_count = prepaint.visual_line_count;
                let entity = _cx.entity();
                _cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
            }
        });
    }
}

impl Render for PromptInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_scope = crate::render_stats::scope("PromptInput");
        let image_chip_numbers = self.image_chip_numbers();
        #[cfg(test)]
        crate::render_stats::test_renders::record(cx.entity_id());
        let focused = self.focus_handle.is_focused(window);
        #[cfg(target_os = "macos")]
        if crate::ime_log::enabled() && self.ime_log_focus_subscriptions.is_none() {
            let id = cx.entity_id();
            crate::ime_log::record(format_args!(
                "composer-focus-initial entity={id:?} focused={focused}"
            ));
            let focus = cx.on_focus(&self.focus_handle, window, move |_, _, _| {
                crate::ime_log::record(format_args!("composer-focus entity={id:?}"));
            });
            let blur = cx.on_blur(&self.focus_handle, window, move |_, _, _| {
                crate::ime_log::record(format_args!("composer-blur entity={id:?}"));
            });
            self.ime_log_focus_subscriptions = Some(vec![focus, blur]);
        }
        let editor_height = self.editor_height(window);
        let show_resize = self.can_resize_editor(window);
        let spacious = self.spacious && window.viewport_size().height >= px(400.);
        self.update_context_index(cx);
        let context_menu = self.standalone_context() || self.mention_token().is_some();
        let suggestions = self.command_suggestions();
        if self.command_selection >= suggestions.len() {
            self.command_selection = 0;
        }
        let command_visible = !self.external_picker_open
            && self.command_completion
            && self.current_command_menu_close().is_none()
            && (!suggestions.is_empty()
                || self.picker_prefix.is_some()
                || context_menu
                || self.content.trim_start().starts_with("/model "));
        let menu_duration = jcode_desktop_motion::policy(
            crate::transition::Transition::Menu,
            context_menu || crate::config::get().appearance.reduce_motion || cx.reduce_motion(),
        )
        .duration;
        let menu_progress =
            self.command_entrance
                .update(command_visible, Instant::now(), menu_duration);
        if self.command_entrance.is_animating() {
            window.request_animation_frame();
        }
        let command_layout_key = (!suggestions.is_empty()).then(|| {
            (
                self.content.clone(),
                suggestions.len(),
                window.viewport_size(),
            )
        });
        if self.command_layout_key != command_layout_key {
            self.command_layout_key = command_layout_key;
            // List geometry is updated during layout, after render.
            // One follow-up frame keeps the thumb correct without idle polling.
            let input = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = input.update(cx, |_, cx| cx.notify());
            });
        }
        let attachment_overlay = self.render_paste_preview(window, cx);
        div()
            .debug_selector(|| "prompt-input".into())
            .children(attachment_overlay)
            .flex()
            .flex_col()
            .key_context(if self.queue_editor {
                "PromptInput QueueEditor"
            } else if context_menu {
                "PromptInput ContextPicker"
            } else if self.on_model_speed.is_some()
                && self.model_stage == model_stage::ModelStage::Models
                && self.command_content().split_whitespace().next() == Some("/model")
            {
                "PromptInput ModelPicker"
            } else {
                "PromptInput"
            })
            .track_focus(&self.focus_handle(cx).tab_index(0).tab_stop(true))
            .relative()
            .child({
                let measured = self.composer_bounds.clone();
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, _, _| {
                        // Absolute children fill the padding box. Include the root's
                        // one-pixel border so reparenting preserves its exact footprint.
                        measured.set(Some(Bounds::new(
                            bounds.origin - point(px(1.0), px(1.0)),
                            size(bounds.size.width + px(2.0), bounds.size.height + px(2.0)),
                        )));
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            })
            .when(command_visible, |el| {
                el.child(
                    gpui::deferred(
                        popup::Popup::new(
                            div()
                                .id("slash-command-suggestions")
                                .debug_selector(|| "slash-command-overlay".into())
                                .w_full()
                                .max_h_full()
                                .opacity(if context_menu { 1.0 } else { 0.65 + 0.35 * menu_progress })
                                .flex()
                                .flex_col()
                                .rounded_lg()
                                .border_1()
                                .border_color(Theme::global().PANEL_BORDER_FOCUS)
                                .bg(Theme::global().HEADER_BG)
                                .shadow_lg()
                                .occlude()
                                .child(
                                    div()
                                        .relative()
                                        .flex()
                                        .flex_col()
                                        .min_h_0()
                                        .p_1()
                                        .child(
                                            div()
                                                .id("slash-command-scroll")
                                                .debug_selector(|| "slash-command-scroll".into())
                                                .min_h_0()
                                                .flex()
                                                .flex_col()
                                                .max_h(popup::scroll_height(
                                                    window.viewport_size().height,
                                                    context_menu || self.picker_prefix == Some("/model")
                                                        || self
                                                            .content
                                                            .trim_start()
                                                            .starts_with("/model"),
                                                ))
                                                .on_scroll_wheel(cx.listener(|_, _, _, cx| {
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                }))
                                                .pr(px(crate::scrollbar::GUTTER - 4.0))
                                                .when(suggestions.is_empty(), |el| {
                                                    el.child(
                                                        div()
                                                            .px_3()
                                                            .py_2()
                                                            .text_size(px(12.0))
                                                            .text_color(
                                                                Theme::global()
                                                                    .composer_secondary(),
                                                            )
                                                            .child(match self.picker_prefix {
                                                                _ if context_menu => self.context_empty_label(),
                                                                Some("/effort") => {
                                                                    "No effort levels match"
                                                                }
                                                                Some("/mode") => {
                                                                    "No modes or skills match"
                                                                }
                                                                _ => "No models match",
                                                            }),
                                                    )
                                                })
                                                .when(!suggestions.is_empty(), |el| {
                                                    let input = cx.entity();
                                                    el.child(
                                                        gpui::list(
                                                            self.command_scroll.clone(),
                                                            move |index, window, cx| {
                                                                input.update(cx, |input, cx| {
                                                                    input.render_command_row(
                                                                        index,
                                                                        suggestions[index].clone(),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                })
                                                            },
                                                        )
                                                        .with_sizing_behavior(
                                                            gpui::ListSizingBehavior::Infer,
                                                        )
                                                        .min_h_0()
                                                        .w_full(),
                                                    )
                                                }),
                                        )
                                        .child(crate::scrollbar::vertical_list(
                                            &self.command_scroll,
                                            "slash-command-scrollbar",
                                        )),
                                ),
                            px(4.0 + 6.0 * (1.0 - menu_progress)),
                        )
                        .dismiss_outside(self.on_picker_outside.clone()),
                    )
                    .with_priority(1),
                )
            })
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::shift_space))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(|this, _: &DecreasePickerSpeed, _, cx| {
                this.step_picker_speed(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &IncreasePickerSpeed, _, cx| {
                this.step_picker_speed(1, cx);
            }))
            // Inside the model menu the effort keys set the highlighted
            // row's level instead of the running session's.
            .capture_action(
                cx.listener(|this, _: &crate::effort::IncreaseEffort, _, cx| {
                    if this.step_picker_effort(1, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .capture_action(
                cx.listener(|this, _: &crate::effort::DecreaseEffort, _, cx| {
                    if this.step_picker_effort(-1, cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::kill_to_start))
            .on_action(cx.listener(Self::kill_to_end))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::history_prev))
            .on_action(cx.listener(Self::history_next))
            .on_action(cx.listener(Self::grow_editor))
            .on_action(cx.listener(Self::shrink_editor))
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::stash))
            .on_action(cx.listener(Self::accept_completion))
            .on_action(cx.listener(Self::alternate_context))
            .when(!self.host_handles_submission, |el| {
                el.on_action(cx.listener(Self::submit))
                    .on_action(cx.listener(Self::queue))
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w_0()
            .flex_none()
            .when(self.chrome, |el| el.bg(Theme::global().INPUT_BG))
            .border_1()
            .border_color(if !self.chrome {
                gpui::transparent_black().into()
            } else if focused {
                Theme::global().PANEL_BORDER_FOCUS
            } else {
                Theme::global().INPUT_BORDER
            })
            // Near-pill corners. A single-line composer is ~38px tall, so an
            // 18px radius reads as fully round while multi-line drafts keep
            // straight sides instead of turning into an ellipse.
            .rounded(px(18.0))
            .children(
                self.attachment_notice
                    .clone()
                    .or_else(|| self.transient_notice.clone())
                    .map(|notice| {
                        div()
                            .debug_selector(|| "composer-notice".into())
                            .px_3()
                            .pt_1()
                            .text_size(px(10.0))
                            .text_color(Theme::global().composer_secondary())
                            .child(notice)
                    }),
            )
            .when(self.stashed_prompt.is_some(), |el| {
                el.child(
                    div()
                        .debug_selector(|| "prompt-stashed".into())
                        .px_3()
                        .pt_1()
                        .text_size(px(10.0))
                        .text_color(Theme::global().composer_secondary())
                        .child("Prompt stashed · Ctrl+S to restore"),
                )
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .px_4()
                    .py_2()
                    .when(self.compact_padding, |el| el.py(px(3.)))
                    .text_size(px(14.0))
                    .when(spacious, |el| {
                        // One line of 16px text plus padding.
                        el.min_h(px(SPACIOUS_MIN_HEIGHT))
                            .px_4()
                            .py_4()
                            .text_size(px(16.0))
                    })
                    .when(self.trailing_inset > 0., |el| {
                        el.pr(px(self.trailing_inset))
                    })
                    .child(
                        div()
                            .id("prompt-editor")
                            .debug_selector(|| "prompt-editor".into())
                            .flex_1()
                            .min_w_0()
                            .max_h(editor_height)
                            .overflow_y_scroll()
                            .track_scroll(&self.editor_scroll)
                            .on_scroll_wheel(cx.listener(
                                |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                    if this.compact_padding {
                                        let delta = event.delta.pixel_delta(window.line_height()).y;
                                        let before = this.editor_scroll.offset().y - delta;
                                        let max = this.editor_scroll.max_offset().y;
                                        if (delta < px(0.) && before > -max)
                                            || (delta > px(0.) && before < px(0.))
                                        {
                                            cx.stop_propagation();
                                        }
                                    } else {
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .text_color(Theme::global().TEXT)
                            .child(
                                div()
                                    .relative()
                                    .child(TextElement { input: cx.entity() })
                                    .children(self.chip_positions.iter().filter_map(
                                        |(index, bounds)| {
                                            let attachment = self.attachments.get(*index)?;
                                            let index = *index;
                                            let chip = crate::image_chip::chip_with_label(
                                                match &attachment.payload {
                                                    AttachmentPayload::Image {
                                                        preview, ..
                                                    } => Some(preview.clone()),
                                                    AttachmentPayload::Text(_) | AttachmentPayload::Context(_) => None,
                                                    AttachmentPayload::File { image, .. }
                                                    | AttachmentPayload::Queued {
                                                        preview: image,
                                                        ..
                                                    } => image
                                                        .as_ref()
                                                        .map(|image| image.preview.clone()),
                                                },
                                                image_chip_numbers[index].map_or_else(
                                                    || crate::image_chip::label(&attachment.label),
                                                    crate::image_chip::numbered_label,
                                                ),
                                                "composer-image-chip",
                                                "composer-image-thumbnail",
                                                "composer-image-label",
                                            )
                                            .absolute()
                                            .left(bounds.origin.x)
                                            .top(bounds.origin.y)
                                            .w(bounds.size.width)
                                            .h(bounds.size.height);
                                            let chip = if matches!(
                                                attachment.payload,
                                                AttachmentPayload::Image { .. }
                                                    | AttachmentPayload::File {
                                                        image: Some(_),
                                                        ..
                                                    }
                                                    | AttachmentPayload::Text(_)
                                                    | AttachmentPayload::Context(context_picker::DraftContext::File { .. })
                                            ) {
                                                crate::keyboard::button(
                                                    chip.id(("composer-attachment", index)),
                                                )
                                                .on_click(cx.listener(
                                                    move |this, event, window, cx| {
                                                        if matches!(
                                                            event,
                                                            gpui::ClickEvent::Keyboard(_)
                                                        ) {
                                                            this.activate_attachment(
                                                                index, window, cx,
                                                            );
                                                        }
                                                    },
                                                ))
                                                .into_any_element()
                                            } else {
                                                chip.into_any_element()
                                            };
                                            Some(chip)
                                        },
                                    )),
                            ),
                    ),
            )
            .when(show_resize, |el| {
                el.child(self.render_editor_resize(window, cx))
            })
    }
}

impl Focusable for PromptInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn is_large_paste(text: &str) -> bool {
    text.split('\n').count() > 10 || text.encode_utf16().count() > 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowOptions};

    #[test]
    fn large_paste_threshold_matches_omp() {
        assert!(!is_large_paste(&"a".repeat(1000)));
        assert!(is_large_paste(&"a".repeat(1001)));
        assert!(!is_large_paste(&"a\n".repeat(9)));
        assert!(is_large_paste(&"a\n".repeat(10)));
        assert!(is_large_paste(&"😀".repeat(501)));
    }

    #[gpui::test]
    fn long_paste_is_collapsed_restored_and_sent_in_full(cx: &mut TestAppContext) {
        let sent = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        let result = sent.clone();
        let text = "a line\n".repeat(12);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", move |text, _, _, _| {
                *result.borrow_mut() = Some(text)
            });
            input.insert_pasted_text(&text, window, cx);
            input
        });
        input.update(vcx, |input, cx| {
            assert_eq!(input.content.as_ref(), "[Pasted text, 13 lines]");
            let snapshot = input.snapshot();
            input.restore(snapshot, cx);
            assert_eq!(input.content.as_ref(), "[Pasted text, 13 lines]");
        });
        input.update_in(vcx, |input, window, cx| {
            input.dispatch_submission(
                SubmissionKind::Message,
                input.content.to_string(),
                false,
                window,
                cx,
            )
        });
        assert_eq!(sent.borrow().as_deref(), Some(text.as_str()));
    }

    #[gpui::test]
    fn clicking_pasted_text_chip_expands_the_original_text(cx: &mut TestAppContext) {
        let text = "line\n".repeat(12);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.insert_pasted_text(&text, window, cx);
            input
        });
        vcx.run_until_parked();
        let chip = vcx
            .debug_bounds("composer-image-chip")
            .expect("pasted text chip");
        vcx.simulate_click(chip.center(), Default::default());
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), text);
            assert!(input.attachments.is_empty());
        });
    }

    #[gpui::test]
    fn pasted_file_chip_sends_only_image_and_no_local_path(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("real picture.png");
        let bytes = include_bytes!("../../../assets/previews/image-preview.png");
        std::fs::write(&path, bytes).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = sent.clone();
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", move |text, images, _, _| {
                *result.borrow_mut() = Some((text, images))
            });
            input.attach_pasted_files(std::slice::from_ref(&path), window, cx);
            input
        });
        input.update(vcx, |input, cx| {
            assert!(input.content.contains("real picture.png"));
            assert!(input.content.contains(&format!("{} B", bytes.len())));
            let snapshot = input.snapshot();
            input.restore(snapshot, cx);
        });
        input.update_in(vcx, |input, window, cx| {
            input.dispatch_submission(
                SubmissionKind::Message,
                input.content.to_string(),
                false,
                window,
                cx,
            )
        });
        let sent = sent.borrow();
        let (text, images) = sent.as_ref().unwrap();
        assert_eq!(text.trim(), "[#image 1]");
        assert_eq!(images.len(), 1);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&images[0].1)
                .unwrap(),
            bytes
        );
    }

    #[gpui::test]
    fn recalled_file_keeps_captured_bytes(cx: &mut TestAppContext) {
        let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = sent.clone();
        let file = SubmittedFile {
            name: "notes.txt".into(),
            mime: "text/plain".into(),
            size: 8,
            data: base64::engine::general_purpose::STANDARD.encode(b"original"),
        };
        let expected = file.clone();
        let (input, vcx) = cx.add_window_view(|_, cx| {
            let mut input =
                PromptInput::new_with_queue_files(cx, "test", move |_, text, _, files, _, _, _| {
                    *result.borrow_mut() = Some((text, files))
                });
            input.recall_prompt("/tmp/notes.txt", Vec::new(), cx);
            input.append_queued_files(vec![file], cx);
            input
        });
        input.update_in(vcx, |input, window, cx| {
            input.dispatch_submission(
                SubmissionKind::Message,
                input.content.to_string(),
                false,
                window,
                cx,
            )
        });
        let files = sent.borrow();
        let (text, files) = files.as_ref().unwrap();
        assert_eq!(text.trim(), "/tmp/notes.txt");
        let actual = &files[0];
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.data, expected.data);
    }

    /// Pasting a file from the file manager points at it. A zip, a folder or
    /// a 3 GB dump is named by path and never read or uploaded.
    #[gpui::test]
    fn pasted_non_image_files_are_referenced_by_path(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("build output.zip");
        let file = std::fs::File::create(&archive).unwrap();
        file.set_len(11 * 1024 * 1024).unwrap();
        let folder = dir.path().join("project");
        std::fs::create_dir(&folder).unwrap();
        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, b"original").unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = sent.clone();
        let pasted = [archive.clone(), folder.clone(), notes.clone()];
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new_with_queue_files(
                cx,
                "test",
                move |_, text, images, files, _, _, _| {
                    *result.borrow_mut() = Some((text, images, files))
                },
            );
            input.attach_pasted_files(&pasted, window, cx);
            input
        });
        input.read_with(vcx, |input, _| {
            assert!(input.attachments.is_empty(), "no file may be read");
            assert_eq!(
                input.attachment_notice.as_deref(),
                Some("3 file paths added"),
                "a size or directory refusal must not appear"
            );
        });
        input.update_in(vcx, |input, window, cx| {
            input.dispatch_submission(
                SubmissionKind::Message,
                input.content.to_string(),
                false,
                window,
                cx,
            )
        });
        let sent = sent.borrow();
        let (text, images, files) = sent.as_ref().unwrap();
        assert_eq!(
            text.trim(),
            format!(
                "\"{}\" {} {}",
                archive.display(),
                folder.display(),
                notes.display()
            )
        );
        assert!(images.is_empty() && files.is_empty(), "nothing is uploaded");
    }

    #[gpui::test]
    fn pasted_file_and_clipboard_images_keep_their_submission_order(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("real picture.png");
        let bytes = include_bytes!("../../../assets/previews/image-preview.png");
        std::fs::write(&path, bytes).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = sent.clone();
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", move |text, images, _, _| {
                *result.borrow_mut() = Some((text, images))
            });
            input.attach_pasted_files(std::slice::from_ref(&path), window, cx);
            input.attach_image(
                crate::clipboard_image::from_bytes("image/png".into(), bytes.to_vec()).unwrap(),
                cx,
            );
            input
        });
        input.update_in(vcx, |input, window, cx| {
            input.dispatch_submission(
                SubmissionKind::Message,
                input.content.to_string(),
                false,
                window,
                cx,
            )
        });
        let sent = sent.borrow();
        let (text, images) = sent.as_ref().unwrap();
        assert!(text.contains("[#image 1]"));
        assert!(text.contains("[#image 2]"));
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn session_image_marker_is_a_chip() {
        let text = "before [#img_0123456789abcdef0123456789abcdef] after [#image 1]";
        let markers: Vec<_> = image_marker_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(
            markers,
            vec!["[#img_0123456789abcdef0123456789abcdef]", "[#image 1]"]
        );
    }

    #[gpui::test]
    fn image_paste_is_an_inline_chip_at_the_cursor_and_backspace_removes_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.set_content("before after".into(), cx);
            input.selected_range = 7..7;
            input.focus_handle.focus(window, cx);
            input
        });
        input.update(vcx, |input, cx| {
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            assert_eq!(input.content.as_ref(), "before [#image 1]after");
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-attachments").is_none());
        let chip = vcx.debug_bounds("composer-image-chip").expect("chip");
        let label = vcx.debug_bounds("composer-image-label").expect("label");
        let thumb = vcx
            .debug_bounds("composer-image-thumbnail")
            .expect("thumbnail");
        assert!(
            thumb.right() < label.left(),
            "thumbnail and label have a gap"
        );
        assert!(
            label.right() <= chip.right(),
            "label must fit inside chip: {label:?} {chip:?}"
        );
        vcx.simulate_keystrokes("backspace");
        vcx.run_until_parked();
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "before after");
            assert!(input.attachments.is_empty());
        });
    }

    #[gpui::test]
    fn composer_thumbnail_opens_its_image(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.set_content("before ".into(), cx);
            input.selected_range = 7..7;
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 1000,
                    height: 600,
                },
                cx,
            );
            input.focus_handle.focus(window, cx);
            input
        });
        vcx.run_until_parked();
        let thumbnail = vcx
            .debug_bounds("composer-image-thumbnail")
            .expect("thumbnail is painted");
        assert_eq!(thumbnail.size, size(px(14.), px(14.)));
        assert!(
            vcx.debug_bounds("prompt-editor")
                .unwrap()
                .contains(&thumbnail.center())
        );
        vcx.simulate_click(thumbnail.center(), Default::default());
        vcx.run_until_parked();
        assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_some()));
        assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
    }

    #[gpui::test]
    fn clicking_end_of_composer_chip_opens_preview(cx: &mut TestAppContext) {
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(vcx, |input, cx| {
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            )
        });
        vcx.run_until_parked();
        let chip = vcx.debug_bounds("composer-image-chip").unwrap();
        vcx.simulate_click(
            point(chip.right() - px(1.), chip.center().y),
            Default::default(),
        );
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
    }

    #[gpui::test]
    fn image_chip_opens_from_tab_and_escape_returns_to_editor(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            input.focus_handle.focus(window, cx);
            input
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| window.focus_next(cx));
        vcx.run_until_parked();
        assert!(!vcx.update(|window, cx| input.read(cx).focus_handle.is_focused(window)));
        let keystroke = gpui::Keystroke::parse("space").unwrap();
        vcx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        vcx.simulate_event(gpui::KeyUpEvent { keystroke });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("paste-image-preview-dialog").is_some());
        assert!(vcx.update(|window, cx| input.read(cx).preview_close_focus.is_focused(window)));
        vcx.simulate_keystrokes("tab");
        assert!(vcx.update(|window, cx| input.read(cx).preview_remove_focus.is_focused(window)));
        vcx.simulate_keystrokes("shift-tab");
        assert!(vcx.update(|window, cx| input.read(cx).preview_close_focus.is_focused(window)));
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("paste-image-preview-dialog").is_none());
        assert!(vcx.update(|window, cx| input.read(cx).focus_handle.is_focused(window)));
    }

    #[gpui::test]
    fn composer_image_chip_never_disappears_at_wrap_boundary(cx: &mut TestAppContext) {
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        let window = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(window, gpui::size(px(240.), px(400.)));
        for n in 1..45 {
            input.update(vcx, |input, cx| {
                input.attachments.clear();
                input.set_content(format!("{} ", "x".repeat(n)), cx);
                input.attach_image(
                    crate::clipboard_image::ClipboardImage {
                        media_type: "image/png".into(),
                        bytes: include_bytes!("../../../assets/previews/image-preview.png")
                            .to_vec(),
                        width: 640,
                        height: 480,
                    },
                    cx,
                );
            });
            vcx.run_until_parked();
            assert!(
                vcx.debug_bounds("composer-image-chip").is_some(),
                "prefix length {n}"
            );
        }
    }

    #[gpui::test]
    fn composer_image_modal_is_bounded_and_dismissible(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 1000,
                    height: 600,
                },
                cx,
            );
            input.attachment_preview = Some(paste_preview::Preview::new(0));
            input.focus_handle.focus(window, cx);
            input
        });
        let window = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(window, gpui::size(px(800.), px(600.)));
        vcx.run_until_parked();
        let modal = vcx.debug_bounds("paste-image-preview-dialog").unwrap();
        assert!(modal.size.width <= px(640.) && modal.size.height <= px(480.));
        assert!((modal.center().x - px(400.)).abs() < px(1.));
        assert!((modal.center().y - px(300.)).abs() < px(1.));
        vcx.simulate_click(point(px(5.), px(5.)), Default::default());
        vcx.run_until_parked();
        assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_none()));
        input.update(vcx, |input, cx| {
            input.attachment_preview = Some(paste_preview::Preview::new(0));
            cx.notify();
        });
        vcx.run_until_parked();
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(input.read_with(vcx, |input, _| input.attachment_preview.is_none()));
    }

    #[gpui::test]
    fn image_preview_remove_is_undoable(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            input.focus_handle.focus(window, cx);
            input
        });
        input.update(vcx, |input, cx| {
            input.attachment_preview = Some(paste_preview::Preview::new(0));
            cx.notify();
        });
        vcx.run_until_parked();
        let remove = vcx
            .debug_bounds("paste-image-preview-remove")
            .unwrap()
            .center();
        vcx.simulate_click(remove, Default::default());
        input.read_with(vcx, |input, _| {
            assert!(input.content.is_empty());
            assert!(input.attachments.is_empty());
        });
        vcx.simulate_keystrokes("cmd-z");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "[#image 1]");
            assert_eq!(input.attachments.len(), 1);
        });
    }

    #[gpui::test]
    fn image_history_never_replays_a_chip_without_its_image(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.set_content("describe ".into(), cx);
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            input.focus_handle.focus(window, cx);
            input
        });
        vcx.simulate_keystrokes("enter up");
        input.read_with(vcx, |input, _| {
            assert!(!input.content.contains("[#image"));
            assert!(input.attachments.is_empty());
        });
    }

    #[gpui::test]
    fn image_draft_round_trips_through_history_navigation(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.history.push("old prompt".into());
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: include_bytes!("../../../assets/previews/image-preview.png").to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            input.focus_handle.focus(window, cx);
            input
        });
        let draft = input.read_with(vcx, |input, _| input.snapshot());
        vcx.simulate_keystrokes("up");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "old prompt");
            assert!(input.attachments.is_empty());
        });
        let persisted = input.read_with(vcx, |input, _| input.snapshot());
        input.update(vcx, |input, cx| input.restore(persisted, cx));
        vcx.simulate_keystrokes("down");
        input.read_with(vcx, |input, _| {
            let restored = input.snapshot();
            assert_eq!(restored.content, draft.content);
            assert_eq!(restored.attachments, draft.attachments);
            assert_eq!(restored.selection_start, draft.selection_start);
            assert_eq!(restored.selection_end, draft.selection_end);
        });
    }

    #[gpui::test]
    fn image_chips_preview_undo_and_submit_in_text_order(cx: &mut TestAppContext) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let output = seen.clone();
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let input = PromptInput::new(cx, "test", move |text, images, _, _| {
                *output.lock().unwrap() = Some((text, images));
            });
            input.focus_handle.focus(window, cx);
            input
        });
        input.update(vcx, |input, cx| {
            let image = |byte| crate::clipboard_image::ClipboardImage {
                media_type: "image/png".into(),
                bytes: vec![byte],
                width: 1,
                height: 1,
            };
            input.set_session_image_base(9, cx);
            input.set_content("AB".into(), cx);
            input.selected_range = 2..2;
            input.attach_image(image(1), cx);
            input.selected_range = 0..0;
            input.attach_image(image(2), cx);
            assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)]);
            let draft = input.content.clone();
            input.open_model_menu(Vec::new(), cx);
            assert!(input.image_chip_numbers().is_empty());
            input.close_model_menu(cx);
            assert_eq!(input.content, draft);
            assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)]);
        });
        vcx.run_until_parked();
        let position = input.read_with(vcx, |input, _| {
            let bounds = input.last_bounds.unwrap();
            bounds.origin
                + input
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_index(4, input.last_line_height.unwrap())
                    .unwrap()
                + point(px(0.), input.last_line_height.unwrap() / 2.)
        });
        vcx.simulate_click(position, Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("paste-image-preview-content").is_some());
        let close = vcx
            .debug_bounds("paste-image-preview-close")
            .unwrap()
            .center();
        vcx.simulate_click(close, Default::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("paste-image-preview-content").is_none());
        input.update(vcx, |input, cx| {
            let end = input.chip_ranges()[1].1.end;
            input.selected_range = end..end;
            cx.notify();
        });
        vcx.simulate_keystrokes("backspace");
        input.read_with(vcx, |input, _| assert_eq!(input.image_chip_numbers(), [Some(10)]));
        vcx.simulate_keystrokes("cmd-z");
        input.read_with(vcx, |input, _| assert_eq!(input.image_chip_numbers(), [Some(11), Some(10)]));
        vcx.simulate_keystrokes("enter");
        let (text, images) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(text, "[#image 1]AB[#image 2]");
        assert_eq!(
            images,
            vec![
                ("image/png".into(), "Ag==".into()),
                ("image/png".into(), "AQ==".into())
            ]
        );
    }

    #[test]
    fn double_click_word_ranges_are_unicode_safe_and_exclude_adjacent_space() {
        for (text, offset, selected) in [
            ("hello world", 1, "hello"),
            ("hello world", 5, " "),
            ("hello world", 7, "world"),
            ("hello world", 11, "world"),
            ("hello, world", 5, ","),
            ("héllo 猫 🚀", 2, "héllo"),
            ("héllo 猫 🚀", 7, "猫"),
            ("héllo 猫 🚀", usize::MAX, "🚀"),
            ("", 0, ""),
        ] {
            assert_eq!(&text[clicked_word_range(text, offset)], selected);
        }
    }

    #[test]
    fn slash_palette_filters_commands_and_models() {
        let models = vec!["gpt-5.6-sol".into(), "claude-fable-5".into()];
        let commands = command_suggestions("/m", &models);
        assert_eq!(commands[0].value, "/model");
        assert!(commands.iter().any(|entry| entry.value == "/models"));

        let picker = command_suggestions("/model claude", &models);
        assert_eq!(picker.len(), 1);
        assert_eq!(picker[0].value, "/model claude-fable-5");
        assert!(command_suggestions("hello /model", &models).is_empty());

        let effort = command_suggestions("/effort x", &models);
        assert_eq!(effort[0].value, "/effort xhigh");

        let tui_commands = command_suggestions("/session", &models);
        assert!(tui_commands.iter().any(|entry| entry.value == "/sessions"));
        assert!(
            command_suggestions("/logical commits", &models)
                .iter()
                .any(|entry| entry.value == "/commit")
        );
    }

    #[test]
    fn slash_palette_keeps_every_command_reachable_by_scrolling() {
        let suggestions = command_suggestions("/", &[]);
        let registered = registered_command_entries().collect::<Vec<_>>();
        assert!(registered.len() > 12);
        assert_eq!(suggestions.len(), registered.len());
        for (suggestion, (command, _)) in suggestions.iter().zip(registered) {
            assert_eq!(suggestion.value, command);
        }
    }

    #[test]
    fn enter_accepts_highlighted_description_match() {
        let suggestions = command_suggestions("/logical commits", &[]);
        assert_eq!(
            accepted_command_submission(&suggestions, 0).as_deref(),
            Some("/commit")
        );
        let aliases = command_suggestions("/models", &[]);
        assert_eq!(
            accepted_command_submission(&aliases, 0).as_deref(),
            Some("/models")
        );
    }

    #[test]
    fn filtered_model_search_accepts_the_full_selected_model_id() {
        let models = vec!["gpt-5.6-sol".into(), "gpt-5.6-luna".into()];
        let suggestions = command_suggestions("/model luna", &models);

        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            accepted_command_submission(&suggestions, 0).as_deref(),
            Some("/model gpt-5.6-luna")
        );
    }

    fn input_window(cx: &mut TestAppContext) -> gpui::WindowHandle<PromptInput> {
        cx.update(bind_keys);
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}))
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();
        window
    }

    #[gpui::test]
    fn arrow_history_waits_for_the_edge_of_a_soft_wrapped_prompt(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let draft = "界 👩‍💻 Привет Jcode ".repeat(80);
        window
            .update(cx, |input, _, cx| {
                input.history.push("older prompt".into());
                input.set_content(draft.clone(), cx);
            })
            .unwrap();
        cx.run_until_parked();

        cx.simulate_keystrokes(*window, "up");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), draft.as_str());
                assert!(input.cursor_offset() < draft.len());
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "down");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), draft.as_str());
                assert_eq!(input.cursor_offset(), draft.len());
            })
            .unwrap();

        cx.simulate_keystrokes(*window, "home up");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "older prompt")
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(*window, "down");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), draft.as_str())
            })
            .unwrap();
    }

    #[gpui::test]
    fn arrow_navigation_keeps_its_column_across_short_and_empty_lines(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let draft = "abcdef\n\nx\nabcdef";
        window
            .update(cx, |input, _, cx| input.set_content(draft.into(), cx))
            .unwrap();
        cx.run_until_parked();

        for (key, expected) in [
            ("up", 9),
            ("up", 7),
            ("up", 6),
            ("down", 7),
            ("down", 9),
            ("down", draft.len()),
        ] {
            cx.simulate_keystrokes(*window, key);
            window
                .update(cx, |input, _, _| {
                    assert_eq!(input.cursor_offset(), expected, "{key}")
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn arrow_navigation_uses_reflowed_rows_after_resize(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let draft = "word word ".repeat(12);
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.history.push("old prompt".into());
            input.set_content(draft.clone(), cx);
            window.focus(&input.focus_handle, cx);
            input
        });
        vcx.simulate_resize(size(px(180.), px(500.)));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("home down");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), draft.as_str());
            assert!((1..draft.len()).contains(&input.cursor_offset()));
        });
        vcx.simulate_keystrokes("up");
        input.read_with(vcx, |input, _| assert_eq!(input.cursor_offset(), 0));

        vcx.simulate_resize(size(px(1600.), px(500.)));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("end up");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "old prompt")
        });
    }

    #[gpui::test]
    fn arrow_navigation_does_not_split_image_or_file_chips(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-file.png");
        let png = include_bytes!("../../../assets/previews/image-preview.png");
        std::fs::write(&path, png).unwrap();
        let (input, vcx) = cx.add_window_view(|window, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {});
            input.attach_image(
                crate::clipboard_image::ClipboardImage {
                    media_type: "image/png".into(),
                    bytes: png.to_vec(),
                    width: 640,
                    height: 480,
                },
                cx,
            );
            input.replace_text_in_range(None, "\n", window, cx);
            input.attach_pasted_files(&[path], window, cx);
            input.replace_text_in_range(None, "\nabc", window, cx);
            window.focus(&input.focus_handle, cx);
            input
        });
        vcx.run_until_parked();
        let before = input.read_with(vcx, |input, _| input.snapshot());
        for label in ["test-file.png", "[#image"] {
            vcx.simulate_keystrokes("up");
            input.read_with(vcx, |input, _| {
                let (_, chip) = input
                    .chip_ranges()
                    .into_iter()
                    .find(|(_, range)| input.content[range.clone()].contains(label))
                    .unwrap();
                assert!([chip.start, chip.end].contains(&input.cursor_offset()));
            });
        }
        vcx.simulate_keystrokes("down down");
        assert_eq!(input.read_with(vcx, |input, _| input.snapshot()), before);
    }

    #[gpui::test]
    fn composer_double_click_selects_a_word_and_preserves_shift_extension(cx: &mut TestAppContext) {
        let handle = input_window(cx);
        handle
            .update(cx, |input, _, cx| {
                input.set_content("hello world".into(), cx)
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |input, window, cx| {
                let origin = input.last_bounds.unwrap().origin;
                let position = origin
                    + input
                        .last_layout
                        .as_ref()
                        .unwrap()
                        .position_for_index(2, window.line_height())
                        .unwrap();
                let event = MouseDownEvent {
                    button: MouseButton::Left,
                    position: position + point(px(0.), window.line_height() / 2.),
                    modifiers: Default::default(),
                    click_count: 2,
                    first_mouse: false,
                };
                input.on_mouse_down(&event, window, cx);
                assert_eq!(input.selected_range, 0..5);
                assert!(input.focus_handle.is_focused(window));
                let mut shift_event = event;
                shift_event.modifiers.shift = true;
                input.on_mouse_down(&shift_event, window, cx);
                assert_eq!(input.selected_range, 0..2);
            })
            .unwrap();
    }

    /// Mouse handlers run outside paint, where `window.line_height()` reflects
    /// the window default rather than the composer's 14px text style. The hit
    /// test must use the row height the layout was actually painted with.
    #[gpui::test]
    fn composer_click_on_each_line_places_caret_on_that_line(cx: &mut TestAppContext) {
        let handle = input_window(cx);
        handle
            .update(cx, |input, _, cx| {
                input.set_content("first\nsecond\nthird".into(), cx)
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |input, window, cx| {
                let bounds = input.last_bounds.unwrap();
                assert_eq!(input.visual_line_count, 3);
                let row_height = bounds.size.height / 3.;
                for (row, line_start) in [(0, 0), (1, 6), (2, 13)] {
                    // Just inside the top edge: a taller handler-time line
                    // height would map this to the row above.
                    let position = bounds.origin + point(px(1.), row_height * row as f32 + px(1.));
                    let event = MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    };
                    input.on_mouse_down(&event, window, cx);
                    assert_eq!(
                        input.selected_range,
                        line_start..line_start,
                        "click at the top of row {row}"
                    );
                    input.on_mouse_up(
                        &MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                        },
                        window,
                        cx,
                    );
                }
            })
            .unwrap();
    }

    #[gpui::test]
    fn composer_ime_bounds_follow_preedit_without_paint(cx: &mut TestAppContext) {
        let handle = input_window(cx);
        handle
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "测😀", window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |input, window, cx| {
                let bounds = input.last_bounds.unwrap();
                let anchor = input.bounds_for_range(3..3, bounds, window, cx).unwrap();
                let mut widths = Vec::new();
                // macOS queries the rect synchronously within the same frame.
                for marked in ["q", "qishi", "q"] {
                    let end = marked.len();
                    input.replace_and_mark_text_in_range(None, marked, Some(end..end), window, cx);
                    let rect = input
                        .bounds_for_range(3..3 + end, bounds, window, cx)
                        .expect("preedit must have bounds before the next paint");
                    assert_eq!(rect.origin, anchor.origin);
                    assert_eq!(rect.size.height, anchor.size.height);
                    widths.push(rect.size.width);
                }
                assert!(widths[0] > px(0.));
                assert!(widths[1] > widths[0]);
                assert_eq!(widths[2], widths[0]);

                let marked = "界\n中文";
                input.replace_and_mark_text_in_range(None, marked, None, window, cx);
                let first_line = input.bounds_for_range(3..6, bounds, window, cx).unwrap();
                assert_eq!(
                    first_line,
                    input.bounds_for_range(3..4, bounds, window, cx).unwrap()
                );
                let second_line = input.bounds_for_range(5..6, bounds, window, cx).unwrap();
                assert_eq!(second_line.top(), first_line.bottom());
                input.replace_text_in_range(None, "其实", window, cx);
                assert_eq!(input.content.as_ref(), "测😀其实");
            })
            .unwrap();
    }

    #[gpui::test]
    fn pinyin_composition_after_cjk_text_does_not_panic(cx: &mut TestAppContext) {
        // Issue #6: macOS Pinyin marks "ni", "ni h", "ni hao" with a caret at
        // the end of the composition, then commits the candidate. Earlier CJK
        // text made the selection math slice inside a character and abort.
        let window = input_window(cx);
        window
            .update(cx, |input, window, cx| {
                input.replace_text_in_range(None, "你好，", window, cx);
                for marked in ["n", "ni", "ni h", "ni hao"] {
                    let caret = marked.encode_utf16().count();
                    input.replace_and_mark_text_in_range(
                        None,
                        marked,
                        Some(caret..caret),
                        window,
                        cx,
                    );
                    assert_eq!(input.content.as_ref(), format!("你好，{marked}"));
                    let start = "你好，".len();
                    assert_eq!(input.marked_range, Some(start..start + marked.len()));
                    assert_eq!(
                        input.selected_range,
                        start + marked.len()..start + marked.len()
                    );
                    assert_eq!(
                        input.marked_text_range(window, cx),
                        Some(3..3 + marked.encode_utf16().count())
                    );
                }
                // A CJK composition with an out-of-range selection is clamped.
                input.replace_and_mark_text_in_range(None, "你好", Some(1..9), window, cx);
                assert_eq!(input.content.as_ref(), "你好，你好");
                let start = "你好，".len();
                assert_eq!(input.selected_range, start + 3..start + 6);
                input.replace_text_in_range(None, "你好🚀", window, cx);
                assert_eq!(input.content.as_ref(), "你好，你好🚀");
                assert!(input.marked_range.is_none());
                assert_eq!(
                    input.selected_range,
                    input.content.len()..input.content.len()
                );

                // Stale marked text and ranges that split characters or run
                // past the end are clamped, never sliced blindly.
                input.marked_range = Some(1..400);
                input.replace_and_mark_text_in_range(None, "zh", Some(2..2), window, cx);
                assert_eq!(input.content.as_ref(), "zh");
                input.replace_text_in_range(Some(1..1000), "中", window, cx);
                assert_eq!(input.content.as_ref(), "z中");
                // Deliberately inverted: the handler must clamp the range
                // instead of slicing blindly, which is the case under test.
                #[allow(clippy::reversed_empty_ranges)]
                let inverted = 5..2;
                input.replace_text_in_range(Some(inverted), "文", window, cx);
                assert_eq!(input.content.as_ref(), "z中文");
                let mut actual = None;
                assert_eq!(
                    input.text_for_range(1..99, &mut actual, window, cx),
                    Some("中文".into())
                );
                assert_eq!(actual, Some(1..3));
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// `[dictation] mode = "insert"`: the words arrive at the caret, not after
    /// a newline, and one undo removes the whole insertion.
    #[gpui::test]
    fn dictation_inserts_at_the_caret_as_one_undo_step(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        for (draft, caret, spoken, expected, caret_after) in [
            ("", 0, "run the suite", "run the suite", 13),
            ("fix the test", 4, "flaky ", "fix flaky the test", 10),
            ("fix the test", 12, "now", "fix the test now", 16),
            ("keep", 0, "please ", "please keep", 7),
            ("один два", 9, "три", "один три два", 16),
        ] {
            input.update(cx, |input, cx| {
                input.set_content(draft.into(), cx);
                input.selected_range = caret..caret;
                input.undo.clear();
                input.redo = vec![EditSnapshot {
                    content: "old redo".into(),
                    attachments: vec![],
                }];
                input.insert_dictation(spoken, cx);
                assert_eq!(input.content.as_ref(), expected);
                assert_eq!(input.selected_range, caret_after..caret_after);
                assert_eq!(
                    input
                        .undo
                        .iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<_>>(),
                    vec![draft.to_owned()],
                    "one undo step"
                );
                assert!(input.redo.is_empty());
                assert!(input.marked_range.is_none());
                assert!(input.history_index.is_none());
            });
        }
    }

    /// A selection is replaced the way typing over it would, so dictation into
    /// a highlighted word does not duplicate it.
    #[gpui::test]
    fn dictation_insert_replaces_the_selection(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, cx| {
            input.set_content("keep drop tail".into(), cx);
            input.selected_range = 5..9;
            input.insert_dictation("spare", cx);
            assert_eq!(input.content.as_ref(), "keep spare tail");
            assert_eq!(input.selected_range, 10..10);
        });
    }

    #[gpui::test]
    fn dictation_appends_newline_without_replacing_existing_draft(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        for (before, expected) in [
            ("", "spoken words"),
            ("typed", "typed\nspoken words"),
            ("typed ", "typed \nspoken words"),
            ("typed\t", "typed\t\nspoken words"),
            ("typed\n", "typed\nspoken words"),
            ("typed\n\n", "typed\n\nspoken words"),
            ("你好 🚀", "你好 🚀\nspoken words"),
        ] {
            input.update(cx, |input, cx| {
                input.set_content(before.into(), cx);
                input.undo.clear();
                input.redo = vec![EditSnapshot {
                    content: "old redo".into(),
                    attachments: vec![],
                }];
                input.selected_range = 0..input.content.len();
                input.marked_range = Some(0..input.content.len());
                input.history_index = Some(0);
                input.append_dictation("  spoken words\n", cx);
                assert_eq!(input.content.as_ref(), expected);
                assert_eq!(
                    input
                        .undo
                        .iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<_>>(),
                    vec![before.to_owned()]
                );
                assert!(input.redo.is_empty());
                assert_eq!(input.selected_range, expected.len()..expected.len());
                assert!(input.marked_range.is_none());
                assert!(input.history_index.is_none());
            });
        }
    }

    /// `[dictation] mode = "replace"` throws a draft away, so it must be one
    /// undoable edit: the discarded draft comes back with a single undo, and a
    /// stale redo from before the replacement is cleared.
    #[gpui::test]
    fn dictation_replace_is_one_undoable_edit(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        for draft in ["", "half typed", "多行\n草稿", "  keep spacing  "] {
            input.update(cx, |input, cx| {
                input.set_content(draft.into(), cx);
                input.selected_range = 0..input.content.len();
                input.marked_range = Some(0..2);
                input.history_index = Some(0);
                input.undo.clear();
                input.redo = vec![EditSnapshot {
                    content: "stale redo".into(),
                    attachments: vec![],
                }];
                input.replace_dictation("  spoken words  ", cx);
                assert_eq!(input.content.as_ref(), "spoken words");
                assert_eq!(
                    input
                        .undo
                        .iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<_>>(),
                    vec![draft.to_owned()],
                    "one undo step"
                );
                assert!(input.redo.is_empty(), "a replacement clears redo");
                assert_eq!(input.selected_range, 12..12);
                assert!(input.marked_range.is_none());
                assert!(input.history_index.is_none());
            });
        }
    }

    /// An empty utterance never wipes the draft and never touches undo history.
    #[gpui::test]
    fn empty_replace_dictation_keeps_the_draft(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, cx| {
            input.set_content("keep me".into(), cx);
            input.undo = vec![EditSnapshot {
                content: "previous".into(),
                attachments: vec![],
            }];
            input.redo = vec![EditSnapshot {
                content: "next".into(),
                attachments: vec![],
            }];
            input.selected_range = 0..3;
            input.replace_dictation("   ", cx);
            assert_eq!(input.content.as_ref(), "keep me");
            assert_eq!(input.selected_range, 0..3);
            assert_eq!(
                input
                    .undo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec!["previous"]
            );
            assert_eq!(
                input
                    .redo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec!["next"]
            );
        });
    }

    #[gpui::test]
    fn ghost_does_not_start_real_requests_in_gpui_tests(cx: &mut TestAppContext) {
        let window = input_window(cx);
        window
            .update(cx, |input, _, _| {
                input.ghost = Some(GhostState {
                    machine: Default::default(),
                    context: Box::new(|_| Vec::new()),
                    task: None,
                    disabled: false,
                    cache: Default::default(),
                    request_counter: None,
                });
            })
            .unwrap();
        cx.simulate_input(*window, "a realistic draft");
        window
            .update(cx, |input, _, _| {
                assert!(input.ghost.as_ref().unwrap().task.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn ghost_burst_sends_once_and_programmatic_edits_send_nothing(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        let test_counter = counter.clone();
        window
            .update(cx, |input, _, _| {
                input.ghost = Some(GhostState {
                    machine: Default::default(),
                    context: Box::new(|_| Vec::new()),
                    task: None,
                    disabled: false,
                    cache: Default::default(),
                    request_counter: Some(test_counter),
                });
            })
            .unwrap();
        cx.simulate_input(*window, "abcd");
        for _ in 0..5 {
            cx.executor().advance_clock(Duration::from_millis(300));
            cx.simulate_input(*window, "e");
        }
        assert_eq!(counter.get(), 0);
        cx.executor().advance_clock(Duration::from_millis(401));
        cx.run_until_parked();
        assert_eq!(counter.get(), 1);

        window
            .update(cx, |input, window, cx| {
                input.set_content("restored draft".into(), cx);
                input.insert_dictation("dictated words", cx);
                input.insert_pasted_text(" pasted contents", window, cx);
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(counter.get(), 1);
    }

    #[gpui::test]
    fn ghost_identical_request_uses_cache_without_network(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        let test_counter = counter.clone();
        window
            .update(cx, |input, _, _| {
                let request = crate::prompt_completion::Request {
                    draft: "abcd".into(),
                    context: vec![],
                    cwd: None,
                    project: None,
                    model: crate::config::get().completions.model.clone(),
                };
                input.ghost = Some(GhostState {
                    machine: Default::default(),
                    context: Box::new(|_| Vec::new()),
                    task: None,
                    disabled: false,
                    cache: [(serde_json::to_string(&request).unwrap(), " cached".into())].into(),
                    request_counter: Some(test_counter),
                });
            })
            .unwrap();
        cx.simulate_input(*window, "abcd");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.ghost_text(), Some(" cached"))
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(counter.get(), 0);
    }

    /// Simulate a completion arriving for the current draft, bypassing network.
    fn show_ghost(input: &mut PromptInput, suffix: &str) {
        let draft = input.content.to_string();
        let ghost = input.ghost.get_or_insert_with(|| GhostState {
            machine: Default::default(),
            context: Box::new(|_| Vec::new()),
            task: None,
            disabled: true,
            cache: Default::default(),
            request_counter: None,
        });
        let generation = ghost.machine.on_edit(&draft, true).unwrap();
        assert!(ghost.machine.on_response(generation, suffix));
    }

    #[gpui::test]
    fn typing_keeps_composer_focus_across_ghost_and_snapshot_frames(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let initial_focus = window
            .update(cx, |input, _, _| input.focus_handle.clone())
            .unwrap();
        for (index, letter) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            cx.simulate_input(*window, letter);
            window
                .update(cx, |input, window, cx| {
                    assert_eq!(input.focus_handle, initial_focus);
                    assert!(initial_focus.is_focused(window));
                    assert_eq!(input.snapshot().content, "abcde"[..=index]);
                    if index >= 3 {
                        show_ghost(input, " suggestion");
                    }
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |input, window, _| {
                    assert_eq!(input.focus_handle, initial_focus);
                    assert!(initial_focus.is_focused(window));
                })
                .unwrap();
        }
        window
            .update(cx, |input, _, _| assert_eq!(&*input.content, "abcde"))
            .unwrap();
    }

    #[gpui::test]
    fn slash_draft_hides_existing_ghost_and_does_not_request_completion(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "previous draft");
        window
            .update(cx, |input, _, _| show_ghost(input, " old suggestion"))
            .unwrap();
        window
            .update(cx, |input, _, cx| input.set_content(String::new(), cx))
            .unwrap();
        cx.simulate_input(*window, "/effort high");
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.ghost_text(), None);
                assert!(input.ghost.as_ref().unwrap().task.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn ghost_completion_tab_accepts_escape_dismisses(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "исправь баг");
        window
            .update(cx, |input, _, _| show_ghost(input, " в композере"))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.ghost_text(), Some(" в композере"))
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "tab");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "исправь баг в композере");
                assert_eq!(input.ghost_text(), None);
            })
            .unwrap();

        // Esc hides the ghost but keeps the draft (a second Esc clears it).
        window
            .update(cx, |input, _, _| show_ghost(input, " now"))
            .unwrap();
        cx.simulate_keystrokes(*window, "escape");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "исправь баг в композере");
                assert_eq!(input.ghost_text(), None);
            })
            .unwrap();

        // Typing a diverging character dismisses; moving the caret hides it.
        window
            .update(cx, |input, _, _| show_ghost(input, " now"))
            .unwrap();
        cx.simulate_input(*window, "!");
        window
            .update(cx, |input, _, _| assert_eq!(input.ghost_text(), None))
            .unwrap();
        window
            .update(cx, |input, _, _| show_ghost(input, " now"))
            .unwrap();
        cx.simulate_keystrokes(*window, "left");
        window
            .update(cx, |input, _, _| assert_eq!(input.ghost_text(), None))
            .unwrap();
    }

    #[gpui::test]
    fn tab_completes_the_selected_slash_command(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "/eff");
        cx.simulate_keystrokes(*window, "tab");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "/effort")
            })
            .unwrap();

        window
            .update(cx, |input, _, cx| input.set_content(String::new(), cx))
            .unwrap();
        cx.simulate_input(*window, "/re");
        let expected: Vec<String> = window
            .update(cx, |input, _, _| {
                input
                    .command_suggestions()
                    .iter()
                    .take(2)
                    .map(|row| row.value.clone())
                    .collect()
            })
            .unwrap();
        assert_eq!(expected.len(), 2);
        cx.simulate_keystrokes(*window, "tab");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), expected[0])
            })
            .unwrap();
        window
            .update(cx, |input, _, _| {
                assert!(input.command_suggestions().is_empty())
            })
            .unwrap();
    }

    #[gpui::test]
    fn escape_hides_slash_menu_before_overlay_cancel(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let cancels = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = cancels.clone();
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|cx| {
                    PromptInput::new(cx, "test", |_, _, _, _| {}).with_on_overlay_cancel(
                        move |_| {
                            seen.set(seen.get() + 1);
                            true
                        },
                    )
                })
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();
        cx.simulate_input(*window, "/swarm");
        window
            .update(cx, |input, _, _| {
                assert!(!input.command_suggestions().is_empty())
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "escape");
        window
            .update(cx, |input, _, _| {
                assert!(input.command_suggestions().is_empty(), "menu hidden");
                assert_eq!(input.content.as_ref(), "/swarm", "draft kept");
            })
            .unwrap();
        assert_eq!(cancels.get(), 0, "first Esc must not stop the agent");
        cx.simulate_keystrokes(*window, "escape");
        assert_eq!(cancels.get(), 1, "with no menu, Esc stops as before");
    }

    #[gpui::test]
    fn tab_accepts_highlighted_exact_command_without_sending(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "/help");
        window
            .update(cx, |input, _, _| {
                assert!(!input.command_suggestions().is_empty())
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "tab");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "/help");
                assert!(input.history.is_empty());
                assert!(input.command_suggestions().is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn registered_slash_commands_submit_or_keep_the_draft_with_feedback(cx: &mut TestAppContext) {
        let window = input_window(cx);
        for command in crate::commands::REGISTERED_COMMANDS {
            let model_step_visible = |input: &PromptInput| {
                command.name == "/model"
                    && (input.model_stage != model_stage::ModelStage::Models
                        || !input.expanded_model_groups.is_empty())
                    && !input.command_suggestions().is_empty()
            };
            window
                .update(cx, |input, _, cx| {
                    input.history.clear();
                    input.transient_notice = None;
                    input.model_stage = Default::default();
                    input.expanded_model_groups.clear();
                    input.set_content(command.name.into(), cx);
                })
                .unwrap();
            // Enter may first accept the visible completion and leave it in
            // the editor. A second Enter must not make it silently disappear.
            let mut confirmed = command.name.to_owned();
            for _ in 0..2 {
                confirmed = window
                    .update(cx, |input, _, _| input.content.to_string())
                    .unwrap();
                cx.simulate_keystrokes(*window, "enter");
                if window
                    .update(cx, |input, _, _| {
                        input.content.is_empty()
                            || input.transient_notice.is_some()
                            || model_step_visible(input)
                    })
                    .unwrap()
                {
                    break;
                }
            }
            window
                .update(cx, |input, _, _| {
                    assert!(
                        input.history.last().is_some_and(|entry| entry == confirmed.trim() || entry.strip_prefix(confirmed.trim()).is_some_and(|argument| argument.starts_with(' ')))
                            || (!input.content.is_empty() && input.transient_notice.is_some())
                            || model_step_visible(input),
                        "{} was neither submitted nor retained with feedback: draft={:?}, history={:?}, notice={:?}, stage={:?}, expanded={:?}",
                        command.name, input.content, input.history, input.transient_notice,
                        input.model_stage, input.expanded_model_groups,
                    );
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn tab_without_ghost_leaves_draft_alone(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "hello");
        cx.simulate_keystrokes(*window, "tab");
        window
            // Tab propagates to its pre-existing handling (plain text input).
            .update(cx, |input, _, _| {
                assert_eq!(input.content.trim_end(), "hello")
            })
            .unwrap();
    }

    #[gpui::test]
    fn dictation_is_one_undoable_edit_and_preserves_attachments(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "typed");
        window
            .update(cx, |input, _, cx| {
                input.attachments.push(Attachment {
                    label: "4×3".into(),
                    payload: AttachmentPayload::Image {
                        media_type: "image/png".into(),
                        encoded: "cG5n".into(),
                        preview: preview_image("image/png", Vec::new()).unwrap(),
                    },
                });
                let previous_edits = input.undo.len();
                input.append_dictation("first line\nsecond line", cx);
                assert_eq!(input.undo.len(), previous_edits + 1);
                assert_eq!(input.content.as_ref(), "typed\nfirst line\nsecond line");
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "ctrl-z");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "typed");
                assert_eq!(input.attachments.len(), 1);
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "ctrl-shift-z");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "typed\nfirst line\nsecond line");
                assert!(matches!(&input.attachments[0].payload, AttachmentPayload::Image { encoded, .. } if encoded.as_ref() == "cG5n"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn model_menu_restores_draft_and_images_after_selection_or_cancel(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, cx| {
            input.set_content("unfinished prompt".into(), cx);
            input.attachments.push(Attachment {
                label: "4×3".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
            for selected in [false, true] {
                input.open_model_menu(Vec::new(), cx);
                input.open_model_menu(Vec::new(), cx); // Reopening must not replace the saved draft.
                assert_eq!(input.content.as_ref(), "/model ");
                assert!(input.attachments.is_empty());
                let snapshot = input.snapshot();
                assert_eq!(snapshot.content, "unfinished prompt");
                assert_eq!(snapshot.attachments.len(), 1);
                if selected {
                    input.set_content(String::new(), cx);
                }
                input.close_model_menu(cx);
                assert_eq!(input.content.as_ref(), "unfinished prompt");
                assert_eq!(input.attachments.len(), 1);
                assert!(matches!(&input.attachments[0].payload, AttachmentPayload::Image { encoded, .. } if encoded.as_ref() == "cG5n"));
            }
        });
    }

    #[gpui::test]
    fn empty_dictation_and_partials_leave_content_and_undo_untouched(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, cx| {
            input.set_content("keep this ".into(), cx);
            input.selected_range = 0..4;
            input.undo = vec![EditSnapshot {
                content: "previous".into(),
                attachments: vec![],
            }];
            input.redo = vec![EditSnapshot {
                content: "next".into(),
                attachments: vec![],
            }];
            input.append_dictation(" \n\t", cx);
            for preview in [Some(String::new()), Some("partial words".into()), None] {
                input.set_voice_preview(preview.clone(), cx);
                assert_eq!(input.voice_preview, preview);
                assert_eq!(input.content.as_ref(), "keep this ");
                assert_eq!(input.selected_range, 0..4);
                assert_eq!(
                    input
                        .undo
                        .iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<_>>(),
                    vec!["previous"]
                );
                assert_eq!(
                    input
                        .redo
                        .iter()
                        .map(|s| s.content.as_str())
                        .collect::<Vec<_>>(),
                    vec!["next"]
                );
            }
        });
    }

    #[gpui::test]
    fn voice_bounds_requires_a_valid_measurement(cx: &mut TestAppContext) {
        let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(cx, |input, _| {
            assert!(input.voice_bounds().is_none());
            let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(100.0), px(30.0)));
            input.last_bounds = Some(bounds);
            assert_eq!(input.voice_bounds(), Some(bounds));
            input.last_bounds = Some(Bounds::new(bounds.origin, size(px(0.0), px(30.0))));
            assert!(input.voice_bounds().is_none());
            input.last_bounds = Some(Bounds::new(point(px(f32::NAN), px(0.0)), bounds.size));
            assert!(input.voice_bounds().is_none());
        });
    }

    #[gpui::test]
    fn voice_preview_never_adds_lines_to_composer(cx: &mut TestAppContext) {
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        input.update(vcx, |input, cx| {
            input.spacious = true;
            input.set_voice_preview(Some(String::new()), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
        let composer = vcx.debug_bounds("prompt-input").unwrap();
        input.read_with(vcx, |input, _| {
            assert_eq!(input.voice_bounds(), Some(composer));
        });
        for lines in [30, 60] {
            input.update(vcx, |input, cx| {
                input.set_voice_preview(Some("recognized words\n".repeat(lines)), cx);
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("voice-live-transcript").is_none());
            input.read_with(vcx, |input, _| {
                assert!(input.content.is_empty());
                assert!(input.undo.is_empty());
            });
        }
        input.update(vcx, |input, cx| input.set_voice_preview(None, cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
    }

    #[gpui::test]
    fn model_groups_expand_with_enter_without_submitting_and_preserve_exact_routes(
        cx: &mut TestAppContext,
    ) {
        let window = input_window(cx);
        let routes: Vec<_> = (1..=5)
            .map(|n| jcode_sdk::ModelRouteInfo {
                speed_options: Vec::new(),
                reasoning_efforts: None,
                model: format!("atlas-{n}"),
                provider: "OpenAI".into(),
                api_method: "openai-api-key".into(),
                available: true,
                detail: String::new(),
                usage: None,
            })
            .collect();
        window
            .update(cx, |input, _, cx| {
                input.set_model_routes(Vec::new(), &routes, None, cx);
                input.set_content("/model".into(), cx);
                assert_eq!(input.model_picker_rows().len(), 3);
                input.command_selection = 3;
                assert!(accepted_command_submission(&input.command_suggestions(), 3).is_none());
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "enter");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "/model");
                assert!(input.history.is_empty());
                assert_eq!(input.model_picker_rows().len(), 5);
                assert_eq!(input.command_selection, 5);
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "enter");
        window
            .update(cx, |input, _, cx| {
                assert_eq!(input.model_picker_rows().len(), 3);
                assert_eq!(input.command_selection, 3);
                input.toggle_model_group("OpenAI · openai-api-key".into(), cx);
                input.set_content("/model atlas-5".into(), cx);
                assert!(!input.expanded_model_groups.is_empty());
                assert_eq!(
                    input.command_suggestions()[0].value,
                    "/model openai-api:atlas-5"
                );
            })
            .unwrap();
        // One Enter switches. No level is in force here, so none is forced.
        cx.simulate_keystrokes(*window, "enter");
        window
            .update(cx, |input, _, _| {
                assert_eq!(
                    input.history.last().map(String::as_str),
                    Some("/model openai-api:atlas-5")
                );
                assert!(input.content.is_empty());
                assert!(input.expanded_model_groups.is_empty());
            })
            .unwrap();
        window
            .update(cx, |input, _, cx| {
                input.set_content("/model".into(), cx);
                input.toggle_model_group("OpenAI · openai-api-key".into(), cx);
                input.set_content(String::new(), cx);
                input.set_content("/model".into(), cx);
                assert_eq!(input.model_picker_rows().len(), 3);
            })
            .unwrap();
    }

    #[gpui::test]
    fn model_group_refresh_reveals_selected_choice_if_it_falls_below_top_three(
        cx: &mut TestAppContext,
    ) {
        let window = input_window(cx);
        let routes = |favored: &str| {
            (1..=5)
                .map(|n| {
                    let model = format!("atlas-{n}");
                    jcode_sdk::ModelRouteInfo {
                        speed_options: Vec::new(),
                        reasoning_efforts: None,
                        usage: Some(jcode_sdk::ModelUsage {
                            count: if model == favored { 99 } else { n },
                            ..Default::default()
                        }),
                        model,
                        provider: "OpenAI".into(),
                        api_method: "openai-oauth".into(),
                        available: true,
                        detail: String::new(),
                    }
                })
                .collect::<Vec<_>>()
        };
        window
            .update(cx, |input, _, cx| {
                // The current route is pinned first, then newest releases.
                input.set_model_routes(Vec::new(), &routes("atlas-1"), Some("atlas-1".into()), cx);
                input.set_content("/model".into(), cx);
                assert_eq!(
                    input.command_suggestions()[0].value,
                    "/model openai-oauth:atlas-1"
                );
                // The catalog changes and atlas-1 is no longer current, so it
                // drops out of the top three. The highlight must follow it.
                input.set_model_routes(Vec::new(), &routes("atlas-5"), Some("atlas-5".into()), cx);
                assert_eq!(
                    input.command_suggestions()[input.command_selection].value,
                    "/model openai-oauth:atlas-1"
                );
                assert_eq!(input.model_picker_rows().len(), 5);
            })
            .unwrap();
    }

    #[gpui::test]
    fn model_usage_ranking_preserves_the_highlighted_choice_on_refresh(cx: &mut TestAppContext) {
        let window = input_window(cx);
        let route = |model: &str, count| jcode_sdk::ModelRouteInfo {
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: model.into(),
            provider: "openai".into(),
            api_method: "oauth".into(),
            available: true,
            detail: String::new(),
            usage: Some(jcode_sdk::ModelUsage {
                count,
                last_used_unix_secs: Some(100),
                tracking_started_unix_secs: Some(1),
                selection_count: 0,
                last_selected_unix_secs: None,
            }),
        };
        window
            .update(cx, |input, _, cx| {
                input.set_model_routes(
                    vec!["a-rare".into(), "z-favorite".into()],
                    &[route("a-rare", 1), route("z-favorite", 12)],
                    Some("z-favorite".into()),
                    cx,
                );
                input.set_content("/model".into(), cx);
                let suggestions = input.command_suggestions();
                assert_eq!(suggestions[0].value, "/model z-favorite");
                assert!(suggestions[0].active_choice);
                assert!(suggestions[0].help.is_empty());
                assert!(
                    suggestions[0]
                        .detail
                        .as_ref()
                        .unwrap()
                        .contains("12 tracked turns")
                );
            })
            .unwrap();
        cx.simulate_keystrokes(*window, "down");
        window
            .update(cx, |input, _, cx| {
                assert_eq!(
                    input.command_suggestions()[input.command_selection].value,
                    "/model a-rare"
                );
                input.set_model_routes(
                    vec!["a-rare".into(), "z-favorite".into()],
                    &[route("a-rare", 20), route("z-favorite", 12)],
                    Some("z-favorite".into()),
                    cx,
                );
                let suggestions = input.command_suggestions();
                // The highlight follows the route, not the slot, when usage
                // reorders the list.
                assert_eq!(suggestions[input.command_selection].value, "/model a-rare");
                assert_eq!(
                    suggestions[input.command_selection].action,
                    Some(model_stage::StageStep::Submit("/model a-rare".into()))
                );
                input.set_content("/model favorite".into(), cx);
                assert_eq!(input.command_suggestions()[0].value, "/model z-favorite");
            })
            .unwrap();
    }

    #[gpui::test]
    fn appended_dictation_is_laid_out_below_existing_draft(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "Existing draft");
        window
            .update(cx, |input, _, cx| {
                input.append_dictation("Dictated words", cx)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, window, _| {
                assert_eq!(input.content.as_ref(), "Existing draft\nDictated words");
                assert_eq!(
                    input.visual_line_count, 2,
                    "both explicit lines must render"
                );
                let layout = input.last_layout.as_ref().unwrap();
                let caret = layout
                    .position_for_index(input.content.len(), window.line_height())
                    .expect("appended text must have a visible caret position");
                assert_eq!(caret.y, window.line_height());
            })
            .unwrap();
    }

    #[gpui::test]
    fn multiline_layout_preserves_blank_lines_unicode_and_hit_testing(cx: &mut TestAppContext) {
        let window = input_window(cx);
        window
            .update(cx, |input, _, cx| {
                input.set_content("αβ\n\n猫 text\n".into(), cx)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, window, _| {
                let height = window.line_height();
                let layout = input.last_layout.as_ref().unwrap();
                assert_eq!(input.visual_line_count, 4);
                for (index, row) in [(0, 0), (5, 1), (6, 2), (15, 3)] {
                    let position = layout.position_for_index(index, height).unwrap();
                    assert_eq!(position, point(px(0.), height * row));
                    let hit = layout
                        .closest_index_for_position(position + point(px(0.), height / 2.), height)
                        .unwrap_or_else(|index| index);
                    assert_eq!(hit, index);
                }
                assert!(
                    layout
                        .position_for_index(input.content.len() + 1, height)
                        .is_none()
                );
                assert_eq!(
                    selection_quads(
                        layout,
                        0..input.content.len(),
                        input.last_bounds.unwrap(),
                        height
                    )
                    .len(),
                    4
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn appended_dictation_scrolls_into_view_after_multiline_growth(cx: &mut TestAppContext) {
        let window = input_window(cx);
        window
            .update(cx, |input, _, cx| {
                input.set_content("Original line\n".repeat(30), cx)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, _, cx| {
                input.append_dictation("New dictated line\nAnother dictated line", cx)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, _window, _| {
                let height =
                    input.last_bounds.unwrap().size.height / input.visual_line_count as f32;
                let caret = input
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_index(input.content.len(), height)
                    .unwrap();
                let top = input.last_bounds.unwrap().top() + caret.y;
                let viewport = input.editor_scroll.bounds();
                assert!(top >= viewport.top() - px(1.));
                assert!(
                    top + height <= viewport.bottom() + px(1.),
                    "dictation caret must be visible: {top:?} {viewport:?}"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn long_prompts_wrap_and_expand_vertically(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, &"a long prompt ".repeat(200));
        cx.run_until_parked();

        window
            .update(cx, |input, _, _| {
                assert!(
                    input.visual_line_count > 1,
                    "long prompt should occupy multiple visual lines"
                );
            })
            .unwrap();
    }

    /// The workspace binds Ctrl+B to ToggleSidebar while the prompt binds it to
    /// MoveWordLeft. The prompt's context-scoped binding must win while the
    /// prompt has focus, otherwise typing would silently move window chrome.
    #[gpui::test]
    fn ctrl_b_moves_by_word_in_the_prompt_despite_the_sidebar_shortcut(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.update(crate::bind_workspace_keys);
        cx.simulate_input(*window, "one two three");
        cx.run_until_parked();

        let mut cx = gpui::VisualTestContext::from_window(*window, cx);
        cx.simulate_keystrokes("ctrl-b");
        cx.run_until_parked();

        window
            .update(&mut cx, |input, _, _| {
                assert_eq!(
                    &input.content[input.selected_range.start..],
                    "three",
                    "Ctrl+B should move the caret one word left inside the prompt"
                );
            })
            .unwrap();
    }

    #[test]
    fn word_motion_crosses_words_and_whitespace() {
        let text = "one   two three";
        assert_eq!(next_word_boundary(text, 0), 6);
        assert_eq!(next_word_boundary(text, 6), 10);
        assert_eq!(next_word_boundary(text, text.len()), text.len());
        assert_eq!(previous_word_boundary(text, text.len()), 10);
        assert_eq!(previous_word_boundary(text, 10), 6);
        assert_eq!(previous_word_boundary(text, 0), 0);
    }

    #[test]
    fn word_motion_never_splits_multibyte_text() {
        let text = "你好  world 🚀";
        let second_word = next_word_boundary(text, 0);
        assert_eq!(&text[second_word..], "world 🚀");
        let emoji = next_word_boundary(text, second_word);
        assert_eq!(&text[emoji..], "🚀");
        assert_eq!(previous_word_boundary(text, text.len()), emoji);
        for offset in [second_word, emoji, previous_word_boundary(text, emoji)] {
            assert!(text.is_char_boundary(offset));
        }
    }

    #[gpui::test]
    fn ported_editing_chords_dispatch_through_the_real_keymap(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "one two");
        cx.simulate_keystrokes(*window, "alt-b ctrl-w");
        window
            .update(cx, |input, _, _| assert_eq!(input.content.as_ref(), "two"))
            .unwrap();

        cx.simulate_keystrokes(*window, "ctrl-z ctrl-shift-z ctrl-u");
        window
            .update(cx, |input, _, _| assert!(input.content.is_empty()))
            .unwrap();
    }

    #[gpui::test]
    fn standalone_btw_opens_history_only_for_normal_composer_submit(cx: &mut TestAppContext) {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = submitted.clone();
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|_, cx| {
            PromptInput::new_with_queue_files(cx, "test", move |kind, text, images, _, queued, _, _| {
                seen.lock().unwrap().push((kind, text, images.len(), queued));
            }).with_standalone_btw()
        });
        vcx.update(|window, cx| {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
        });
        vcx.simulate_input("btw");
        vcx.simulate_keystrokes("enter");
        vcx.simulate_input("btw this matters");
        vcx.simulate_keystrokes("enter");
        vcx.simulate_input("btw");
        vcx.simulate_keystrokes("ctrl-enter");
        vcx.simulate_input("btw");
        vcx.update(|window, cx| {
            input.update(cx, |input, cx| assert!(input.control_submit(window, cx)));
        });
        vcx.simulate_input("btw");
        input.update(vcx, |input, _| {
            input.attachments.push(Attachment {
                label: "fixture".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
        });
        vcx.simulate_keystrokes("enter");
        use SubmissionKind::{Command, Message};
        assert_eq!(submitted.lock().unwrap().as_slice(), &[
            (Command, "btw".into(), 0, false),
            (Message, "btw this matters".into(), 0, false),
            (Message, "btw".into(), 0, true),
            (Message, "btw".into(), 0, false),
            (Message, "btw".into(), 1, false),
        ]);
    }

    #[gpui::test]
    fn ctrl_enter_preserves_images_and_respects_empty_and_disabled_input(cx: &mut TestAppContext) {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = submitted.clone();
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|_, cx| {
            PromptInput::new_with_queue(cx, "test", move |text, images, queued, _, _| {
                seen.lock().unwrap().push((text, images, queued));
            })
        });
        vcx.update(|window, cx| {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
        });
        vcx.simulate_keystrokes("ctrl-enter");
        assert!(submitted.lock().unwrap().is_empty());
        input.update(vcx, |input, cx| {
            input.attachments.push(Attachment {
                label: "4×3".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
            input.set_submission_enabled(false, cx);
        });
        vcx.simulate_keystrokes("ctrl-enter");
        assert!(submitted.lock().unwrap().is_empty());
        input.update(vcx, |input, cx| {
            assert_eq!(input.attachments.len(), 1);
            input.set_submission_enabled(true, cx);
        });
        vcx.simulate_keystrokes("ctrl-enter");
        assert_eq!(
            &*submitted.lock().unwrap(),
            &[(
                "[image]".into(),
                vec![("image/png".into(), "cG5n".into())],
                true
            )]
        );
        input.read_with(vcx, |input, _| {
            assert!(input.attachments.is_empty());
            assert_eq!(input.visual_line_count, 1);
            assert_eq!(input.history, vec!["[image]"]);
        });
    }

    #[gpui::test]
    fn history_and_escape_dispatch_through_the_real_keymap(cx: &mut TestAppContext) {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = submitted.clone();
        cx.update(bind_keys);
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|cx| {
                    PromptInput::new(cx, "test", move |text, _, _, _| {
                        seen.lock().unwrap().push(text)
                    })
                })
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                window.focus(&input.focus_handle, cx)
            })
            .unwrap();

        cx.simulate_input(*window, "first");
        cx.simulate_keystrokes(*window, "enter");
        cx.simulate_input(*window, "draft");
        cx.simulate_keystrokes(*window, "ctrl-[ ctrl-] up escape");

        assert_eq!(&*submitted.lock().unwrap(), &["first"]);
        window
            .update(cx, |input, _, _| assert!(input.content.is_empty()))
            .unwrap();
    }

    #[gpui::test]
    fn other_sessions_todos_never_become_composer_examples(cx: &mut TestAppContext) {
        let home = std::env::temp_dir().join(format!(
            "jcode-desktop-example-prompts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(home.join("todos")).unwrap();
        std::fs::write(
            home.join("todos/closed.json"),
            r#"[{"content":"Ship the onboarding flow","status":"pending"},
                {"content":"Fix the parser crash","status":"in_progress"},
                {"content":"Already done","status":"completed"}]"#,
        )
        .unwrap();
        std::fs::write(
            home.join("todos/running.json"),
            r#"[{"content":"Still being worked on","status":"pending"}]"#,
        )
        .unwrap();
        let session = |id: &str, status: &str| {
            let mut info: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
                "session_id": id,
                "status": status,
                "working_dir": "/repo/app",
            }))
            .unwrap();
            info.title = Some(id.into());
            info
        };
        let prompts = crate::example_prompts::load_from(
            &home,
            &[session("closed", "idle"), session("running", "running")],
        );
        std::fs::remove_dir_all(&home).unwrap();

        let (input, vcx) = cx.add_window_view(|_, cx| {
            let mut input =
                PromptInput::new(cx, "Type something…", |_, _, _, _| {}).with_example_prompts();
            input.set_working_dir(Some("/repo/app"));
            input
        });
        vcx.update(|_, cx| crate::example_prompts::set(prompts, cx));
        vcx.run_until_parked();
        let picked = |vcx: &mut gpui::VisualTestContext| {
            input.update(vcx, |input, _| {
                let state = input.motion.borrow();
                let picked = state
                    .prompts
                    .as_ref()
                    .expect("composer painted its examples");
                (picked.prompts[0].to_string(), picked.prompts[1].to_string())
            })
        };
        let (first, second) = picked(vcx);
        assert!(motion::FALLBACK_PROMPTS.contains(&first.as_str()));
        assert!(motion::FALLBACK_PROMPTS.contains(&second.as_str()));

        // A composer in another project falls back to the generic catalog.
        input.update(vcx, |input, cx| {
            input.set_working_dir(Some("/elsewhere"));
            cx.notify();
        });
        vcx.run_until_parked();
        let (first, second) = picked(vcx);
        assert!(
            motion::FALLBACK_PROMPTS.contains(&first.as_str()),
            "{first}"
        );
        assert!(
            motion::FALLBACK_PROMPTS.contains(&second.as_str()),
            "{second}"
        );
    }

    #[gpui::test]
    fn composer_editor_has_no_prompt_prefix_in_compact_or_spacious_mode(cx: &mut TestAppContext) {
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "Type something…", |_, _, _, _| {}));
        for spacious in [false, true] {
            input.update(vcx, |input, cx| {
                input.spacious = spacious;
                cx.notify();
            });
            vcx.run_until_parked();
            let composer = vcx.debug_bounds("prompt-input").unwrap();
            let editor = vcx.debug_bounds("prompt-editor").unwrap();
            let left = editor.left() - composer.left();
            let right = composer.right() - editor.right();
            assert!(
                f32::from(left - right).abs() < 1.0,
                "editor must not reserve extra left padding for a prompt marker"
            );
        }
    }

    #[gpui::test]
    fn prompt_stash_survives_snapshot_restart(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        vcx.update(|window, cx| {
            input.update(cx, |input, cx| window.focus(&input.focus_handle, cx))
        });
        vcx.simulate_keystrokes("a ctrl-s");
        let encoded = input.read_with(vcx, |input, _| {
            serde_json::to_string(&input.snapshot()).unwrap()
        });
        let snapshot = serde_json::from_str(&encoded).unwrap();
        let (restarted, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        vcx.update(|window, cx| {
            restarted.update(cx, |input, cx| {
                input.restore(snapshot, cx);
                window.focus(&input.focus_handle, cx);
            })
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-stashed").is_some());
        vcx.simulate_keystrokes("ctrl-s");
        restarted.read_with(vcx, |input, _| assert_eq!(input.content.as_ref(), "a"));
    }

    #[gpui::test]
    fn prompt_stash_returns_after_send(cx: &mut TestAppContext) {
        let window = input_window(cx);
        cx.simulate_input(*window, "saved draft");
        cx.simulate_keystrokes(*window, "ctrl-s");
        window
            .update(cx, |input, _, _| assert!(input.content.is_empty()))
            .unwrap();
        cx.simulate_input(*window, "send this");
        cx.simulate_keystrokes(*window, "enter");
        window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "saved draft")
            })
            .unwrap();
    }

    #[gpui::test]
    fn prompt_stash_swaps_and_keeps_indicator(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        vcx.update(|window, cx| {
            input.update(cx, |input, cx| window.focus(&input.focus_handle, cx))
        });
        vcx.simulate_keystrokes("a ctrl-s");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("prompt-stashed").is_some());
        vcx.simulate_keystrokes("b ctrl-s");
        input.update(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "a");
            assert_eq!(input.stashed_prompt.as_ref().unwrap().0, "b");
        });
        vcx.simulate_keystrokes("ctrl-s");
        input.update(vcx, |input, _| assert_eq!(input.content.as_ref(), "b"));
    }

    #[gpui::test]
    fn submitting_wrapped_prompt_resets_height_before_the_next_paint(cx: &mut TestAppContext) {
        let (input, vcx) =
            cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
        vcx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.content = "A wrapped prompt occupying several visual lines".into();
                input.visual_line_count = 12;
                input.submit(&Submit, window, cx);
                assert!(input.content.is_empty());
                assert_eq!(
                    input.visual_line_count, 1,
                    "cleared editor retained old wrapped height"
                );
            });
        });
    }

    #[gpui::test]
    fn image_only_prompt_submits_the_attachment_and_clears_the_composer(cx: &mut TestAppContext) {
        let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = submitted.clone();
        cx.update(bind_keys);
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|cx| {
                    PromptInput::new(cx, "test", move |text, images, _, _| {
                        seen.lock().unwrap().push((text, images));
                    })
                })
            })
            .unwrap()
        });
        window
            .update(cx, |input, window, cx| {
                input.attachments.push(Attachment {
                    label: "4×3".into(),
                    payload: AttachmentPayload::Image {
                        media_type: "image/png".into(),
                        encoded: "cG5n".into(),
                        preview: preview_image("image/png", Vec::new()).unwrap(),
                    },
                });
                window.focus(&input.focus_handle, cx);
            })
            .unwrap();

        cx.simulate_keystrokes(*window, "enter");

        assert_eq!(
            &*submitted.lock().unwrap(),
            &[(
                "[image]".to_string(),
                vec![("image/png".to_string(), "cG5n".to_string())]
            )]
        );
        window
            .update(cx, |input, _, _| {
                assert!(input.content.is_empty());
                assert!(input.attachments.is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn removing_an_attachment_updates_the_visible_count(cx: &mut TestAppContext) {
        let window = input_window(cx);
        window
            .update(cx, |input, _, cx| {
                for label in ["4×3", "8×6"] {
                    input.attachments.push(Attachment {
                        label: label.into(),
                        payload: AttachmentPayload::Image {
                            media_type: "image/png".into(),
                            encoded: "cG5n".into(),
                            preview: preview_image("image/png", Vec::new()).unwrap(),
                        },
                    });
                }
                input.remove_attachment(0, cx);
                assert_eq!(input.attachments.len(), 1);
                assert_eq!(input.attachments[0].label.as_ref(), "8×6");
                assert_eq!(input.attachment_notice.as_deref(), None);
            })
            .unwrap();
    }
}

#[cfg(test)]
#[path = "input_responsive_tests.rs"]
mod responsive_tests;

#[cfg(test)]
mod pending_session_tests {
    use super::*;

    #[gpui::test]
    fn pending_commands_preserve_editor_and_explain_readiness(cx: &mut gpui::TestAppContext) {
        cx.update(bind_keys);
        let (input, vcx) = cx.add_window_view(|_, cx| {
            let mut input = PromptInput::new(cx, "test", |_, _, _, _| {
                panic!("pending commands must not invoke submit callbacks");
            });
            input.set_pending_session(true, cx);
            input
        });
        vcx.update(|window, cx| {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
        });
        for command in ["/clear", "/login", "/changelog", "/onboarding-sim"] {
            input.update(vcx, |input, cx| {
                input.content = command.into();
                input.selected_range = command.len()..command.len();
                cx.notify();
            });
            vcx.simulate_keystrokes("enter");
            vcx.simulate_keystrokes("ctrl-enter");
            input.read_with(vcx, |input, _| {
                assert_eq!(input.content.as_ref(), command);
                assert_eq!(
                    input.attachment_notice.as_deref(),
                    Some(PENDING_COMMAND_NOTICE)
                );
            });
        }
        input.update(vcx, |input, cx| {
            input.set_pending_session(false, cx);
            assert!(input.attachment_notice.is_none());
            assert_eq!(input.content.as_ref(), "/onboarding-sim");
        });
    }

    #[gpui::test]
    fn shift_modified_enter_space_backspace_edit_the_composer(cx: &mut gpui::TestAppContext) {
        cx.update(bind_keys);
        let submitted = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sink = submitted.clone();
        let (input, vcx) = cx.add_window_view(move |_, cx| {
            PromptInput::new(cx, "test", move |text, _, _, _| {
                sink.borrow_mut().push(text.to_string());
            })
        });
        vcx.update(|window, cx| {
            window.focus(&input.read(cx).focus_handle.clone(), cx);
        });
        vcx.simulate_input("ab");
        vcx.simulate_keystrokes("shift-enter");
        vcx.simulate_input("c");
        vcx.simulate_keystrokes("shift-space");
        vcx.simulate_input("dx");
        vcx.simulate_keystrokes("shift-backspace");
        input.read_with(vcx, |input, _| {
            assert_eq!(input.content.as_ref(), "ab\nc d")
        });
        assert!(submitted.borrow().is_empty());
        vcx.simulate_keystrokes("enter");
        assert_eq!(submitted.borrow().as_slice(), ["ab\nc d".to_string()]);
    }
}

#[cfg(test)]
#[path = "input_model_profile_tests.rs"]
mod model_profile_tests;

#[cfg(test)]
#[path = "input_model_picker_tests.rs"]
mod model_picker_tests;

#[cfg(test)]
#[path = "input_model_order_tests.rs"]
mod model_order_tests;

#[cfg(test)]
#[path = "input_model_stage_tests.rs"]
mod model_stage_tests;

/// Pick the route spec to switch to when the user selects `provider`.
///
/// Keeps the current model when the target provider serves it. Otherwise
/// lands on the provider's newest flagship using the daemon's shared ranking
/// (`preferred_model_for_provider`), not the alphabetically first route, which
/// previously put Copilot users on `claude-opus-4.6`.
fn provider_route_spec_in(
    details: &HashMap<String, model_menu::ModelDetails>,
    provider: &str,
    current: Option<&str>,
) -> Option<String> {
    let wanted = provider_route_methods(provider);
    let mut matches: Vec<_> = details
        .iter()
        .filter(|(_, detail)| {
            let method = detail.api_method.to_ascii_lowercase();
            let owner = detail.provider.to_ascii_lowercase();
            wanted.iter().any(|wanted| {
                method == *wanted || method.starts_with(&format!("{wanted}-")) || owner == *wanted
            })
        })
        .collect();
    if let Some((spec, _)) = matches
        .iter()
        .find(|(_, detail)| current.is_some_and(|model| detail.model == model))
    {
        return Some((*spec).clone());
    }
    let candidates: Vec<&str> = matches
        .iter()
        .map(|(_, detail)| detail.model.as_str())
        .collect();
    if let Some(best) =
        jcode_base::auth::lifecycle::preferred_model_for_provider(provider, &candidates)
        && let Some((spec, _)) = matches.iter().find(|(_, detail)| detail.model == best)
    {
        return Some((*spec).clone());
    }
    matches.sort_by(|(a, left), (b, right)| {
        right
            .recommended
            .cmp(&left.recommended)
            .then_with(|| a.cmp(b))
    });
    matches.first().map(|(spec, _)| (*spec).clone())
}

#[cfg(test)]
mod provider_route_spec_tests {
    use super::*;

    fn details(routes: &[(&str, &str, &str)]) -> HashMap<String, model_menu::ModelDetails> {
        routes
            .iter()
            .map(|(model, provider, method)| {
                (
                    format!("{method}:{model}"),
                    model_menu::ModelDetails {
                        model: (*model).into(),
                        recommended: false,
                        provider: (*provider).into(),
                        api_method: (*method).into(),
                        usage: None,
                        speed_options: Vec::new(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn switching_provider_lands_on_newest_flagship() {
        let catalog = details(&[
            ("claude-haiku-4.5", "Copilot", "copilot"),
            ("claude-opus-4.6", "Copilot", "copilot"),
            ("claude-opus-5.5", "Copilot", "copilot"),
            ("anthropic/claude-sonnet-4", "auto", "openrouter"),
            ("anthropic/claude-opus-4.8", "auto", "openrouter"),
            ("grok-code-fast-1", "Grok Build", "grok-build-acp"),
            ("grok-4.7", "Grok Build", "grok-build-acp"),
            ("claude-opus-4-6", "Anthropic", "claude-oauth"),
            ("claude-opus-5-5", "Anthropic", "claude-oauth"),
            ("gpt-5.5", "OpenAI", "openai-oauth"),
            ("gpt-6-astra", "OpenAI", "openai-oauth"),
        ]);
        let cases = [
            ("copilot", "copilot:claude-opus-5.5"),
            ("openrouter", "openrouter:anthropic/claude-opus-4.8"),
            ("grok-build", "grok-build-acp:grok-4.7"),
            ("claude", "claude-oauth:claude-opus-5-5"),
            ("openai", "openai-oauth:gpt-6-astra"),
        ];
        for (provider, expected) in cases {
            assert_eq!(
                provider_route_spec_in(&catalog, provider, Some("some-other-model")).as_deref(),
                Some(expected),
                "{provider}"
            );
        }
    }

    #[test]
    fn switching_provider_keeps_current_model_when_served() {
        let catalog = details(&[
            ("claude-opus-4.6", "Copilot", "copilot"),
            ("claude-opus-5.5", "Copilot", "copilot"),
        ]);
        assert_eq!(
            provider_route_spec_in(&catalog, "copilot", Some("claude-opus-4.6")).as_deref(),
            Some("copilot:claude-opus-4.6")
        );
    }
}

/// A route for a signed-in provider the session catalog does not list yet.
///
/// A session's catalog follows its current runtime, so after a switch to one
/// provider the others can be missing from it. Without this the picker had no
/// way back: every other row was inert until the user signed in again.
pub(crate) fn provider_fallback_spec(provider: &str) -> Option<String> {
    let prefix = match provider {
        "claude" => "claude-oauth",
        "anthropic-api" => "claude-api",
        "openai" => "openai-oauth",
        "openai-api" => "openai-api",
        "copilot" | "cursor" | "gemini" | "antigravity" => provider,
        other => {
            let profile = jcode_base::provider_catalog::openai_compatible_profile_by_id(other)?;
            return profile
                .default_model
                .map(|model| format!("{}:{model}", profile.id));
        }
    };
    let model = jcode_base::auth::lifecycle::default_model_for_provider(provider)?;
    Some(format!("{prefix}:{model}"))
}

fn provider_route_methods(provider: &str) -> Vec<String> {
    match provider {
        "openai" => vec!["openai-oauth".into()],
        "openai-api" => vec!["openai-api-key".into(), "openai-api".into()],
        "claude" => vec!["claude-oauth".into()],
        "anthropic-api" => vec![
            "anthropic-api-key".into(),
            "anthropic-api".into(),
            "claude-api".into(),
        ],
        other => vec![other.into()],
    }
}
