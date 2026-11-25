//! Panel: one Jcode session as a spatial card with a live transcript.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, ListAlignment, ListState,
    ScrollHandle, SharedString, StyledImage, Task, Window, div, img, list, point, prelude::*, px,
    relative,
};
use jcode_sdk::ApiEvent;
use serde::{Deserialize, Serialize};

use crate::commands::{help_markdown, registered_command};
use crate::harness::{Bridge, Command, SessionOperation};
use crate::input::{PromptInput, PromptInputSnapshot};
use crate::markdown;
use crate::preview_state::PreviewState;
use crate::text_selection::{self, TextSelection};
use crate::theme::{Ink, Palette, Theme};
use crate::todoist::{CreateTask, Project as TodoistProject, Task as TodoistTask, TodoistClient};

#[path = "panel_control.rs"]
mod control;

#[path = "panel_workspace_commands.rs"]
pub(crate) mod workspace_commands;

#[path = "panel_special_commands.rs"]
mod special_commands;

#[path = "panel_command_recovery.rs"]
mod command_recovery;

#[path = "panel_runtime_commands.rs"]
mod runtime_commands;

#[path = "panel_commands.rs"]
mod desktop_commands;

#[path = "panel_chrome.rs"]
mod chrome;
#[path = "panel_classify.rs"]
mod classify;
#[path = "panel_turn.rs"]
mod turn;
use turn::{AutoRetry, StopNotice};
#[path = "panel_meter.rs"]
pub(crate) mod meter;
#[path = "panel_facade.rs"]
mod facade;
#[path = "panel_fixtures.rs"]
mod fixtures;
use fixtures::{demo_items, demo_tool_times, screenshot_fixture_for};
#[path = "panel_labels.rs"]
mod labels;
#[path = "panel_todo_card.rs"]
mod todo_card;
#[path = "panel_tool_summary.rs"]
mod tool_summary;
pub(crate) use labels::{human_auth_method, tool_card_token_pill};
use labels::*;
use todo_card::*;
use tool_summary::*;
pub(crate) use crate::accounts::pretty_provider_name;
#[path = "panel_todoist.rs"]
mod todoist;
#[path = "panel_render.rs"]
mod render;
#[path = "panel_items.rs"]
mod items;
pub use items::{AssistantOutputPhase, Item, TodoCardPayload, Transcript, TranscriptImage};
use items::*;
#[path = "panel_live.rs"]
pub(crate) mod live;
use live::Live;
#[path = "panel_clean_chat.rs"]
mod clean_chat;
#[path = "panel_assistant_output.rs"]
mod assistant_output;


#[cfg(test)]
#[path = "panel_command_workflow_tests.rs"]
mod command_workflow_tests;

#[cfg(test)]
#[path = "panel_slash_send_tests.rs"]
mod slash_send_tests;

#[cfg(test)]
#[path = "panel_rewind_tests.rs"]
mod rewind_tests;


#[path = "panel_menu.rs"]
mod overlay_keys;

#[cfg(test)]
#[path = "panel_display_tests.rs"]
mod display_tests;

#[cfg(test)]
#[path = "panel_applet_interaction_tests.rs"]
mod applet_interaction_tests;

#[cfg(test)]
#[path = "panel_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "panel_file_drop_tests.rs"]
mod file_drop_tests;

#[cfg(test)]
#[path = "panel_memory_tests.rs"]
mod memory_tests;

#[cfg(test)]
#[path = "panel_provisional_history_tests.rs"]
mod provisional_history_tests;

#[path = "panel_reveal.rs"]
mod reveal;
pub(crate) use reveal::PreviewScroll;

#[path = "panel_text_document.rs"]
mod text_document;

#[path = "panel_snapshot.rs"]
mod snapshot;
pub use snapshot::TranscriptSnapshot;

#[path = "panel_background_task.rs"]
mod background_task;

#[path = "panel_tool_output.rs"]
mod tool_output;
#[path = "panel_codemode_card.rs"]
mod codemode_card;

#[path = "panel_compaction.rs"]
pub(crate) mod compaction;

#[path = "panel_compaction_render.rs"]
mod compaction_render;
#[path = "panel_history_paging.rs"]
mod history_paging;
#[path = "panel_history_items.rs"]
mod history_items;

#[path = "panel_activity.rs"]
mod activity;
#[path = "panel_activity_state.rs"]
mod activity_state;
#[path = "panel_scroll_motion.rs"]
mod scroll_motion;
#[path = "panel_steady_activity.rs"]
mod steady_activity;
#[path = "panel_tail_glide.rs"]
mod tail_glide;
use scroll_motion::WheelGlide;
#[path = "panel_agent_message.rs"]
mod agent_message;
#[path = "panel_ask.rs"]
mod ask;
#[path = "panel_ask_images.rs"]
mod ask_images;
#[path = "panel_background_jobs.rs"]
mod background_jobs;
#[path = "panel_cache_miss.rs"]
mod cache_miss;
#[path = "panel_cache_timer.rs"]
pub(crate) mod cache_timer;
#[path = "panel_composer.rs"]
mod composer;
#[path = "panel_context_usage.rs"]
mod context_usage;
#[path = "panel_decision.rs"]
mod decision;
#[path = "panel_decision_tests.rs"]
#[cfg(test)]
mod decision_tests;
#[path = "panel_fork_prompt.rs"]
mod fork_prompt;
#[path = "panel_rig_hud.rs"]
mod rig_hud;
#[path = "panel_routing.rs"]
mod account_routing;
#[path = "panel_side_question.rs"]
mod side_question;
#[path = "panel_session_status.rs"]
mod session_status;
#[path = "panel_swarm_card.rs"]
mod swarm_card;
#[path = "panel_swarm_indicator.rs"]
mod swarm_indicator;
pub(crate) use swarm_indicator::OpenSwarmWorker;
#[path = "panel_diff.rs"]
mod diff_review;
#[path = "panel_effort.rs"]
mod effort_switch;
#[path = "panel_model_switch.rs"]
mod model_switch;
#[path = "panel_speed.rs"]
mod speed;
#[path = "panel_mode.rs"]
mod mode_switch;
// One disclosure model for every truncated line: tool titles, errors, output,
// prompts, notices, background tasks and swarm lines.
#[path = "expandable.rs"]
pub(crate) mod expandable;
#[path = "panel_flicker.rs"]
mod flicker;
#[path = "panel_gallery.rs"]
mod gallery;
#[path = "panel_image_pane.rs"]
mod image_pane;
#[cfg(test)]
#[path = "panel_image_pane_tests.rs"]
mod image_pane_tests;
#[path = "panel_image_preview.rs"]
mod image_preview;
#[path = "panel_latest.rs"]
mod latest;
#[path = "panel_login.rs"]
mod login;
#[path = "panel_preview.rs"]
mod preview;
#[path = "panel_prompt.rs"]
mod prompt;
#[path = "panel_queue.rs"]
mod queue;
#[path = "panel_recovery.rs"]
mod recovery;
#[cfg(test)]
#[path = "panel_scroll_momentum_tests.rs"]
mod scroll_momentum_tests;
#[path = "panel_shortcuts.rs"]
pub(crate) mod shortcuts;
#[path = "panel_startup.rs"]
mod startup;
#[path = "panel_stop_reason.rs"]
mod stop_reason;
#[path = "panel_stream_reveal.rs"]
mod stream_reveal;
#[cfg(test)]
#[path = "panel_stream_scroll_tests.rs"]
mod stream_scroll_tests;
pub use startup::StartupLayout;
#[path = "panel_applet.rs"]
pub(crate) mod applet_panel;
#[path = "panel_demo_replay.rs"]
pub(crate) mod demo_replay;
#[path = "panel_gmail_draft_card.rs"]
mod gmail_draft_card;
#[path = "panel_gmail_read_card.rs"]
mod gmail_read_card;
#[path = "panel_mcp_list_card.rs"]
mod mcp_list_card;
#[path = "panel_openrig.rs"]
pub(crate) mod openrig;
#[path = "panel_orchestration.rs"]
pub(crate) mod orchestration;
#[path = "panel_response_stats.rs"]
mod response_stats;
#[path = "panel_side_document.rs"]
mod side_document;
#[path = "panel_tab_outline.rs"]
mod tab_outline;
#[path = "panel_user_response.rs"]
mod user_response;
pub(crate) use tab_outline::{TabRing, tab_corner_radius};
#[path = "panel_tool_streaming.rs"]
mod tool_streaming;
#[path = "panel_usage.rs"]
pub(crate) mod usage;
#[path = "panel_voice_state.rs"]
mod voice_state;
#[path = "panel_voice_trace.rs"]
mod voice_trace;
#[path = "panel_voice.rs"]
pub(crate) mod voice;
pub use side_document::SideDocumentSnapshot;

pub(crate) type SessionOpener =
    Arc<dyn Fn(crate::harness::UnfinishedSession, &mut Window, &mut App)>;

// Keep the last message/card clear of the composer and its metadata. This is
// outside the scrolling list so it remains visible even while reading history.
const TRANSCRIPT_BOTTOM_GAP: f32 = 12.0;

// Distance from the composer's pill row to the panel's bottom edge. The docked
// dock and the welcome-started layout share it, so pill rows line up across
// panels instead of one sitting flush with the window edge.
const COMPOSER_BOTTOM_INSET: f32 = 8.0;

/// Opening a panel is not yet a conversation. A local draft asks the runtime
/// for a session only when the user sends its first prompt, so abandoned
/// panels never register a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftSession {
    /// Mounted, with no decision yet about when to create its session.
    Unrequested,
    /// Waiting for the first prompt.
    Deferred,
    /// The create request is in flight, or the session exists.
    Requested,
}

pub(crate) const DEFERRED_SESSION_STATUS: &str = "New session";

fn command_unavailable_message(input: &str) -> String {
    let name = input.split_whitespace().next().unwrap_or(input);
    match registered_command(name) {
        Some(command) => format!(
            "`{name}` is a Jcode command, but its {} behavior is not available in Desktop yet.",
            command.help.to_ascii_lowercase()
        ),
        None => format!("Unknown command: `{name}`. Type `/help` for available commands."),
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

pub(crate) struct AccountsPanelClosed;
pub(crate) struct AccountsPanelChooseModel;
/// A banked reset on the Accounts page was clicked. Opens the review only.
pub(crate) struct AccountsPanelRedeemReset(pub crate::accounts::BankedReset);

impl gpui::EventEmitter<AccountsPanelRedeemReset> for Panel {}

impl gpui::EventEmitter<AccountsPanelChooseModel> for Panel {}

impl gpui::EventEmitter<AccountsPanelClosed> for Panel {}

pub struct Panel {
    rig_feed: Entity<crate::openrig_feed::Feed>,
    rig_host_id: Option<String>,
    rig_demand: Option<crate::openrig_feed::Demand>,
    rig_identity: Option<rig_hud::Identity>,
    pub preview_state: Option<PreviewState>,
    /// Scripted onboarding replay: real rendering and motion, never sounds.
    pub(crate) demo: bool,
    pub session_id: String,
    pub title: SharedString,
    pub working_dir: Option<String>,
    pub status: String,
    pub connection_phase: String,
    continuing_after_reload: bool,
    turn_outcome: Option<turn::TurnOutcome>,
    /// Model id serving this session, e.g. `gpt-5.6-sol`.
    pub model: Option<String>,
    /// Provider display name, e.g. `openai` or `anthropic`.
    pub provider: Option<String>,
    /// Credential route for the current model, e.g. `oauth` or `api key`.
    pub auth_method: Option<String>,
    pub account_label: Option<String>,
    /// Reasoning effort, e.g. `high`, when the provider exposes it.
    pub reasoning_effort: Option<String>,
    skills: Vec<(String, String)>,
    active_skill: Option<String>,
    pinned_modes: Vec<String>,
    mode_multiselect: bool,
    mode_catalog: crate::mode_catalog::ModeCatalog,
    /// A model switch the daemon accepted but holds until the running turn
    /// ends. The session keeps serving `model` meanwhile, so this is shown
    /// beside the model chip and never replaces it.
    pub pending_model: Option<String>,
    /// Effort requests the runtime has not answered yet.
    pending_effort: Option<effort_switch::PendingEffort>,
    /// `/save` or `/unsave` requests awaiting the daemon's confirmation, so
    /// the transcript only claims success once the bookmark persisted.
    pending_save_requests: usize,
    /// Latest provider-reported prompt occupancy, with cache accounting normalized.
    context_tokens: Option<u64>,
    context_usage: context_usage::State,
    /// Memory pipeline activity for the session, when something is running.
    /// Display-only: it feeds the composer status line, never the transcript.
    memory_activity: Option<jcode_sdk::api::MemoryActivitySnapshot>,
    response_stats: response_stats::Tracker,
    cache_telemetry: cache_timer::card::CacheTelemetry,
    pub items: Transcript,
    /// Streaming assistant text accumulates here until the turn ends.
    streaming_text: String,
    live_assistant_output: Option<assistant_output::LiveAssistantOutput>,
    streaming_reasoning: String,
    /// Paced, fading reveal of the live rows above. Presentation only.
    text_reveal: stream_reveal::StreamReveal,
    reasoning_reveal: stream_reveal::StreamReveal,
    #[cfg(test)]
    animate_stream_in_tests: bool,
    sound_events: crate::sound_events::SoundEvents,
    activity_spinner: Entity<activity::Spinner>,
    latest_activity_spinner: Entity<activity::Spinner>,
    tab_outline: Entity<tab_outline::TabOutline>,
    /// The last turn finished while this session was out of view.
    finished_unseen: bool,
    sidebar_spinner: Entity<activity::Spinner>,
    /// Selected workspace surface, independent of temporary keyboard focus.
    surface_focused: bool,
    pub input: Entity<PromptInput>,
    /// Coordinator-only: its swarm workers, for the composer indicator.
    swarm_workers: Vec<crate::swarm_view::Worker>,
    background_jobs: background_jobs::BackgroundJobs,
    /// Coordinator-only: background swarm awaits still in flight. The daemon
    /// persists them, so a reload no longer makes the coordinator look idle.
    pending_awaits: Vec<jcode_sdk::PendingAwait>,
    pending_await_refresh: Option<swarm_indicator::AwaitRefresh>,
    tab_dependency_waiting: bool,
    /// This session's own record, when it is a swarm worker. Headless workers
    /// answer no model probe on attach, so their model and effort come from
    /// here instead of reading as unset.
    worker_record: Option<jcode_sdk::WorkerMeta>,
    /// The worker list opened from the composer's count is showing.
    swarm_list_open: bool,
    swarm_list_focus: FocusHandle,
    swarm_finished_open: bool,
    swarm_finished_focus: FocusHandle,
    swarm_picker_rows: Vec<FocusHandle>,
    swarm_list_scroll: ScrollHandle,
    swarm_trigger_focus: FocusHandle,
    /// Animated status marks (activity icon, worker spinner and rows).
    swarm_marks: crate::swarm_glyph::Marks,
    /// Worker spend (model, provider, total tokens) for inline swarm cards.
    swarm_usage: HashMap<String, jcode_desktop_harness::swarm_usage::WorkerUsage>,
    swarm_row_focus: std::cell::RefCell<HashMap<String, gpui::FocusHandle>>,
    voice: voice_state::VoiceState,
    image_pane_open: bool,
    session_image_gallery: gallery::Gallery,
    session_image_base: usize,
    draft_has_images: bool,
    image_pane_selected: Option<usize>,
    image_pane_scroll: ScrollHandle,
    image_pane_focus: FocusHandle,
    image_pane_stacked: bool,
    image_preview: Option<TranscriptImage>,
    image_preview_focus_pending: bool,
    pending_history_image: Option<String>,
    history_image_requests: std::cell::RefCell<HashMap<String, image_preview::HistoryImageLoad>>,
    image_preview_dimensions: Option<(u32, u32)>,
    diff_review: Option<diff_review::DiffReview>,
    edit_previews: diff_review::EditPreviews,
    image_preview_zoom: f32,
    image_preview_scroll: gpui::ScrollHandle,
    image_preview_drag: Option<gpui::Point<gpui::Pixels>>,
    pub focus_handle: FocusHandle,
    transcript_list: ListState,
    clean_chat_override: Option<bool>,
    pending_approval: Option<clean_chat::PendingApproval>,
    /// The account serving this session, from the daemon's latest decision.
    routing_chip: Option<crate::routing::RoutingChip>,
    /// A spending limit holds the turn until the person answers.
    routing_gate: Option<crate::routing::GateCard>,
    rewind_selection: Option<history_paging::rewind::Selection>,
    expandable_viewport_height: gpui::Pixels,
    transcript_row_count: usize,
    transcript_measurements: TranscriptMeasurements,
    flicker_diagnostics: std::rc::Rc<std::cell::RefCell<flicker::Detector>>,
    startup_layout: Option<startup::StartupLayout>,
    offscreen_prompt: Option<usize>,
    offscreen_prompt_clip: Option<gpui::Pixels>,
    expanded_pinned_prompt: Option<usize>,
    swarm_scrolls: std::cell::RefCell<std::collections::HashMap<String, ScrollHandle>>,
    /// Tracks a Desktop publish pipeline. Hot reloads recover it from the title.
    publish_tracker: bool,
    transcript_selection: Entity<TextSelection>,
    transcript_text_document: text_document::TranscriptTextDocument,
    transcript_dragging: bool,
    /// Remaining wheel travel. Precise touchpad input stays directly mapped
    /// so native gesture control never fights a second momentum animation.
    transcript_wheel_glide: WheelGlide,
    transcript_wheel_frame: Option<Instant>,
    transcript_wheel_frame_pending: bool,
    /// Eased tail follow state. See `panel_tail_glide`.
    tail_glide: tail_glide::TailGlide,
    stick_to_bottom: bool,
    transcript_end_visible: bool,
    /// Height of the floating composer dock. The transcript scrolls beneath
    /// it, padded by this much so the live end still clears the composer.
    composer_dock_height: f32,
    /// Eased screen position of the live activity row.
    steady_activity: steady_activity::SteadyCell,
    /// Padding last applied to the final transcript row. A change must
    /// remeasure that row, whose cached height includes it.
    transcript_end_pad: f32,
    /// A detached reload offset cannot be applied until asynchronous history
    /// has rebuilt the scroll region. Painting the empty panel clamps it to 0.
    pending_history_scroll: Option<(f32, f32)>,
    bridge: Bridge,
    /// Whether this draft has asked the runtime for its session yet.
    draft_session: DraftSession,
    /// Model and effort picked on a draft, applied once its session exists.
    draft_config: DraftConfig,
    speed: speed::SessionSpeed,
    history_loaded: bool,
    history_paging: Option<history_paging::HistoryPaging>,
    reconnect_response: Option<jcode_sdk::HistoryMessage>,
    restored_transcript: bool,
    /// Leading items painted from the persisted session record while the
    /// runtime attaches. The authoritative history reply replaces them.
    provisional_items: usize,
    provisional_task: Option<gpui::Task<()>>,
    /// A stored transcript is being read. Suppress the fresh-session empty
    /// state so an established conversation never flashes the new-chat prompt.
    awaiting_persisted: bool,
    /// Regions the user expanded, keyed by tool call id and by row key (see
    /// `expandable::key`). One flat set, so every disclosure toggles the same
    /// way and is hashed into the transcript fingerprint the same way.
    expanded_regions: HashSet<String>,
    /// Retain closing cards until their fade finishes, and allow smooth reversal.
    tool_detail_motion: HashMap<String, crate::transition::AnimatedValue>,
    prompt_queue: queue::PromptQueue,
    pending_users: VecDeque<usize>,
    accepted_users: HashMap<usize, Instant>,
    /// Arrival time (Unix epoch ms) of user prompts, by item index. Taken from
    /// the stored message record or the moment of sending, never render time.
    user_times: HashMap<usize, i64>,
    /// Newly received tool calls, keyed by call id, while their entrance runs.
    arriving_tools: HashMap<String, Instant>,
    /// Item count when each unanchored inline applet first appeared, so an
    /// `end` card stays where it was mounted as the conversation continues.
    applet_positions: HashMap<String, usize>,
    /// Selection scope for transcript applet cards.
    applet_selection: Entity<TextSelection>,
    /// Applet runtime generation last reflected in transcript measurements.
    applet_generation: (u64, u64, bool),
    /// Unanchored transcript applets as `(item position, instance)`, in order.
    applet_rows: Vec<(usize, String)>,
    /// Advances whenever `applet_rows` is replaced.
    applet_rows_version: u64,
    /// Transcript-derived data reused while its inputs are unchanged.
    transcript_derived: Option<TranscriptDerived>,
    control_reveal: Option<reveal::ControlReveal>,
    /// Live tool timing: start instants while running, final durations after.
    tool_started: HashMap<String, Instant>,
    tool_durations: HashMap<String, Duration>,
    tool_wall_times: HashMap<String, (Option<i64>, Option<i64>)>,
    codemode_summaries: HashMap<String, Box<jcode_sdk::api::CodemodeSummary>>,
    /// A running tool's breathing veil and clock, each a leaf view that
    /// ticks on its own, so the transcript is not rebuilt 20 times a second
    /// while a tool runs. Filled while the transcript renders.
    running_tool_tickers: std::cell::RefCell<crate::pulse_text::Tickers>,
    /// Gmail compose cards a later call replaced, keyed by call id, cached
    /// for the transcript revision it was computed from.
    superseded_drafts: std::cell::RefCell<(
        u64,
        std::rc::Rc<HashMap<String, gmail_draft_card::Superseded>>,
    )>,
    unfinished_work: Option<Vec<crate::harness::UnfinishedSession>>,
    unfinished_session_opener: Option<SessionOpener>,
    side_document: Option<side_document::SideDocument>,
    /// One mounted applet instance, rendered from the shared applet runtime.
    applet: Option<applet_panel::AppletPanel>,
    /// A native Todoist-backed task view. The existing session todo cards remain
    /// independent and continue to represent the agent's current work.
    todoist: Option<TodoistPanelState>,
    /// Live sessions, their run state and todos. Never shows chat content.
    orchestration: Option<orchestration::State>,
    openrig: Option<Entity<openrig::State>>,
    recovery_picker_open: bool,
    /// Pending `ask` tool questions, answered from an inline card.
    ask_card: Option<ask::AskCard>,
    /// Questions waiting behind `ask_card`, oldest first.
    queued_asks: VecDeque<ask::QueuedAsk>,
    ask_focus_return: Option<FocusHandle>,
    ask_recovery: Option<gpui::Task<()>>,
    answered_asks: HashSet<String>,
    unavailable_asks: HashMap<String, u64>,
    ask_attachment_id: u64,
    /// Scroll state of the queue card's rows, so an inline queue editor can
    /// bring its row into view when it opens.
    queue_items_scroll: gpui::ScrollHandle,
    queue_edit_anchor: gpui::ScrollAnchor,
    pending_ask_draft: Option<ask::AskDraft>,
    /// `/btw` answer streaming from a hidden fork of this conversation.
    side_question: Option<side_question::SideQuestionCard>,
    side_question_history: Option<side_question::HistoryView>,
    side_question_resize: crate::input::VerticalResize,
    composer_overlay_room: std::cell::Cell<gpui::Pixels>,
    model_picker_open: bool,
    picker_search: Option<(PickerKind, Entity<PromptInput>)>,
    picker_triggers: [FocusHandle; 4],
    composer_popover_watch: Option<(FocusHandle, gpui::Subscription, gpui::Subscription)>,
    composer_popover_trigger: Option<FocusHandle>,
    focus_pending: bool,
    /// Read-only transcript surface (resume preview): the chat renderer
    /// without composer, footer, or focus stealing.
    pub(crate) transcript_only: bool,
    /// Words from the session search that opened (or is previewing) this
    /// transcript, marked wherever they occur until the user sends a prompt.
    pub(crate) search_terms: Vec<String>,
    login: Option<login::LoginState>,
    available_models: Vec<String>,
}

/// What the user chose for a draft before its session exists. The runtime
/// hears about it together with the first prompt, never earlier.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct DraftConfig {
    model: Option<String>,
    effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speed: Option<jcode_provider_core::SpeedMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modes: Vec<String>,
}

impl DraftConfig {
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.effort.is_none()
            && self.speed.is_none()
            && self.modes.is_empty()
    }
}

/// Model routes the runtime last reported to any panel of this app. A draft
/// has no session to ask, so its model picker reads this instead.
#[derive(Default)]
struct KnownModelRoutes(Vec<jcode_sdk::ModelRouteInfo>);

impl gpui::Global for KnownModelRoutes {}

#[derive(Clone)]
struct LocalDraftModels {
    current: Option<String>,
    routes: Vec<jcode_sdk::ModelRouteInfo>,
}

impl gpui::Global for LocalDraftModels {}

struct RememberedDraftModel(crate::config::DraftModelDefaults);

impl gpui::Global for RememberedDraftModel {}

/// `/model` and `/effort` only record a choice, so a draft answers them
/// without a runtime session.
pub(crate) fn is_draft_config_command(content: &str) -> bool {
    matches!(
        content.split_whitespace().next(),
        Some("/model" | "/models" | "/effort" | "/mode" | "/speed")
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickerKind {
    Model,
    Effort,
    Mode,
    Context,
}

/// Panel/chrome notifications do not imply that settled message heights changed.
/// GPUI measures visible rows on every layout and invalidates all rows on width
/// changes. Only its offscreen measurements need explicit content invalidation.
#[derive(Default)]
struct TranscriptMeasurements {
    /// Advances on every content invalidation; rows are remeasured until
    /// `measured` has caught up with it.
    revision: u64,
    measured: u64,
    item_count: usize,
    row_count: usize,
    streaming_lengths: (usize, usize),
    fonts: Option<(&'static str, &'static str, &'static str)>,
    #[cfg(test)]
    remeasured_rows: usize,
}

impl TranscriptMeasurements {
    /// Settled content changed height somewhere: remeasure every row.
    fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn take_range(
        &mut self,
        item_count: usize,
        row_count: usize,
        streaming_lengths: (usize, usize),
        fonts: (&'static str, &'static str, &'static str),
    ) -> Option<std::ops::Range<usize>> {
        let range = if self.revision != self.measured
            || self.item_count != item_count
            || self.row_count != row_count
            || self.fonts != Some(fonts)
        {
            Some(0..row_count)
        } else if self.streaming_lengths != streaming_lengths {
            // At most reasoning, assistant text, and activity occupy the live
            // suffix. Settling/merging reasoning explicitly dirties history.
            Some(row_count.saturating_sub(3)..row_count)
        } else {
            None
        };
        self.measured = self.revision;
        self.item_count = item_count;
        self.row_count = row_count;
        self.streaming_lengths = streaming_lengths;
        self.fonts = Some(fonts);
        let range = range.filter(|range| !range.is_empty());
        #[cfg(test)]
        if let Some(range) = &range {
            self.remeasured_rows += range.len();
        }
        range
    }
}

#[cfg(test)]
#[path = "panel_measurement_tests.rs"]
mod measurement_tests;

#[cfg(test)]
#[path = "panel_sound_tests.rs"]
mod sound_tests;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MinimapSessionState {
    Idle,
    Working,
    Streaming,
    Complete,
    Error,
}

#[derive(Debug)]
enum TodoistLoadState {
    Loading,
    Ready,
    Error(String),
}

#[derive(Debug)]
struct TodoistPanelState {
    load: TodoistLoadState,
    tasks: Vec<TodoistTask>,
    projects: Vec<TodoistProject>,
    selected_project: Option<String>,
    busy_tasks: HashSet<String>,
    scroll: ScrollHandle,
    scroll_layout: Option<(usize, usize, Option<String>, gpui::Size<gpui::Pixels>)>,
}

impl TodoistPanelState {
    /// Offline geometry fixture shared by headless tests and native screenshots.
    fn fixture(count: usize) -> Self {
        Self {
            load: TodoistLoadState::Ready,
            tasks: (0..count)
                .map(|index| {
                    serde_json::from_value(serde_json::json!({
                        "id": format!("task-{index}"),
                        "content": format!("Task {index}: review the scrolling task list"),
                        "project_id": "inbox",
                        "priority": 1
                    }))
                    .expect("valid offline task")
                })
                .collect(),
            projects: Vec::new(),
            selected_project: None,
            busy_tasks: HashSet::new(),
            scroll: ScrollHandle::new(),
            scroll_layout: None,
        }
    }
}

#[path = "panel_publish.rs"]
mod publish_panel;

impl Panel {
    /// Activity painted over the tab. The session emoji stays still.
    pub(crate) fn tab_ring(&self) -> Option<TabRing> {
        // Only a live turn animates. `minimap_state` treats any non-idle
        // status as work, which would mark merely connected sessions.
        let state = match self.minimap_state() {
            MinimapSessionState::Error => MinimapSessionState::Error,
            _ if self
                .ask_card
                .as_ref()
                .is_some_and(|card| !card.submitting())
                || self
                    .routing_gate
                    .as_ref()
                    .is_some_and(|gate| !gate.view().submitting) =>
            {
                return Some(TabRing::question());
            }
            _ if self.activity_active() && self.has_live_wait_tool() => {
                return Some(TabRing::waiting());
            }
            _ if self.activity_active() => MinimapSessionState::Working,
            _ if self.tab_dependency_waiting || !self.awaiting_ids().is_empty() => {
                return Some(TabRing::waiting());
            }
            _ if self.finished_unseen => MinimapSessionState::Complete,
            _ => MinimapSessionState::Idle,
        };
        TabRing::resolve(state, self.latest_todo_progress())
    }

    fn has_live_wait_tool(&self) -> bool {
        self.items
            .iter()
            .rev()
            .take_while(|item| !matches!(item, Item::User(_)))
            .any(|item| {
                let Item::Tool {
                    name,
                    input,
                    done: false,
                    ..
                } = item
                else {
                    return false;
                };
                matches!(name.as_str(), "bg" | "swarm")
                    && serde_json::from_str::<serde_json::Value>(input).is_ok_and(|input| {
                        matches!(
                            (name.as_str(), input["action"].as_str()),
                            ("bg", Some("wait")) | ("swarm", Some("await_members"))
                        )
                    })
            })
    }

    pub(crate) fn tab_outline(&self) -> gpui::AnyView {
        self.tab_outline.clone().into()
    }

    /// Sidebar spinner for a session running in the shared daemon without an
    /// open panel. Created once per running session and reused while it runs.
    pub(crate) fn daemon_sidebar_mark(cx: &mut gpui::App) -> gpui::AnyView {
        cx.new(activity::Spinner::for_daemon_session).into()
    }

    #[cfg(test)]
    pub(crate) fn sidebar_mark(&self) -> Option<gpui::AnyView> {
        self.supports_voice()
            .then(|| self.sidebar_spinner.clone().into())
    }

    /// What the spinners of this panel read of it.
    fn activity_readings(owner: &Entity<Self>) -> activity::Readings {
        activity::Readings {
            activity: Live::of(owner, |panel: &Panel, _| panel.orb_activity()),
            status: Live::of(owner, |panel: &Panel, _| panel.status_line()),
        }
    }

    fn orb_activity(&self) -> activity::Activity {
        if self.activity_active() {
            activity::Activity::from_status_line(&self.status_line())
        } else {
            activity::Activity::Idle
        }
    }

    pub(crate) fn sidebar_activity(&self) -> Option<gpui::AnyView> {
        self.activity_active()
            .then(|| self.sidebar_spinner.clone().into())
    }

    /// Whether the regular conversation surface has anything that can consume
    /// a vertical scroll. Workspace gesture routing uses this to let an empty
    /// panel behave like bare canvas and move between strips instead.
    pub(crate) fn has_scrollable_conversation(&self) -> bool {
        self.is_default_directory()
            || self.is_machines()
            || self.is_change_review()
            || self.is_accounts_panel()
            || self.is_side_document()
            || self.applet.is_some()
            || self.todoist.is_some()
            || self.orchestration.is_some()
            || self.openrig.is_some()
            || self.transcript_row_count > 0
            || !self.streaming_text.is_empty()
            || !self.streaming_reasoning.is_empty()
    }

    pub const MACHINES_SESSION_ID: &str = "settings://machines";

    pub(crate) fn is_machines(&self) -> bool {
        self.session_id == Self::MACHINES_SESSION_ID
    }

    pub const DEFAULT_DIRECTORY_SESSION_ID: &str = "settings://default-directory";

    pub(crate) fn is_default_directory(&self) -> bool {
        self.session_id == Self::DEFAULT_DIRECTORY_SESSION_ID
    }

    pub const STARTUP_SESSION_ID: &str = "startup://draft";

    pub fn is_startup_draft(&self) -> bool {
        self.session_id == Self::STARTUP_SESSION_ID
    }

    pub(crate) fn is_pending_session_id(id: &str) -> bool {
        id == Self::STARTUP_SESSION_ID || id.starts_with("startup://draft/")
    }

    pub(crate) fn completion_context(&self) -> Vec<crate::prompt_completion::Turn> {
        completion_turns(&self.items)
    }

    pub(crate) fn is_pending_session(&self) -> bool {
        Self::is_pending_session_id(&self.session_id)
    }


    pub(crate) fn listable_conversation(&self, cx: &App) -> bool {
        !self.is_pending_session()
            || self.has_queued_prompts()
            || self.input.read(cx).has_user_draft()
    }


    pub(crate) fn can_fork(&self) -> bool {
        self.preview_state.is_none()
            && !self.is_default_directory()
            && !self.is_machines()
            && !self.is_change_review()
            && !self.is_accounts_panel()
            && !self.is_side_document()
            && self.session_id != "unfinished-work"
            && self.orchestration.is_none()
            && self.openrig.is_none()
            && !self.is_pending_session()
    }


    #[cfg(test)]
    pub(crate) fn composer_identity_for_test(&self) -> (String, Option<String>) {
        self.composer_identity()
    }


    fn latest_todo_payload(&self) -> Option<TodoCardPayload> {
        self.items.iter().rev().find_map(|item| match item {
            Item::Todos(payload) => Some(payload.clone()),
            Item::Tool {
                name,
                output,
                done: true,
                error: None,
                ..
            } if name == "todo" => parse_todo_tool_output(output),
            _ => None,
        })
    }

    pub(crate) fn uses_catalog_todos(&self) -> bool {
        self.latest_todo_card().is_none()
    }

    fn applet_session_tasks(&self, app: &App) -> crate::applet_view::SessionTasks {
        if let Some((rows, (completed, total))) = app
            .try_global::<crate::applet_view::CatalogTasks>()
            .and_then(|catalog| catalog.session(&self.session_id))
            && self.uses_catalog_todos()
        {
            return crate::applet_view::SessionTasks {
                rows: rows.iter().map(|(name, status)| vec![name.clone(), status.clone()]).collect(),
                completed,
                total,
            };
        }
        let fallback;
        let payload = if let Some(derived) = self.transcript_derived.as_ref()
            && derived.key.items_revision == self.items.revision()
        {
            derived.latest_todo.as_deref()
        } else {
            fallback = self.latest_todo_payload();
            fallback.as_ref()
        };
        let todos = payload.map_or(&[][..], |payload| payload.todos.as_slice());
        let total = todos.len();
        let completed = todos
            .iter()
            .filter(|todo| todo.status.eq_ignore_ascii_case("completed"))
            .count();
        let rows = todos
            .iter()
            .take(100)
            .map(|todo| vec![todo.content.clone(), todo.status.clone()])
            .collect();
        crate::applet_view::SessionTasks {
            rows,
            completed,
            total,
        }
    }

    fn open_model_picker(&mut self, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        if self.available_models.is_empty() {
            self.open_recovery_models(cx);
            return;
        }
        self.recovery_picker_open = false;
        self.model_picker_open = true;
        self.open_picker_search(PickerKind::Model, cx);
    }

    fn open_model_picker_deferred(&mut self, cx: &mut Context<Self>) {
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = panel.update(cx, |panel, cx| panel.open_model_picker(cx));
        });
    }

    pub(super) fn open_picker_search(&mut self, kind: PickerKind, cx: &mut Context<Self>) {
        self.swarm_list_open = false;
        self.select_background_job(None, cx);
        let settings = self.input.read(cx).picker_settings();
        let models = self.available_models.clone();
        let deferred = self.session_deferred();
        let submit = cx.weak_entity();
        let cancel = cx.weak_entity();
        let outside = cx.weak_entity();
        let speed = cx.weak_entity();
        let accepted = cx.weak_entity();
        let draft = self.input.downgrade();
        let search = cx.new(|cx| {
            let placeholder = match kind {
                PickerKind::Model => "Search models…",
                PickerKind::Effort => "Search effort…",
                PickerKind::Mode => "Search modes and skills • Tab toggles • Enter closes",
                PickerKind::Context => "Search files, skills and context…",
            };
            let mut input = PromptInput::new(cx, placeholder, move |content, _, _, app| {
                if let Some(panel) = submit.upgrade() {
                    panel.update(app, |panel, cx| {
                        if matches!(kind, PickerKind::Mode | PickerKind::Context) {
                            if content.starts_with("/mode toggle ") || content == "/mode off" {
                                if panel.session_deferred() {
                                    panel.handle_slash_command(&content, cx);
                                } else {
                                    panel.handle_runtime_command(&content, cx);
                                }
                            }
                            return;
                        }
                        if content.starts_with("/model ") || content.starts_with("/effort ") {
                            panel.handle_slash_command(&content, cx);
                        }
                        panel.picker_search = None;
                        panel.model_picker_open = false;
                        panel.composer_popover_trigger = None;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                }
            })
            .with_on_model_speed(move |choice, app| {
                let _ = speed.update(app, |panel, cx| panel.choose_model_speed(choice, cx));
            })
            .with_on_overlay_cancel(move |app| {
                if let Some(panel) = cancel.upgrade() {
                    panel.update(app, |panel, cx| {
                        panel.picker_search = None;
                        panel.model_picker_open = false;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                    true
                } else {
                    false
                }
            })
            .with_on_picker_outside(move |focus, window, app| {
                let _ = outside.update(app, |panel, cx| {
                    panel.dismiss_composer_popover(focus, true, window, cx);
                });
            });
            input.apply_picker_settings(settings, cx);
            input.set_pending_session(deferred, cx);
            input.set_speed_picker(self.speed_picker(), cx);
            input.start_picker_search(
                match kind {
                    PickerKind::Model => "/model",
                    PickerKind::Effort => "/effort",
                    PickerKind::Mode | PickerKind::Context => "/mode",
                },
                models,
                cx,
            );
            if kind == PickerKind::Context {
                input.set_skills(self.skills.clone(), cx);
                input.set_current_modes(self.mode_names().to_vec(), self.mode_catalog.clone(), cx);
                input.start_context_picker(self.composer_context_root(), draft, move |app| {
                    let _ = accepted.update(app, |panel, cx| {
                        panel.picker_search = None;
                        panel.composer_popover_trigger = None;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                }, cx);
            }
            input
        });
        self.picker_search = Some((kind, search));
        self.focus_pending = true;
        cx.notify();
    }

    fn composer_context_root(&self) -> Option<&std::path::Path> {
        if crate::harness::is_remote_session(&self.session_id) {
            return None;
        }
        self.working_dir.as_deref().map(std::path::Path::new).filter(|path| path.is_absolute())
    }

    fn composer_popover_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.picker_search
            .as_ref()
            .map(|(_, search)| search.read(cx).focus_handle.clone())
            .or_else(|| self.swarm_list_open.then(|| self.swarm_list_focus.clone()))
    }

    fn dismiss_composer_popover(
        &mut self,
        expected: &FocusHandle,
        restore_if_focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // An outside click can open a different picker. Never dismiss that one.
        if self.composer_popover_focus(cx).as_ref() != Some(expected) {
            return;
        }
        let restore = restore_if_focused && expected.contains_focused(window, cx);
        if self.picker_search.take().is_some() {
            self.model_picker_open = false;
        } else {
            self.swarm_list_open = false;
            self.select_background_job(None, cx);
        }
        self.composer_popover_trigger = None;
        if restore {
            self.focus_input(window, cx);
        }
        cx.notify();
    }

    fn watch_composer_popover_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer_popover_focus(cx);
        if self.composer_popover_watch.as_ref().map(|(f, _, _)| f) == focus.as_ref() {
            return;
        }
        self.composer_popover_watch = focus.map(|focus| {
            let blurred = focus.clone();
            let deactivated = focus.clone();
            let blur = cx.on_focus_out(&focus, window, move |panel, _, window, cx| {
                panel.dismiss_composer_popover(&blurred, false, window, cx);
            });
            let activation = cx.observe_window_activation(window, move |panel, window, cx| {
                if !window.is_window_active() {
                    panel.dismiss_composer_popover(&deactivated, true, window, cx);
                }
            });
            (focus, blur, activation)
        });
    }

    /// Toggle the `/effort` suggestion menu from the effort pill.
    pub(super) fn toggle_effort_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        if self
            .picker_search
            .as_ref()
            .is_some_and(|(kind, _)| *kind == PickerKind::Effort)
        {
            self.picker_search = None;
        } else {
            if self.model_picker_open {
                if self.picker_search.is_some() {
                    self.close_model_picker(cx);
                } else {
                    // A typed /model is still a draft, not a transient menu.
                    self.model_picker_open = false;
                }
            }
            self.login = None;
            self.recovery_picker_open = false;
            self.open_picker_search(PickerKind::Effort, cx);
        }
        self.focus_input(window, cx);
        cx.notify();
    }

    fn close_model_picker(&mut self, cx: &mut Context<Self>) {
        self.model_picker_open = false;
        if self
            .picker_search
            .as_ref()
            .is_some_and(|(kind, _)| *kind == PickerKind::Model)
        {
            self.picker_search = None;
            self.focus_pending = true;
        } else {
            let input = self.input.clone();
            cx.defer(move |cx| input.update(cx, |input, cx| input.close_model_menu(cx)));
        }
        cx.notify();
    }

    /// Ask the daemon to bookmark or unbookmark this session. The transcript
    /// note waits for [`Self::session_saved_settled`] so a rejected request
    /// never reads as saved.
    fn request_session_saved(&mut self, saved: bool, label: Option<String>) {
        if self.is_side_document() || self.preview_state.is_some() {
            return;
        }
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation: SessionOperation::SetSaved(saved, label),
        });
        self.pending_save_requests += 1;
    }

    /// Settle one pending `/save` or `/unsave`. Returns false when no request
    /// from this panel was waiting, e.g. a sidebar pin, so callers stay quiet.
    pub(crate) fn session_saved_settled(
        &mut self,
        saved: bool,
        label: Option<&str>,
        error: Option<&str>,
    ) -> bool {
        if self.pending_save_requests == 0 {
            return false;
        }
        self.pending_save_requests -= 1;
        self.items.push(match (error, saved, label) {
            (Some(error), true, _) => Item::Error(format!("Failed to save session: {error}")),
            (Some(error), false, _) => Item::Error(format!("Failed to unsave session: {error}")),
            (None, true, Some(label)) => Item::Assistant(format!(
                "📌 Session saved as \"{label}\". It will appear at the top of /resume."
            )),
            (None, true, None) => {
                Item::Assistant("📌 Session saved. It will appear at the top of /resume.".into())
            }
            (None, false, _) => Item::Assistant("Session removed from saved.".into()),
        });
        true
    }

    fn run_session_operation(&mut self, operation: SessionOperation, _message: impl Into<String>) {
        if self.is_side_document() || self.preview_state.is_some() {
            return;
        }
        let compact = matches!(operation, SessionOperation::Compact);
        self.bridge.send(Command::SessionOperation {
            session_id: self.session_id.clone(),
            operation,
        });
        if !compact {
            self.items.push(Item::Assistant(
                "Requested the session change. The conversation will refresh after the runtime confirms it.".into(),
            ));
        }
    }

    fn submit_command_prompt(&mut self, prompt: &str, cx: &mut Context<Self>) {
        if self.is_side_document() || self.preview_state.is_some() {
            return;
        }
        self.bridge.send(Command::Send {
            session_id: self.session_id.clone(),
            content: prompt.to_string(),
            images: Vec::new(),
            attachments: Vec::new(),
        });
        let index = self.items.len();
        self.items.push(Item::User(prompt.to_string()));
        self.stamp_user(index);
        self.pending_users.push_back(index);
        crate::sounds::play(crate::sounds::Cue::Sent, cx);
    }


    pub(crate) fn history_loaded(&self) -> bool {
        self.history_loaded
    }


    pub fn is_busy(&self) -> bool {
        self.status != "idle" || self.activity_active()
    }

    /// A compact, presentation-neutral summary for the workspace minimap.
    /// The transcript remains the source of truth, so todo progress keeps
    /// working across live updates without duplicating state in `Workspace`.
    pub(crate) fn minimap_state(&self) -> MinimapSessionState {
        let status = self.lowercase_status();
        if status.contains("error")
            || status.contains("crash")
            || matches!(
                self.items.last(),
                Some(Item::Error(_) | Item::RuntimeError { .. })
            )
        {
            MinimapSessionState::Error
        } else if !self.streaming_text.is_empty() || !self.streaming_reasoning.is_empty() {
            MinimapSessionState::Streaming
        } else if status != "idle" {
            MinimapSessionState::Working
        } else if self
            .latest_todo_progress()
            .is_some_and(|(completed, total)| total > 0 && completed == total)
        {
            MinimapSessionState::Complete
        } else {
            MinimapSessionState::Idle
        }
    }

    #[cfg(test)]
    pub(crate) fn test_set_todos(&mut self, json: &str, cx: &mut Context<Self>) {
        self.items
            .push(Item::Todos(parse_todo_tool_output(json).unwrap()));
        cx.notify();
    }

    fn latest_todo_card(&self) -> Option<&TodoCardPayload> {
        self.items.iter().rev().find_map(|item| match item {
            Item::Todos(payload) => Some(payload),
            _ => None,
        })
    }

    pub(crate) fn latest_todo_progress(&self) -> Option<(usize, usize)> {
        self.latest_todo_card()
            .map(|payload| {
                (
                    payload
                        .todos
                        .iter()
                        .filter(|todo| !todo.is_request() && todo.status == "completed")
                        .count(),
                    payload
                        .todos
                        .iter()
                        .filter(|todo| !todo.is_request() && todo.status != "cancelled")
                        .count(),
                )
            })
            .filter(|(_, total)| *total > 0)
    }


    /// Prefer meaningful activity over transport bookkeeping.
    fn status_line(&self) -> String {
        if self.continuing_after_reload && self.activity_active() && self.status != "compacting" {
            return "Interrupted by update, continuing…".into();
        }
        let phase = self.connection_phase.trim();
        if !phase.is_empty() && !matches!(phase, "connected" | "streaming") {
            return activity_state::phase_label(phase);
        }
        // Memory recall runs before the model request, so while it works its
        // step is the most specific thing this session is doing. Streaming,
        // reasoning and running tools take over as soon as they start.
        if self.activity_active()
            && !matches!(
                self.status.as_str(),
                "thinking" | "streaming" | "running_tools" | "compacting"
            )
            && let Some(activity) = self.memory_activity.as_ref()
            && let Some(label) = activity.status_label()
        {
            return label;
        }
        if self.activity_active() {
            // Buffers may still contain an earlier block. The latest semantic
            // event remains authoritative even after ReasoningDone flushes it.
            match self.status.as_str() {
                "thinking" => return "Thinking".into(),
                "streaming" => return "Responding".into(),
                "running_tools" => return "Running tools".into(),
                "compacting" => return "Compacting context".into(),
                _ => {}
            }
            if !self.streaming_reasoning.is_empty() {
                return "Thinking".into();
            }
            if !self.streaming_text.is_empty() {
                return "Responding".into();
            }
            if phase == "streaming" && self.status != "running_tools" {
                return "Responding".into();
            }
        }
        match self.status.as_str() {
            "idle" | "connected" => "Ready".into(),
            "busy" | "running" => "Working".into(),
            "generating" => "Working".into(),
            "thinking" => "Thinking".into(),
            "streaming" => "Responding".into(),
            "running_tools" => "Running tools".into(),
            "compacting" => "Compacting context".into(),
            status => status.replace('_', " "),
        }
    }

    fn lowercase_status(&self) -> std::borrow::Cow<'_, str> {
        // Chrome and transcript paths query this repeatedly each frame. Most
        // status snapshots are already lowercase and need no allocated copy.
        if self.status.bytes().any(|byte| byte.is_ascii_uppercase()) {
            std::borrow::Cow::Owned(self.status.to_ascii_lowercase())
        } else {
            std::borrow::Cow::Borrowed(self.status.as_str())
        }
    }

    fn activity_active(&self) -> bool {
        let status = self.lowercase_status();
        if status.contains("error") || status.contains("crash") || status.starts_with("lost:") {
            return false;
        }
        // Historical transcript errors do not describe the current turn. A
        // transport streaming event can also arrive before its session status.
        self.continuing_after_reload
            || !self.streaming_text.is_empty()
            || !self.streaming_reasoning.is_empty()
            || self.connection_phase.trim() == "streaming"
            || activity_state::is_request_phase(&self.connection_phase)
            || matches!(
                status.as_ref(),
                "generating"
                    | "running"
                    | "busy"
                    | "thinking"
                    | "streaming"
                    | "running_tools"
                    | "compacting"
            )
    }

    fn render_transcript_activity(&self) -> gpui::AnyElement {
        div()
            .debug_selector(|| "transcript-activity".into())
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .px_3()
            .pt(px(10.0))
            .pb_2()
            .children(crate::fixture_control_targets::marker(
                || "transcript-activity".into(),
                || Some(self.session_id.clone()),
            ))
            .child(self.activity_spinner.clone())
            .into_any_element()
    }


    pub fn focus_input(&self, window: &mut Window, cx: &mut App) {
        let handle = self.input_focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// Only neutral reading surfaces may hand typing back to the composer.
    /// Never redirect keys from another editor, picker, terminal or dialog.
    pub(crate) fn resume_composer_typing(
        &self,
        event: &gpui::KeyDownEvent,
        workspace_focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let key = &event.keystroke;
        if key.modifiers.control
            || key.modifiers.platform
            || key.modifiers.alt
            || !key
                .key_char
                .as_deref()
                .is_some_and(|text| !text.is_empty() && text.chars().all(|c| !c.is_control()))
            || self.transcript_only
            || self.ask_card.is_some()
            || self.routing_gate.is_some()
            || self.login.is_some()
            || self.recovery_picker_open
            || self.input_focus_handle(cx) != self.input.read(cx).focus_handle
            || !(workspace_focused
                || self
                    .transcript_selection
                    .read(cx)
                    .focus_handle()
                    .is_focused(window))
        {
            return false;
        }
        self.focus_input(window, cx);
        let input = self.input.clone();
        let text = key.key_char.clone().unwrap();
        // The input's on_change callback updates this panel synchronously.
        // Release the panel borrow before inserting the first character.
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                gpui::EntityInputHandler::replace_text_in_range(input, None, &text, window, cx);
            });
        });
        true
    }

    /// Whether keys typed now are delivered to this panel: its composer, its
    /// current primary surface, or its selectable transcript.
    pub(crate) fn workspace_focus_handles(&self, cx: &App) -> ([Option<FocusHandle>; 4], FocusHandle) {
        ([Some(self.input.read(cx).focus_handle.clone()), Some(self.input_focus_handle(cx)),
          self.image_pane_open.then(|| self.image_pane_focus.clone()),
          Some(self.transcript_selection.read(cx).focus_handle().clone())], self.focus_handle.clone())
    }

    pub(crate) fn workspace_surface_state(&self) -> (bool, bool, bool) {
        (self.surface_focused, self.finished_unseen, self.openrig.is_some())
    }

    pub(crate) fn owns_keyboard_focus(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle.is_focused(window)
            || self.input_focus_handle(cx).is_focused(window)
            || (self.image_pane_open && self.image_pane_focus.is_focused(window))
            || self.focus_handle.contains_focused(window, cx)
            || self
                .transcript_selection
                .read(cx)
                .focus_handle()
                .is_focused(window)
    }

    pub fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        if let Some(handle) = self.login_input_focus_handle(cx) {
            return handle;
        }
        if let Some((_, search)) = &self.picker_search {
            return search.read(cx).focus_handle.clone();
        }
        if let Some(handle) = self.document_focus_handle(cx) {
            return handle;
        }
        if self.image_preview.is_some() || self.diff_review.is_some() || self.is_accounts_panel() {
            self.focus_handle.clone()
        } else if self.unfinished_work.is_some()
            || self.orchestration.is_some()
            || self.openrig.is_some()
            || self.is_side_document()
            || self.applet.is_some()
        {
            // Read-only panels do not render their prompt input. Focusing that
            // detached handle prevents workspace actions such as FocusLeft from
            // bubbling through the rendered panel tree.
            self.focus_handle.clone()
        } else {
            self.input.read(cx).focus_handle.clone()
        }
    }

    /// Whether `set_surface_focused(focused)` would change anything. Callers
    /// drawing a frame check this first, since GPUI counts any update made
    /// while drawing as a change to the views reading it.
    pub(crate) fn needs_surface_update(&self, focused: bool) -> bool {
        self.surface_focused != focused || (focused && self.finished_unseen)
    }

    pub(crate) fn set_surface_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.surface_focused != focused {
            self.surface_focused = focused;
            cx.notify();
        }
        if focused && self.finished_unseen {
            // Viewing the session reads its result.
            self.finished_unseen = false;
            cx.notify();
        }
    }

    /// Give this panel a settled conversation, so its composer is docked
    /// over the transcript as it is in a real session.
    #[cfg(test)]
    pub(crate) fn test_settled_conversation(&mut self, lines: usize, cx: &mut Context<Self>) {
        self.startup_layout = None;
        *self.items = (0..lines)
            .map(|n| Item::Assistant(format!("message {n}")))
            .collect();
        cx.notify();
    }

    /// The transcript's vertical scroll offset, so tests can prove a gesture
    /// did or did not scroll it.
    #[cfg(test)]
    pub fn test_scroll_offset_y(&self) -> gpui::Pixels {
        self.pending_history_scroll
            .map(|(_, y)| px(y))
            .unwrap_or_else(|| self.transcript_list.scroll_px_offset_for_scrollbar().y)
    }

    #[cfg(test)]
    pub fn append_test_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.items.push(Item::Error(message.into()));
        cx.notify();
    }
}

/// Keep provider-level reasoning segments in one visual block. Some providers
/// emit `ReasoningDone` between segments even though they belong to the same
/// uninterrupted thinking phase.
impl Panel {
    /// A prompt's number counting the turns paged out of the loaded window.
    pub(super) fn absolute_prompt_number(&self, index: usize) -> Option<usize> {
        let offset = match &self.history_paging {
            Some(paging) => paging.prompt_offset(),
            None => Some(0),
        }?;
        prompt::prompt_number(&self.items, index).map(|number| number + offset)
    }

    /// The ask editor dictation is aimed at, when an ask card is showing one.
    pub(super) fn ask_voice_input(&self) -> Option<Entity<PromptInput>> {
        self.ask_card.as_ref()?.voice_input()
    }
}

fn append_reasoning(items: &mut Vec<Item>, text: String) {
    if let Some(Item::Reasoning(existing)) = items.last_mut() {
        append_reasoning_text(existing, &text);
    } else {
        items.push(Item::Reasoning(text));
    }
}


/// Session tabs and sidebar rows share one label, never an internal session ID.
pub(crate) fn folder_session_title(session_id: &str, title: &str) -> SharedString {
    custom_session_title(session_id, title)
        .unwrap_or("New session")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .into()
}

fn custom_session_title<'a>(session_id: &str, title: &'a str) -> Option<&'a str> {
    let title = title.trim();
    (!title.is_empty() && title != session_id && title != short_id(session_id)).then_some(title)
}

/// Give a new conversation an immediate, useful label while the agent is still
/// working out the more durable todo/plan title.
fn first_prompt_title(prompt: &str) -> Option<String> {
    const MAX_CHARS: usize = 64;
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || normalized.starts_with('/') {
        return None;
    }
    let mut chars = normalized.chars();
    let title = chars.by_ref().take(MAX_CHARS).collect::<String>();
    Some(if chars.next().is_some() {
        format!("{}…", title.trim_end())
    } else {
        title
    })
}

/// Defensive render-time grouping for transcripts assembled from more than one
/// event source. Streaming normally merges reasoning as it arrives, but a
/// reconnect or provider boundary can leave adjacent reasoning items behind.
/// They are one uninterrupted visual phase and should therefore paint as one
/// card. Keep the first index so expansion state remains stable.
#[cfg(test)]
fn coalesce_reasoning_rows(rows: Vec<(usize, Item)>) -> Vec<(usize, Item)> {
    let mut grouped: Vec<(usize, Item)> = Vec::with_capacity(rows.len());
    for (index, item) in rows {
        match (grouped.last_mut(), item) {
            (Some((_, Item::Reasoning(existing))), Item::Reasoning(text)) => {
                append_reasoning_text(existing, &text);
            }
            (_, item) => grouped.push((index, item)),
        }
    }
    grouped
}

fn append_reasoning_text(existing: &mut String, text: &str) {
    let separated = existing
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
        || text.chars().next().is_some_and(char::is_whitespace);
    if !separated {
        existing.push_str("\n\n");
    }
    existing.push_str(text);
}

impl Panel {
    /// Apply a streaming event addressed to this session.
    pub fn apply(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        let was_active = self.activity_active();
        self.apply_event(event, cx);
        if self.activity_active() && !classify::is_retry_phase(&self.connection_phase) {
            self.turn_outcome = None;
        }
        self.note_turn_transition(was_active, cx);
    }

    pub(super) fn apply_event(&mut self, event: &ApiEvent, cx: &mut Context<Self>) {
        if matches!(
            event,
            ApiEvent::TextDelta { .. }
                | ApiEvent::TextReplace { .. }
                | ApiEvent::ReasoningDelta { .. }
                | ApiEvent::ToolStart { .. }
                | ApiEvent::MessageAccepted { .. }
                | ApiEvent::Error { .. }
                | ApiEvent::TurnStopped { .. }
        ) {
            // New live evidence supersedes the earlier persisted candidate.
            self.reconnect_response = None;
        }
        // Streaming appends only affect the live suffix. Keep a conservative
        // full invalidation for tools, status transitions, images and errors.
        // Metadata-only events still repaint chrome without discarding heights.
        if !matches!(
            event,
            ApiEvent::TextDelta { .. }
                | ApiEvent::ReasoningDelta { .. }
                | ApiEvent::TokenUsage { .. }
                | ApiEvent::RequestUsage { .. }
                | ApiEvent::ContextOccupancy { .. }
                | ApiEvent::CacheKeepalive { .. }
                | ApiEvent::ModelInfo { .. }
                | ApiEvent::RuntimeInfo { .. }
                | ApiEvent::SessionRenamed { .. }
        ) {
            self.transcript_measurements.invalidate();
        }
        // Any event from the session proves the stream is back. A reattach
        // may resume events without a SessionConnected update, so the
        // `lost: …` banner must not outlive the outage.
        if self.status.starts_with("lost:") && !matches!(event, ApiEvent::Error { .. }) {
            self.status = "connected".into();
            self.queue_reconnected(cx);
        }
        self.response_stats.observe(event, self.provider.as_deref());
        self.observe_activity_state(event);
        self.observe_session_image_ids(event, cx);
        if matches!(event, ApiEvent::TurnDone { .. } | ApiEvent::TurnStopped { .. }) {
            self.dismiss_routing_gate(cx);
        }
        if let Some(cue) = self.sound_events.observe(event)
            && self.preview_state.is_none()
            && !self.demo
        {
            crate::sounds::play(cue, cx);
        }
        match event {
            ApiEvent::SessionRecovery { .. } => {
                self.continuing_after_reload = true;
            }
            ApiEvent::AskRequest {
                request_id,
                tool_call_id,
                questions,
                ..
            } => {
                self.continuing_after_reload = false;
                self.show_live_ask(request_id, tool_call_id, questions, cx);
            }
            ApiEvent::RoutingDecision {
                account,
                reason,
                warnings,
                pending,
                ..
            } => self.routing_decision(account, reason, warnings, pending.as_deref(), cx),
            ApiEvent::RoutingGate {
                gate_id,
                reason,
                options,
                ..
            } => self.open_routing_gate(gate_id, reason, options, cx),
            ApiEvent::RoutingGateClosed { gate_id, .. } => self.close_routing_gate(gate_id, cx),
            ApiEvent::PermissionRequest {
                tool_name,
                description,
                ..
            } => {
                self.pending_approval = Some(clean_chat::PendingApproval {
                    tool_name: tool_name.clone(),
                    description: description.clone(),
                });
            }
            ApiEvent::RuntimeCommandResult {
                session_id,
                command,
                data,
                message,
            } => {
                if *command == jcode_sdk::api::RuntimeCommand::BackgroundOutput {
                    if let (Some(task_id), Some(output)) =
                        (data["task_id"].as_str(), data["output"].as_str())
                    {
                        for item in self.items.iter_mut() {
                            match item {
                                Item::BackgroundTask {
                                    task_id: id,
                                    detail,
                                    ..
                                } if id == task_id => {
                                    *detail = Some(format!(
                                        "**Background task**\n\n```text\n{output}\n```"
                                    ));
                                }
                                Item::Tool {
                                    name,
                                    input,
                                    output: body,
                                    ..
                                } if name == "bg"
                                    && serde_json::from_str::<serde_json::Value>(input)
                                        .ok()
                                        .and_then(|v| {
                                            v.get("task_id")
                                                .and_then(|v| v.as_str())
                                                .map(str::to_owned)
                                        })
                                        .as_deref()
                                        == Some(task_id) =>
                                {
                                    let metadata = body
                                        .split_once("\n\nOutput preview:\n")
                                        .map(|(metadata, _)| metadata)
                                        .unwrap_or(body.as_str());
                                    *body = format!(
                                        "{metadata}\n\nOutput preview:\n```text\n{output}\n```"
                                    );
                                }
                                _ => {}
                            }
                        }
                        self.transcript_measurements.invalidate();
                        cx.notify();
                    }
                    return;
                }
                if *command == jcode_sdk::api::RuntimeCommand::Mode {
                    self.mode_multiselect = data["pinned_skills"].is_array();
                    self.pinned_modes = crate::mode_catalog::pinned_names(data, "pinned_skill");
                    // A modern daemon already applied these modes to its session.
                    // Do not send its compatibility primary back as a legacy replacement.
                    self.active_skill = if self.mode_multiselect {
                        None
                    } else {
                        self.pinned_modes.first().cloned()
                    };
                    self.sync_mode_menu(cx);
                    if message.is_empty() {
                        cx.notify();
                        return;
                    }
                }
                if *command == jcode_sdk::api::RuntimeCommand::Skills {
                    let skills = data["skills"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|skill| {
                            Some((
                                skill["name"].as_str()?.to_string(),
                                skill["description"].as_str().unwrap_or("").to_string(),
                            ))
                        })
                        .collect::<Vec<_>>();
                    self.skills = skills;
                    self.mode_catalog = data["skills"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|skill| {
                            Some((
                                skill["name"].as_str()?.to_owned(),
                                crate::mode_catalog::ModeMetadata {
                                    mode: skill["mode"].as_bool(),
                                    icon: skill["icon"].as_str().map(str::to_owned),
                                },
                            ))
                        })
                        .collect();
                    let multiselect = data["pinned_skills"].is_array();
                    if multiselect
                        && !self.mode_multiselect
                        && self.active_skill == self.pinned_modes.first().cloned()
                    {
                        self.active_skill = None;
                    }
                    self.mode_multiselect = multiselect;
                    self.pinned_modes = crate::mode_catalog::pinned_names(data, "active_skill");
                    if !multiselect && self.active_skill.is_none() {
                        self.active_skill = self.pinned_modes.first().cloned();
                    }
                    self.sync_mode_menu(cx);
                    cx.notify();
                    if message.is_empty() {
                        return;
                    }
                }
                let output = jcode_sdk::api::RuntimeCommandOutput {
                    session_id: session_id.clone(),
                    command: *command,
                    data: data.clone(),
                    message: message.clone(),
                };
                self.items.push(Item::Assistant(output.message.clone()));
                self.apply_special_command_artifacts(&output, cx);
                self.stick_to_bottom = true;
            }
            ApiEvent::PeerUserMessage { content, .. } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.stamp_user(self.items.len());
                self.items.push(Item::User(content.clone()));
                self.stick_to_bottom = true;
            }
            ApiEvent::AgentMessage {
                from_session,
                from_name,
                message,
                ..
            } => {
                self.flush_reasoning();
                self.flush_streaming();
                self.items.push(Item::AgentMessage {
                    from_session: from_session.clone(),
                    from_name: from_name.clone(),
                    message: message.clone(),
                    timestamp_ms: response_stats::now_ms(),
                });
            }
            ApiEvent::SwarmWorkerNotice { message, .. } => {
                self.flush_reasoning();
                self.flush_streaming();
                self.items.push(Item::SwarmWorkerNotice(message.clone()));
            }
            ApiEvent::MessageAccepted { .. } => {
                acknowledge_next(
                    &mut self.pending_users,
                    &mut self.accepted_users,
                    Instant::now(),
                );
            }
            ApiEvent::TextDelta { text, message_id, .. } => {
                self.append_assistant_delta(text, message_id.as_deref());
            }
            ApiEvent::TextDone { message_id, .. } => {
                self.close_assistant_output(message_id.as_deref());
            }
            ApiEvent::TextReplace { text, message_id, .. } => {
                self.replace_assistant_output(message_id.as_deref(), text);
            }
            ApiEvent::ReasoningDelta { text, .. } => {
                // Thinking that resumes after answer text is a new phase. Settle
                // the text first so the thought paints below it instead of
                // merging into the earlier thinking block above the text.
                self.flush_streaming();
                self.continue_after_closed_output();
                self.streaming_reasoning.push_str(text);
            }
            ApiEvent::ReasoningDone { .. } => {
                self.flush_reasoning();
            }
            ApiEvent::ToolStart { call_id, name, .. } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.arriving_tools.insert(call_id.clone(), Instant::now());
                self.tool_started.insert(call_id.clone(), Instant::now());
                self.tool_wall_times
                    .insert(call_id.clone(), (response_stats::now_ms(), None));
                self.items.push(Item::Tool {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    input: String::new(),
                    output: String::new(),
                    done: false,
                    error: None,
                });
            }
            ApiEvent::ToolInputDelta { call_id, delta, .. } => {
                if let Some(Item::Tool { input, name, .. }) = self.find_tool(call_id) {
                    input.push_str(delta);
                    if name == "ask" {
                        self.schedule_ask_recovery(cx);
                    }
                }
            }
            ApiEvent::ToolExec { call_id, name, .. } => {
                if self
                    .pending_approval
                    .as_ref()
                    .is_some_and(|approval| approval.tool_name == *name)
                {
                    self.pending_approval = None;
                }
                if name == "ask" {
                    self.schedule_ask_recovery(cx);
                }
                self.tool_started.insert(call_id.clone(), Instant::now());
                self.tool_wall_times
                    .insert(call_id.clone(), (response_stats::now_ms(), None));
                let key = format!("clock:{call_id}");
                self.running_tool_tickers
                    .get_mut()
                    .retain(|existing| existing != key);
            }
            ApiEvent::ToolDone {
                call_id,
                name,
                output,
                error,
                codemode,
                ..
            } => {
                if classify::is_codemode(name) && let Some(summary) = codemode {
                    self.codemode_summaries
                        .insert(call_id.clone(), summary.clone());
                }
                self.tool_wall_times
                    .entry(call_id.clone())
                    .or_insert((None, None))
                    .1 = response_stats::now_ms();
                let completed_ask = self.ask_card.as_ref().is_some_and(|card| {
                    name == "ask"
                        && (card.tool_call_id == *call_id
                            || (card.tool_call_id.is_empty()
                                && card.request_id.strip_prefix("ask-") == Some(call_id)))
                });
                let ask_input = if completed_ask {
                    self.cancel_ask_voice(cx);
                    let card = self.ask_card.take().expect("checked above");
                    self.ask_focus_return = card.focus_scope.clone();
                    let was_reload = output.contains("reload")
                        || error
                            .as_deref()
                            .is_some_and(|error| error.contains("reload"));
                    self.pending_ask_draft = was_reload.then(|| card.draft(cx));
                    let input = serde_json::json!({"questions": card.questions}).to_string();
                    Some(input)
                } else {
                    None
                };
                if let Some(Item::Tool {
                    input,
                    done,
                    error: slot,
                    output: output_slot,
                    ..
                }) = self.find_tool(call_id)
                {
                    *done = true;
                    *slot = error.clone();
                    *output_slot = output.clone();
                    if input.is_empty()
                        && let Some(ask_input) = &ask_input
                    {
                        *input = ask_input.clone();
                    }
                    if let Some(started) = self.tool_started.remove(call_id) {
                        self.tool_durations
                            .insert(call_id.clone(), started.elapsed());
                    }
                    let running = &self.tool_started;
                    self.running_tool_tickers.get_mut().retain(|key| {
                        key.split_once(':')
                            .is_some_and(|(_, call)| running.contains_key(call))
                    });
                } else {
                    self.arriving_tools.insert(call_id.clone(), Instant::now());
                    self.items.push(Item::Tool {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        input: ask_input.unwrap_or_default(),
                        output: output.clone(),
                        done: true,
                        error: error.clone(),
                    });
                }
                // Any finished tool retires the questions it was waiting on,
                // including computer-use consent prompts.
                self.forget_queued_ask(call_id);
                if name == "ask" {
                    self.show_next_queued_ask(cx);
                    self.schedule_ask_recovery(cx);
                }
            }
            ApiEvent::Compacted { message, .. } => {
                self.context_usage.compacting = true;
                // Older daemons include a multi-line context report in the acknowledgement.
                let label = if message.lines().count() == 1 {
                    message.as_str()
                } else {
                    "Compacting context…"
                };
                self.show_compaction_running(label);
            }
            ApiEvent::MemoryInjected { content, .. } => {
                // The daemon stores the same row it sends here, so the live
                // line and the reloaded one come from one body. Never coalesce
                // repeated injections: each delivery is part of the timeline.
                if !content.trim().is_empty() {
                    self.flush_reasoning();
                    self.flush_streaming();
                    self.items
                        .push(Item::Memory(compaction::CompactionRow::complete(
                            content.clone(),
                        )));
                }
            }
            ApiEvent::MemoryActivity { activity, .. } => {
                // Activity is chrome, not transcript: keep the latest snapshot
                // only while the pipeline is doing something.
                self.memory_activity = (!activity.is_idle()).then(|| activity.clone());
            }
            // A compaction started or finished. The divider replaces the old
            // transient "Context compaction" card: it is one rule line, it
            // carries the method/token/duration label, and the completed row is
            // also written to session history so it survives a reload.
            ApiEvent::CompactionDivider {
                content,
                running,
                pre_tokens,
                post_tokens,
                trigger,
                ..
            } => {
                self.context_usage.compacting = *running;
                if *running {
                    if matches!(trigger.as_deref(), None | Some("manual")) {
                        self.manual_compaction_started();
                    }
                    self.show_compaction_running(content.clone());
                } else {
                    self.finish_compaction_divider(content.clone());
                    // The composer's context figure described the pre-compaction
                    // transcript until the next provider report; adopt the
                    // daemon's measured post-compaction size now.
                    if let Some(post_tokens) = post_tokens {
                        self.context_tokens = Some(*post_tokens);
                        self.set_compacted_context_estimate(*post_tokens);
                    }
                    if matches!(trigger.as_deref(), None | Some("manual")) {
                        self.finish_manual_context_compaction(
                            trigger.as_deref() == Some("manual"),
                            *pre_tokens,
                            *post_tokens,
                            cx,
                        );
                    } else {
                        self.refresh_context_usage(cx);
                    }
                }
            }
            ApiEvent::BackgroundTasks { snapshot, .. } => {
                self.apply_background_tasks(snapshot, cx);
                return;
            }
            ApiEvent::BackgroundProgress {
                task_id,
                summary,
                done: true,
                ..
            } if task_id == "context-compaction" => {
                self.context_usage.compacting = false;
                self.fail_compaction(summary);
                let reason = summary.lines().rev().find(|line| !line.trim().is_empty());
                self.settle_context_action(
                    context_usage::Action::Compact,
                    reason
                        .unwrap_or(summary)
                        .trim_start_matches("⚠ ")
                        .replace("**", ""),
                    true,
                    cx,
                );
            }
            ApiEvent::BackgroundProgress {
                task_id,
                label,
                summary,
                detail,
                percent,
                done,
                ..
            } => {
                if let Some(Item::BackgroundTask {
                    label: current_label,
                    summary: current_summary,
                    detail: current_detail,
                    percent: current_percent,
                    done: current_done,
                    ..
                }) = self.items.iter_mut().rev().find(|item| {
                    matches!(item, Item::BackgroundTask { task_id: id, .. } if id == task_id)
                }) {
                    *current_label = label.clone();
                    *current_summary = summary.clone();
                    if detail.is_some() {
                        *current_detail = detail.clone();
                    }
                    *current_percent = *percent;
                    *current_done = *done;
                } else {
                    self.items.push(Item::BackgroundTask {
                        task_id: task_id.clone(),
                        label: label.clone(),
                        summary: summary.clone(),
                        detail: detail.clone(),
                        percent: *percent,
                        done: *done,
                    });
                }
            }
            ApiEvent::SidePaneImages { images, .. } => {
                self.note_session_image_ordinal(images.iter().filter_map(|image| image.session_image.as_ref()).map(|image| image.ordinal).max(), cx);
                for image in images.iter().cloned() {
                    self.insert_rendered_image(image);
                }
            }
            ApiEvent::TurnDone { .. } => {
                self.refresh_context_usage(cx);
                self.reconcile_reconnect_response();
                self.flush_reasoning();
                self.flush_streaming();
                let outcome = self.finished_turn_outcome();
                self.settle_assistant_output(if outcome == turn::TurnOutcome::Done {
                    AssistantOutputPhase::Final
                } else {
                    AssistantOutputPhase::Update
                });
                self.finish_response();
                self.turn_outcome = Some(outcome);
            }
            ApiEvent::ProviderGuardrailRetry {
                turn_id,
                attempt,
                max_attempts,
                ..
            } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                let text = format!(
                    "Provider stopped the response. Jcode retried the request (attempt {attempt} of {max_attempts})."
                );
                if let Some(Item::ProviderGuardrailRetry { text: current, .. }) =
                    self.items.iter_mut().rev().find(|item| {
                        matches!(item, Item::ProviderGuardrailRetry { turn_id: id, .. } if id == turn_id)
                    })
                {
                    *current = text;
                } else {
                    self.items.push(Item::ProviderGuardrailRetry {
                        turn_id: turn_id.clone(),
                        text,
                    });
                }
            }
            ApiEvent::TurnStopped {
                reason,
                message,
                provider_stop_reason,
                ..
            } => {
                self.record_stop(turn::StopNotice::from_event(
                    reason,
                    message,
                    provider_stop_reason.as_deref(),
                ));
                self.dismiss_stopped_asks(cx);
            }
            ApiEvent::SessionStatus { status, .. }
                if status == "attached" || (status == "connected" && self.activity_active()) =>
            {
                // Transport bookkeeping is not a turn transition. In particular,
                // a late attach notification must not resurrect a completed turn.
            }
            ApiEvent::SessionStatus { status, .. } => {
                if let Some(notice) = turn::StopNotice::from_status(status) {
                    // New runtimes send a richer event before their legacy status.
                    if !matches!(self.items.last(), Some(Item::Stopped(notice)) if !notice.provisional)
                    {
                        self.record_stop(notice);
                    }
                    self.dismiss_stopped_asks(cx);
                    cx.notify();
                    return;
                }
                self.status = if status == "processing" {
                    "running".into()
                } else {
                    status.clone()
                };
                // The attachment can report idle before the daemon claims
                // recovery. A terminal event, not that snapshot, ends recovery.
                if status == "idle" && !self.continuing_after_reload {
                    self.reconcile_reconnect_response();
                    self.flush_reasoning();
                    self.settle_idle_output();
                    self.finish_response();
                }
            }
            ApiEvent::ConnectionPhase { phase, .. } => {
                self.connection_phase = phase.clone();
            }
            ApiEvent::ModelSwitch {
                state,
                model,
                error,
                ..
            } => self.apply_model_switch(*state, model.as_deref(), error.as_deref(), cx),
            ApiEvent::ModelInfo {
                provider,
                model,
                reasoning_effort,
                auth_method,
                account_label,
                ..
            } => {
                let previous_route = (
                    self.provider.clone(),
                    self.model.clone(),
                    self.auth_method.clone(),
                    self.account_label.clone(),
                );
                if provider.is_some() {
                    self.provider = provider.clone();
                }
                // A known model makes an absent effort authoritative. A wholly
                // empty identity does not erase the persisted fallback.
                if model.is_some() || reasoning_effort.is_some() {
                    self.identity_effort(reasoning_effort.clone());
                }
                // Only an actual switch invalidates the auth method: the route
                // catalog keyed it by model, but effort broadcasts repeat the
                // current model and must not wipe a still-correct label.
                if model.is_some() && *model != self.model {
                    self.model = model.clone();
                    self.auth_method = None;
                    self.account_label = None;
                }
                // The daemon's resolved credential is authoritative, including
                // an OAuth<->API key switch that keeps the same model id.
                if let Some(method) = auth_method.as_deref() {
                    self.auth_method = Some(human_auth_method(method));
                    // A resolved credential report is authoritative even when
                    // it clears a previous named OAuth account for an API key.
                    self.account_label = account_label.clone();
                } else if account_label.is_some() {
                    self.account_label = account_label.clone();
                }
                if previous_route
                    != (
                        self.provider.clone(),
                        self.model.clone(),
                        self.auth_method.clone(),
                        self.account_label.clone(),
                    )
                {
                    self.invalidate_context_meter();
                    self.refresh_context_usage(cx);
                    self.invalidate_speed_route(cx);
                    let update = |input: &mut PromptInput, cx: &mut Context<PromptInput>| {
                        input.set_serving_model(
                            self.model.as_deref(),
                            self.provider.as_deref(),
                            self.auth_method.as_deref(),
                            cx,
                        );
                    };
                    self.input.update(cx, update);
                    if let Some((PickerKind::Model, search)) = &self.picker_search {
                        search.update(cx, update);
                    }
                }
                self.sync_effort_menu(cx);
            }
            ApiEvent::RuntimeInfo {
                provider,
                model,
                reasoning_effort,
                auth_method,
                account_label,
                routes,
                speed_mode,
                speed_options,
                ..
            } => {
                if provider.is_some() {
                    self.provider = provider.clone();
                }
                // An empty runtime snapshot carries no identity yet. Keep the
                // persisted effort until the runtime actually reports a model.
                if model.is_some() || reasoning_effort.is_some() {
                    self.identity_effort(reasoning_effort.clone());
                }
                if model.is_some() {
                    self.model = model.clone();
                }
                self.account_label = account_label.clone();
                self.auth_method = auth_method
                    .as_deref()
                    .map(human_auth_method)
                    .or_else(|| auth_method_for_model(self.model.as_deref(), routes));
                if !routes.is_empty() {
                    cx.set_global(KnownModelRoutes(routes.clone()));
                }
                let models = available_model_names(routes);
                self.available_models = models.clone();
                let current = self
                    .model
                    .as_ref()
                    .and_then(|model| {
                        let mut matching = routes.iter().filter(|route| {
                            route.available
                                && route.model == *model
                                && self
                                    .provider
                                    .as_ref()
                                    .is_none_or(|provider| route.provider == *provider)
                                && self.auth_method.as_ref().is_none_or(|auth| {
                                    human_auth_method(&route.api_method) == *auth
                                })
                        });
                        let route = matching.next()?;
                        matching
                            .next()
                            .is_none()
                            .then(|| crate::input::model_menu::route_spec(route))
                    })
                    .or_else(|| self.model.clone());
                let update = |input: &mut PromptInput, cx: &mut Context<PromptInput>| {
                    input.set_model_routes(models.clone(), routes, current.clone(), cx);
                };
                self.input.update(cx, update);
                if let Some((PickerKind::Model, search)) = &self.picker_search {
                    search.update(cx, update);
                }
                self.sync_effort_menu(cx);
                self.update_speed(*speed_mode, speed_options, cx);
            }
            ApiEvent::SpeedModeChanged { speed_mode, speed_options, .. } => {
                self.update_speed(*speed_mode, speed_options, cx);
            }
            ApiEvent::SpeedModeReported { requested, actual, .. } => {
                self.record_speed_report(*requested, *actual, cx);
            }
            ApiEvent::TokenUsage {
                input,
                cache_read_input,
                cache_creation_input,
                ..
            } => {
                self.context_tokens =
                    Some(jcode_base::compaction::effective_context_tokens_from_usage(
                        self.provider.as_deref().unwrap_or_default(),
                        *input,
                        *cache_read_input,
                        *cache_creation_input,
                    ));
            }
            // The daemon replays the persisted prompt occupancy when a session
            // attaches. It describes a request that completed before this
            // attachment, so it is context state only: no turn stats, no cost,
            // and no effect on the live stream counters.
            ApiEvent::ContextOccupancy { prompt_tokens, .. } => {
                self.context_tokens = Some(*prompt_tokens);
            }
            ApiEvent::ContextCapChanged { .. } => self.refresh_context_usage(cx),
            ApiEvent::RequestUsage { .. } | ApiEvent::CacheKeepalive { .. } => {
                self.cache_telemetry.observe(event)
            }
            ApiEvent::KvCacheMiss { .. } => {
                if let Some(notice) = cache_miss::CacheMissNotice::from_event(event) {
                    self.cache_telemetry.miss(&notice);
                    // Keep the notice in order: settle text streamed so far.
                    self.flush_reasoning();
                    self.flush_streaming();
                    self.items.push(Item::CacheMiss(notice));
                }
            }
            ApiEvent::SessionRenamed { display_title, .. } => {
                self.title = display_title.clone().into();
            }
            ApiEvent::Error { code, message } => {
                self.flush_reasoning();
                self.settle_assistant_output(AssistantOutputPhase::Update);
                self.finish_response();
                self.turn_outcome = Some(turn::TurnOutcome::Error);
                if !matches!(self.items.last(), Some(Item::Stopped(notice)) if notice.detail == *message)
                {
                    self.items.push(Item::RuntimeError {
                        code: *code,
                        message: message.clone(),
                    });
                }
            }
            ApiEvent::SoftInterruptInjected { .. } => {
                self.settle_assistant_output(AssistantOutputPhase::Update);
            }
            _ => {}
        }
        self.observe_prompt_queue(event, cx);
        // Follow only during render, after pending user input. Installing an
        // end sentinel here replaces the painted scroll position before layout,
        // so an intervening upward wheel delta is clamped back to the bottom.
        cx.notify();
    }

    /// A turn that finishes out of view leaves a green "done" glow on its
    /// tab until the session is seen. A new turn clears it right away.
    pub(super) fn note_turn_transition(&mut self, was_active: bool, cx: &mut Context<Self>) {
        let active = self.activity_active();
        let finished_unseen = match (was_active, active) {
            (false, true) => false,
            (true, false) => {
                !self.surface_focused && self.minimap_state() != MinimapSessionState::Error
            }
            _ => return,
        };
        if self.finished_unseen != finished_unseen {
            self.finished_unseen = finished_unseen;
            cx.notify();
        }
    }

    /// Settle partial output and stop activity without waiting for a later idle event.
    pub(super) fn finish_response(&mut self) {
        self.continuing_after_reload = false;
        self.pending_approval = None;
        self.transcript_measurements.invalidate();
        self.flush_reasoning();
        self.flush_streaming();
        if let Some(stats) = self.response_stats.finish() {
            self.items.push(Item::ResponseStats(stats));
        }
        self.status = "idle".into();
        self.connection_phase.clear();
    }

    /// Advance the paced reveal of live reasoning and response text. Only the
    /// visible prefix is rendered, so bursts flow in instead of jumping.
    /// Returns true when motion is instant this frame (reduced motion, tests).
    pub(super) fn tick_stream_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // Render tests assert on painted text from a single frame.
        #[cfg(test)]
        let test_snap = !self.animate_stream_in_tests;
        #[cfg(not(test))]
        let test_snap = false;
        let instant = test_snap
            || self.preview_state.is_some()
            || cx.reduce_motion()
            || crate::config::get().appearance.reduce_motion;
        let now = cx.background_executor().now();
        let text = self.text_reveal.tick(&self.streaming_text, now, instant);
        let reasoning = self
            .reasoning_reveal
            .tick(&self.streaming_reasoning, now, instant);
        if text || reasoning {
            window.request_animation_frame();
        }
        instant
    }

    /// Raw streamed input of a tool call in this transcript.
    pub(crate) fn tool_input(&self, call_id: &str) -> Option<&str> {
        self.items.iter().rev().find_map(|item| match item {
            Item::Tool {
                call_id: id, input, ..
            } if id == call_id => Some(input.as_str()),
            _ => None,
        })
    }

    pub(super) fn find_tool(&mut self, call_id: &str) -> Option<&mut Item> {
        // The legacy harness protocol streams `tool_input` without an id. The
        // bridge preserves that fact as an empty call_id, so associate those
        // deltas with the most recent active call. Exact ids still win for
        // modern providers and for overlapping completed calls.
        if call_id.is_empty() {
            return self
                .items
                .iter_mut()
                .rev()
                .find(|item| matches!(item, Item::Tool { done: false, .. }));
        }
        self.items.iter_mut().rev().find(
            |item| matches!(item, Item::Tool { call_id: existing, .. } if existing == call_id),
        )
    }

    pub(super) fn insert_rendered_image(&mut self, image: jcode_sdk::RenderedImage) {
        // Side-pane events have no user-message boundary. Pasted images arrive
        // with the local echo or as part of history, never as tool side effects.
        if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
            return;
        }
        if self.items.iter().any(|item| {
            matches!(item, Item::Image(existing)
                if existing.anchor == image.anchor && existing.source == image.source
                    && match (&existing.session_image, &image.session_image) {
                        (Some(existing), Some(incoming)) => existing.image_id == incoming.image_id,
                        _ => existing.media_type == image.media_type && existing.data == image.data,
                    }
                    && (!matches!(&image.source, jcode_sdk::RenderedImageSource::ToolResult { tool_name } if tool_name == "ask") || existing.label == image.label))
        }) {
            return;
        }
        let insertion = match image.anchor.as_ref() {
            Some(jcode_sdk::RenderedImageAnchor::ToolCall { id }) => self
                .items
                .iter()
                .rposition(|item| matches!(item, Item::Tool { call_id, .. } if call_id == id))
                .map(|index| {
                    // A batch may return several images. Preserve their order
                    // instead of inserting each one directly after the tool.
                    index + 1
                        + self.items[index + 1..]
                            .iter()
                            .take_while(|item| {
                                matches!(item, Item::Image(existing) if existing.anchor == image.anchor)
                            })
                            .count()
                }),
            _ => None,
        };
        let insertion = insertion.unwrap_or_else(|| {
            // Unanchored images still belong after already-streamed text,
            // not before it when the stream is eventually flushed.
            self.flush_reasoning();
            self.flush_streaming();
            self.items.len()
        });
        self.insert_transcript_image(insertion, TranscriptImage::from_rendered(image));
    }

    pub(super) fn insert_transcript_image(&mut self, insertion: usize, image: TranscriptImage) {
        for index in &mut self.pending_users {
            if *index >= insertion {
                *index += 1;
            }
        }
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(index, at)| (index + usize::from(index >= insertion), at))
            .collect();
        self.user_times = std::mem::take(&mut self.user_times)
            .into_iter()
            .map(|(index, at)| (index + usize::from(index >= insertion), at))
            .collect();
        let shift = |index| index + usize::from(index >= insertion);
        self.offscreen_prompt = self.offscreen_prompt.map(shift);
        self.expanded_pinned_prompt = self.expanded_pinned_prompt.map(shift);
        if let Some(selected) = &mut self.image_pane_selected
            && *selected >= insertion
        {
            *selected += 1;
        }
        self.items.insert(insertion, Item::Image(image));
    }

    pub(super) fn flush_reasoning(&mut self) {
        if !self.streaming_reasoning.trim().is_empty() {
            self.transcript_measurements.invalidate();
            append_reasoning(
                &mut self.items,
                std::mem::take(&mut self.streaming_reasoning),
            );
        } else {
            self.streaming_reasoning.clear();
        }
    }

    pub fn message_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.prompt_queue.paused = true;
        self.sound_events.cancel();
        if self.preview_state.is_none() {
            crate::sounds::play(crate::sounds::Cue::Error, cx);
        }
        self.finish_response();
        self.turn_outcome = Some(turn::TurnOutcome::Error);
        self.items.push(Item::Error(message));
        self.status = "idle".into();
        self.connection_phase.clear();
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        cx.notify();
    }
}

impl Panel {
    /// Refresh transcript applet placement from the shared runtime. Cards
    /// anchored to a tool call render in that call's row. Everything else
    /// (`end`, `after_message`, and tool anchors whose call is not in this
    /// transcript, such as batch subcalls) is pinned at the position where
    /// it first appeared so it scrolls with the conversation.
    pub(super) fn sync_applet_rows(&mut self, cx: &mut Context<Self>) {
        use jcode_applet_types::{Placement, placement::Anchor};
        let runtime = crate::applet_runtime::get(cx);
        let generation = runtime.generation.get();
        let clean = self.clean_chat_enabled();
        let key = (generation, self.items.revision(), clean);
        if key == self.applet_generation {
            return;
        }
        self.applet_generation = key;
        let host = runtime.host.borrow();
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        let mut receipts = None;
        for mounted in host.inline_for(&self.session_id) {
            let id = mounted.instance.id.clone();
            if let Placement::Inline {
                anchor: Anchor::ToolCall { call_id },
                ..
            } = &mounted.instance.placement
            {
                if clean
                    && let Some(index) = self.items.iter().position(
                        |item| matches!(item, Item::Tool { call_id: c, .. } if c == call_id),
                    )
                {
                    rows.push((index, id));
                }
                continue;
            }
            // Older snapshots only stored End. Recover the original mount, not a later update.
            let position = if mounted.instance.applet == jcode_applet_types::agent::APPLET_ID {
                let origins = receipts.get_or_insert_with(|| {
                    let mut origins = HashMap::new();
                    for (index, item) in self.items.iter().enumerate() {
                        if let Item::Tool { name, output, .. } = item
                            && name == "applet"
                            && let Some((local, rest)) = output
                                .trim_start()
                                .strip_prefix("Applet `")
                                .and_then(|s| s.split_once("` revision "))
                            && rest.starts_with("1.")
                        {
                            origins
                                .entry(crate::applet_runtime::agent_instance_id(
                                    &self.session_id,
                                    local,
                                ))
                                .or_insert(index + 1);
                        }
                    }
                    origins
                });
                let Some(&position) = origins.get(&id) else {
                    continue;
                };
                position
            } else {
                *self
                    .applet_positions
                    .entry(id.clone())
                    .or_insert(self.items.len())
            };
            seen.insert(id.clone());
            rows.push((position, id));
        }
        drop(host);
        // The panel snapshot may arrive before the first runtime snapshot.
        if !rows.is_empty() || !seen.is_empty() {
            self.applet_positions.retain(|id, _| seen.contains(id));
        }
        rows.sort_by_key(|(position, _)| *position);
        if rows.is_empty() && self.applet_rows.is_empty() {
            return;
        }
        // Any card change (mount, patch, close) can change row heights.
        self.transcript_measurements.invalidate();
        self.applet_rows = rows;
        self.applet_rows_version = self.applet_rows_version.wrapping_add(1);
    }

    /// Focus and scroll handles are created when a tool or agent-message row
    /// first renders. Drop the ones whose row left the transcript, so a history
    /// reload or rewind does not leave a handle behind for every past call.
    pub(super) fn prune_row_state(&self) {
        if self.swarm_row_focus.borrow().is_empty() && self.swarm_scrolls.borrow().is_empty() {
            return;
        }
        let mut live: std::collections::HashSet<std::borrow::Cow<str>> = Default::default();
        for (index, item) in self.items.iter().enumerate() {
            match item {
                Item::Tool { call_id, .. } => {
                    live.insert(call_id.as_str().into());
                }
                Item::AgentMessage { .. } => {
                    live.insert(expandable::Key::details(index).into());
                }
                _ => {}
            }
        }
        self.swarm_row_focus
            .borrow_mut()
            .retain(|key, _| live.contains(key.as_str()));
        self.swarm_scrolls
            .borrow_mut()
            .retain(|key, _| live.contains(key.as_str()));
    }

    /// Rows, pinnable prompt rows and the latest todo snapshot, rebuilt only
    /// when the transcript, live text, applet rows or disclosure state change.
    /// The selectable text document is synced on the same schedule.
    pub(super) fn transcript_derived(&mut self, cx: &mut Context<Self>) -> &TranscriptDerived {
        use std::hash::{Hash, Hasher};
        let display = crate::config::display();
        let clean_chat = self.clean_chat_with_display(&display);
        let streaming = {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            if !clean_chat {
                self.reasoning_reveal
                    .visible(&self.streaming_reasoning)
                    .hash(&mut state);
            }
            self.text_reveal
                .visible(&self.streaming_text)
                .hash(&mut state);
            state.finish()
        };
        let expansions = {
            // Order-independent: the set is small and unordered.
            let mut sum = 0u64;
            for entry in &self.expanded_regions {
                let mut state = std::collections::hash_map::DefaultHasher::new();
                entry.hash(&mut state);
                sum = sum.wrapping_add(state.finish().rotate_left(17));
            }
            sum ^ self.expanded_regions.len() as u64
        };
        let key = TranscriptDerivedKey {
            items_revision: self.items.revision(),
            applet_rows_version: self.applet_rows_version,
            streaming,
            expansions,
            display,
            clean_chat,
        };
        if self
            .transcript_derived
            .as_ref()
            .is_none_or(|derived| derived.key != key)
        {
            if self.transcript_derived.as_ref().is_some_and(|derived| {
                derived.key.display != key.display || derived.key.clean_chat != clean_chat
            }) {
                self.transcript_measurements.invalidate();
            }
            let rows = Arc::new(self.transcript_render_rows_with_display(&key.display));
            if let Some(document) = self.transcript_text_document.sync(
                &self.items,
                &rows,
                &self.expanded_regions,
            ) {
                self.transcript_selection
                    .update(cx, |selection, _| selection.set_document(document));
            }
            // Live rows are appended after settled rows. Their growth and
            // disclosure changes cannot move a settled prompt's row index.
            let prompt_rows = self
                .transcript_derived
                .as_ref()
                .filter(|derived| {
                    derived.key.items_revision == key.items_revision
                        && derived.key.applet_rows_version == key.applet_rows_version
                        && derived.key.display == key.display
                        && derived.key.clean_chat == clean_chat
                })
                .map(|derived| derived.prompt_rows.clone())
                .unwrap_or_else(|| {
                    Arc::new(
                        rows.iter()
                            .enumerate()
                            .filter_map(|(row, entry)| match entry.source {
                                TranscriptRowSource::Settled(index)
                                    if classify::is_pinnable_prompt(&self.items[index]) =>
                                {
                                    Some((row, index))
                                }
                                _ => None,
                            })
                            .collect(),
                    )
                });
            if self
                .transcript_derived
                .as_ref()
                .is_none_or(|derived| derived.key.items_revision != key.items_revision)
            {
                self.prune_row_state();
            }
            // Live text and disclosure changes rebuild rows, not settled todo
            // state. Do not rescan history, clone the payload or format its
            // fingerprint for every streaming chunk.
            let (latest_todo, tail_headers) = self
                .transcript_derived
                .as_ref()
                .filter(|derived| derived.key.items_revision == key.items_revision)
                .map(|derived| (derived.latest_todo.clone(), derived.tail_headers))
                .unwrap_or_else(|| {
                    let latest_todo = self.latest_todo_payload().map(Arc::new);
                    let tail_headers = transcript_tail_headers(latest_todo.as_deref());
                    (latest_todo, tail_headers)
                });
            let active_error = clean_chat
                .then(|| clean_chat::active_error(&self.items))
                .flatten();
            self.transcript_derived = Some(TranscriptDerived {
                key,
                rows,
                prompt_rows,
                latest_todo,
                tail_headers,
                active_error,
            });
        }
        self.transcript_derived.as_ref().unwrap()
    }

    #[cfg(test)]
    pub(crate) fn test_transcript_rows_ptr(&self) -> Option<usize> {
        self.transcript_derived
            .as_ref()
            .map(|derived| Arc::as_ptr(&derived.rows) as usize)
    }

    #[cfg(test)]
    pub(crate) fn test_transcript_row_count(&self) -> usize {
        self.transcript_derived
            .as_ref()
            .map_or(0, |derived| derived.rows.len())
    }

    pub(super) fn transcript_render_rows(&self) -> Vec<TranscriptRenderRow> {
        self.transcript_render_rows_with_display(&crate::config::display())
    }

    pub(super) fn transcript_render_rows_with_display(
        &self,
        display: &crate::config::DisplayConfig,
    ) -> Vec<TranscriptRenderRow> {
        let clean_chat = self.clean_chat_with_display(display);
        let detail_items = if clean_chat {
            &[]
        } else {
            self.items.as_slice()
        };
        let mut rows: Vec<TranscriptRenderRow> = Vec::with_capacity(self.items.len() + 2);
        let mut previous_role = None;
        let mut inline_images = HashSet::new();
        let mut user_attachments_end = 0;
        let completed_asks: HashSet<&str> = detail_items
            .iter()
            .filter_map(|item| match item {
                Item::Tool {
                    call_id,
                    name,
                    done: true,
                    ..
                } if name == "ask" => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        // A completion notice is redundant only when a completed bg call
        // already contains the same task, status and actual output preview.
        // Keep nonmatching previews and ongoing tasks: they carry distinct data.
        let completed_bg: Vec<(String, String, String)> = detail_items
            .iter()
            .filter_map(|item| {
                let Item::Tool {
                    name,
                    input,
                    output,
                    done: true,
                    error: None,
                    ..
                } = item
                else {
                    return None;
                };
                let result = tool_output::background_output(name, output)?;
                let task = serde_json::from_str::<serde_json::Value>(input)
                    .ok()?
                    .get("task_id")?
                    .as_str()?
                    .to_owned();
                if !result
                    .fields
                    .iter()
                    .any(|line| line == &format!("Task: {task}"))
                {
                    return None;
                }
                let status = result
                    .fields
                    .iter()
                    .find_map(|line| line.strip_prefix("Status: "))?
                    .trim()
                    .to_owned();
                let preview = result.preview?;
                if status.is_empty() || preview.trim().is_empty() {
                    return None;
                }
                Some((task, status, preview))
            })
            .collect();
        let mut applets = self.applet_rows.iter().enumerate().peekable();
        let applet_row = |ordinal: usize, instance: &String| TranscriptRenderRow {
            // Derived rows need indices that never collide with items or the
            // live streaming sentinels at the top of the range.
            index: usize::MAX / 2 + ordinal,
            source: TranscriptRowSource::Owned(Box::new(Item::Applet(instance.clone()))),
            role: None,
            show_label: false,
        };

        for (index, item) in self.items.iter().enumerate() {
            while let Some((ordinal, (_, instance))) =
                applets.next_if(|(_, (position, _))| *position <= index)
            {
                rows.push(applet_row(ordinal, instance));
            }
            let mut source = TranscriptRowSource::Settled(index);
            if !clean_chat && matches!(item, Item::SideQuestion(_)) {
                continue;
            }
            if clean_chat {
                if !clean_chat::conversation_item(item)
                    && !matches!(item, Item::Tool { name, input, output, call_id, done: true, error }
                        if error.is_none() && swarm_card::is_external_send(name, input, output)
                            || self.codemode_summaries.get(call_id).is_some_and(|summary|
                                Self::external_codemode_messages(summary).next().is_some()))
                {
                    continue;
                }
                if let Some(text) = item.assistant_text() {
                    let public = markdown::public_transcript(text);
                    if public.trim().is_empty() {
                        continue;
                    }
                    if let std::borrow::Cow::Owned(text) = public {
                        let public = match item {
                            Item::AssistantOutput { phase, stream_id, .. } => Item::AssistantOutput {
                                text,
                                phase: *phase,
                                stream_id: stream_id.clone(),
                            },
                            _ => Item::Assistant(text),
                        };
                        source = TranscriptRowSource::Owned(Box::new(public));
                    }
                }
            }
            if matches!(item, Item::User(_)) {
                user_attachments_end = if classify::is_pinnable_prompt(item) {
                    self.user_attachment_range(index).end
                } else {
                    index + 1
                };
                inline_images = self
                    .user_image_markers(index)
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect();
            }
            if inline_images.contains(&index) {
                continue;
            }
            if matches!(item, Item::AssistantOutput { text, .. } if text.trim().is_empty()) {
                continue;
            }
            // A cached transcript from before the history merge fix can still
            // contain detached copies appended after an unrelated tool row.
            if index >= user_attachments_end
                && matches!(item, Item::Image(image)
                    if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput)
                        && matches!(image.anchor, Some(jcode_sdk::RenderedImageAnchor::UserPrompt { .. })))
            {
                continue;
            }
            if let Item::BackgroundTask {
                task_id,
                summary,
                detail: Some(detail),
                done: true,
                ..
            } = item
                && let Some((_, rest)) = detail.split_once("```text\n")
                && let Some((preview, _)) = rest.rsplit_once("\n```")
                && completed_bg.iter().any(|(id, status, output)| {
                    id == task_id
                        && summary.contains(status)
                        && preview.trim_end() == output.trim_end()
                })
            {
                continue;
            }
            // Ask-result attachments render inside their answer card. Keep
            // the image items for the session image browser and history.
            if let Item::Image(image) = item
                && matches!(&image.source, jcode_sdk::RenderedImageSource::ToolResult { tool_name } if tool_name == "ask")
                && let Some(jcode_sdk::RenderedImageAnchor::ToolCall { id }) = &image.anchor
                && completed_asks.contains(id.as_str())
            {
                continue;
            }
            if matches!(item, Item::Reasoning(_))
                && display.thinking_display != crate::config::ThinkingDisplay::Full
            {
                continue;
            }
            if display.screenshot_mode && matches!(item, Item::ResponseStats(_)) {
                continue;
            }
            if matches!(item, Item::Todos(_))
                || matches!(item, Item::Tool { name, .. } if name == "todo")
            {
                continue;
            }

            if let Item::Reasoning(text) = item
                && let Some(previous) = rows.last_mut()
            {
                match &mut previous.source {
                    TranscriptRowSource::Settled(previous_index) => {
                        if let Item::Reasoning(existing) = &self.items[*previous_index] {
                            let mut joined = existing.clone();
                            append_reasoning_text(&mut joined, text);
                            previous.source =
                                TranscriptRowSource::Owned(Box::new(Item::Reasoning(joined)));
                            continue;
                        }
                    }
                    TranscriptRowSource::Owned(item) => {
                        if let Item::Reasoning(existing) = item.as_mut() {
                            append_reasoning_text(existing, text);
                            continue;
                        }
                    }
                }
            }

            let role = role_of(item);
            rows.push(TranscriptRenderRow {
                index,
                source,
                role,
                show_label: role.is_some() && role != previous_role,
            });
            previous_role = role.or(previous_role);
        }

        for (ordinal, (_, instance)) in applets {
            rows.push(applet_row(ordinal, instance));
        }

        for (index, item) in [
            (!clean_chat
                && !self.streaming_reasoning.is_empty()
                && display.thinking_display != crate::config::ThinkingDisplay::Off)
                .then(|| {
                    (
                        usize::MAX - 1,
                        Item::Reasoning(
                            self.reasoning_reveal
                                .visible(&self.streaming_reasoning)
                                .to_owned(),
                        ),
                    )
                }),
            (!self.streaming_text.is_empty()).then(|| {
                (
                    usize::MAX,
                    self.assistant_output_item({
                        let text = self.text_reveal.visible(&self.streaming_text);
                        if clean_chat {
                            markdown::public_transcript(text).into_owned()
                        } else {
                            text.to_owned()
                        }
                    }),
                )
            }),
        ]
        .into_iter()
        .flatten()
        {
            let role = role_of(&item);
            rows.push(TranscriptRenderRow {
                index,
                source: TranscriptRowSource::Owned(Box::new(item)),
                role,
                show_label: role.is_some() && role != previous_role,
            });
            previous_role = role.or(previous_role);
        }

        rows
    }

    /// Compact elapsed time: ticking while a tool runs, final duration after.
    pub(super) fn render_tool_clock(
        &self,
        index: usize,
        call_id: &str,
        finished: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let clock = compact_clock();
        let start = self
            .tool_wall_times
            .get(call_id)
            .and_then(|(start, _)| *start)
            .and_then(tool_clock_label);
        if finished {
            let duration = self
                .tool_durations
                .get(call_id)
                .map(|duration| format_tool_elapsed(*duration));
            let label = [start, duration]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            (!label.is_empty()).then(|| clock.child(label).into_any_element())
        } else {
            let started = *self.tool_started.get(call_id)?;
            let _ = index;
            // Ticks in its own view: refreshing the clock must not rebuild
            // the transcript around it.
            let ticker = self.running_tool_tickers.borrow_mut().get(
                &format!("clock:{call_id}"),
                crate::pulse_text::Kind::Elapsed {
                    since: started,
                    format: format_tool_elapsed,
                },
                cx,
            );
            Some(
                clock
                    .children(start.map(|start| format!("{start} · ")))
                    .child(ticker)
                    .into_any_element(),
            )
        }
    }

    pub(super) fn tool_timing_detail(&self, call_id: &str) -> String {
        let (start, finish) = self
            .tool_wall_times
            .get(call_id)
            .copied()
            .unwrap_or_default();
        let start = start
            .and_then(tool_clock_label)
            .unwrap_or_else(|| "unknown".into());
        let finish = finish
            .and_then(tool_clock_label)
            .unwrap_or_else(|| "unknown".into());
        let duration = self
            .tool_durations
            .get(call_id)
            .copied()
            .or_else(|| self.tool_started.get(call_id).map(Instant::elapsed))
            .map(format_tool_elapsed)
            .unwrap_or_else(|| "unknown".into());
        format!("Started at: {start}   Finished at: {finish}   Duration: {duration}")
    }

    pub(super) fn render_item(
        &self,
        index: usize,
        item: &Item,
        display: &crate::config::DisplayConfig,
        _show_avatar: bool,
        _response_viewport_height: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if display.screenshot_mode
            && matches!(
                item,
                Item::Error(_) | Item::RuntimeError { .. } | Item::BackgroundTask { .. }
            )
        {
            return div()
                .px_1()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(
                    if matches!(item, Item::Error(_) | Item::RuntimeError { .. }) {
                        "Error details hidden"
                    } else {
                        "Background task"
                    },
                )
                .into_any_element();
        }
        let notice_title = match item {
            Item::Stopped(notice) => Some(notice.title()),
            Item::CacheMiss(_) => Some("Cache miss"),
            _ => None,
        };
        if display.screenshot_mode
            && let Some(title) = notice_title
        {
            return div()
                .px_2()
                .py_1()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(title.to_owned())
                .into_any_element();
        }
        match item {
            Item::Applet(instance) => {
                let selection = self.applet_selection.clone();
                crate::applet_surface::card_with_tasks(
                    instance, true, &selection,
                    Some((&self.session_id, &|app| self.applet_session_tasks(app))), window, cx,
                )
                    .unwrap_or_else(|| div().into_any_element())
            }
            Item::Stopped(notice) => self.render_stop_notice(index, notice, window, cx),
            Item::SwarmWorkerNotice(text) => div()
                .debug_selector(|| "swarm-worker-notice".into())
                .min_w_0()
                .px_2()
                .py_1()
                .truncate()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(text_selection::plain(
                    self.transcript_selection.clone(),
                    format!("{index}-swarm-worker-notice"),
                    text.clone(),
                    window,
                    cx,
                ))
                .into_any_element(),
            Item::ProviderGuardrailRetry { text, .. } => div()
                .debug_selector(|| "provider-guardrail-retry".into())
                .min_w_0()
                .px_2()
                .py_1()
                .truncate()
                .text_size(px(11.))
                .text_color(Theme::global().TEXT_DIM)
                .child(text_selection::plain(
                    self.transcript_selection.clone(),
                    format!("{index}-provider-guardrail-retry"),
                    text.clone(),
                    window,
                    cx,
                ))
                .into_any_element(),
            Item::Compaction(row) => self.render_compaction_divider(index, row, window, cx),
            Item::Memory(row) => self.render_memory_divider(index, row, window, cx),
            Item::CacheMiss(notice) => self.render_cache_miss_notice(index, notice, window, cx),
            Item::ResponseStats(stats) => stats.render(index).into_any_element(),
            Item::User(text) if jcode_sdk::todo::is_auto_poke_message(text) => {
                self.render_auto_poke_notice(index, text, window, cx)
            }
            Item::User(text) => self.render_user_prompt(index, text, None, window, cx),
            Item::Image(image) => {
                if matches!(image.source, jcode_sdk::RenderedImageSource::UserInput) {
                    return self.render_user_image_chip(index, image, cx);
                }
                if self.image_pane_open && !display.screenshot_mode {
                    return self.render_image_pane_link(index, image, cx);
                }
                let panel = cx.entity().downgrade();
                let label = if display.screenshot_mode {
                    "Image attachment".to_string()
                } else {
                    image.display_label("Image attachment")
                };
                div()
                    .id(("transcript-image", index))
                    .debug_selector(|| "transcript-image".into())
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .when_some(image.preview.clone(), |el, preview| {
                        let scroll_panel = panel.clone();
                        let gesture_panel = panel.clone();
                        el.child(
                            crate::inline_image::InlineImage::new(index, preview)
                                .metadata(label.clone())
                                .on_fit_scroll(move |event, window, cx| {
                                    let _ = scroll_panel.update(cx, |panel, cx| {
                                        if panel.transcript_list.is_scrollbar_dragging() {
                                            return;
                                        }
                                        let y = f32::from(
                                            event.delta.pixel_delta(window.line_height()).y,
                                        );
                                        if y == 0.0 {
                                            return;
                                        }
                                        if event.delta.precise() {
                                            panel.glide_transcript_input(-y, true, cx);
                                        } else {
                                            panel.glide_transcript_wheel(-y, cx);
                                        }
                                    });
                                })
                                .on_gesture(move |_, cx| {
                                    let _ = gesture_panel.update(cx, |panel, _| {
                                        panel.cancel_transcript_momentum();
                                    });
                                }),
                        )
                    })
                    .when(image.preview.is_none(), |el| {
                        let lazy_image = image.clone();
                        let lazy = image.data.starts_with("jcode-lazy:");
                        el.child(
                            div()
                                .id(("history-image-placeholder", index))
                                .debug_selector(|| "history-image-placeholder".into())
                                .text_size(px(11.0))
                                .text_color(Theme::global().TEXT_DIM)
                                .when(lazy, |el| {
                                    el.cursor_pointer().on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.open_image_preview(lazy_image.clone(), window, cx);
                                        },
                                    ))
                                })
                                .child(if lazy {
                                    format!("{label} (click to load)")
                                } else {
                                    format!("{label} (could not display {})", image.media_type)
                                }),
                        )
                    })
                    .into_any_element()
            }
            Item::Assistant(text) | Item::AssistantOutput { text, .. } => div()
                .debug_selector(|| "assistant-response".into())
                .font_family(Theme::global().FONT_AI)
                .px_1()
                .text_color(Theme::global().TEXT)
                .child(markdown::with_stream_fade(
                    if index == usize::MAX {
                        self.text_reveal.fade(&self.streaming_text)
                    } else {
                        markdown::StreamFade::default()
                    },
                    || {
                        markdown::render_interactive(
                            text,
                            index,
                            &self.transcript_selection,
                            window,
                            cx,
                            false,
                            self.media_preview_handler(cx),
                        )
                        .into_any_element()
                    },
                ))
                .into_any_element(),
            Item::AgentMessage {
                from_session,
                from_name,
                message,
                timestamp_ms,
            } => self.render_agent_message(
                index,
                from_session,
                from_name.as_deref(),
                message,
                *timestamp_ms,
                window,
                cx,
            ),
            Item::SideQuestion(question) => {
                self.render_inline_side_question(index, question, window, cx)
            }
            // Thinking is secondary transcript text, not a separate card. Keep
            // the same presentation while streaming, settled, and restored.
            // Never truncate it: there is deliberately no disclosure control.
            Item::Reasoning(text) => div()
                .debug_selector(|| "reasoning-inline".into())
                .font_family(Theme::global().FONT_AI)
                .flex_none()
                .px_1()
                .text_size(px(12.0))
                .italic()
                .text_color(Theme::global().REASONING)
                .child(markdown::with_stream_fade(
                    if index == usize::MAX - 1 {
                        self.reasoning_reveal.fade(&self.streaming_reasoning)
                    } else {
                        markdown::StreamFade::default()
                    },
                    || {
                        markdown::render_interactive(
                            text,
                            index,
                            &self.transcript_selection,
                            window,
                            cx,
                            true,
                            self.media_preview_handler(cx),
                        )
                        .into_any_element()
                    },
                ))
                .into_any_element(),
            Item::Todos(payload) => render_todo_card_with_style(
                payload,
                false,
                &format!("todo-{index}"),
                &self.transcript_selection,
                window,
                cx,
            ),
            Item::BackgroundTask {
                task_id,
                label,
                summary,
                detail,
                percent,
                done,
            } => {
                let key = expandable::Key::task(task_id);
                let expanded = self.expanded_regions.contains(&key);
                let (row_summary, exit_code) = background_task::summary_parts(summary);
                // One ellipsis budget, with the title ahead of progress text.
                let mut header = label.clone();
                let summary_start = header.len();
                if !row_summary.is_empty() {
                    header.push_str(" · ");
                    header.push_str(&row_summary);
                }
                let summary_style = (summary_start < header.len()).then(|| {
                    (
                        summary_start..header.len(),
                        gpui::HighlightStyle {
                            color: Some(crate::theme::to_hsla(Theme::global().TEXT_FAINT)),
                            ..Default::default()
                        },
                    )
                });
                let header = gpui::StyledText::new(header).with_highlights(summary_style);
                let toggle_key = key.clone();
                let background_task_id = task_id.clone();
                let done_for_click = *done
                    && detail
                        .as_deref()
                        .is_some_and(|value| value.contains("_Full output:_"));
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .min_w_0()
                    .debug_selector(|| "background-task-card".into())
                    .child(
                        div()
                            .id(("background-task-toggle", index))
                            .debug_selector(|| "background-task-expand".into())
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .min_w_0()
                            .overflow_hidden()
                            .px_1()
                            .py(px(2.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.stop_propagation();
                                if !this.expanded_regions.contains(&toggle_key) && done_for_click {
                                    this.request_background_output(&background_task_id);
                                }
                                this.toggle_line(&toggle_key, cx);
                            }))
                            .child(crate::tool_icon::render_badge(
                                "bg",
                                *done,
                                *done
                                    && (exit_code.is_some()
                                        || summary.to_lowercase().contains("failed")),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .debug_selector(|| "background-task-title".into())
                                    .text_color(Theme::global().TOOL_TEXT)
                                    .child(header),
                            )
                            .when_some(exit_code, |el, code| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .px_2()
                                        .rounded_full()
                                        .bg(Theme::global().INLINE_CODE_BG)
                                        .text_color(Theme::global().ERROR)
                                        .child(format!("exit {code}")),
                                )
                            })
                            .when_some(
                                percent.filter(|value| !*done && value.is_finite()),
                                |el, percent| {
                                    el.child(
                                        div()
                                            .flex_none()
                                            .w(px(40.0))
                                            .debug_selector(|| "background-task-progress".into())
                                            .h(px(2.0))
                                            .rounded_full()
                                            .bg(Theme::global().TOOL_BORDER)
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(relative((percent / 100.0).clamp(0.0, 1.0)))
                                                    .rounded_full()
                                                    .bg(Theme::global().WARN),
                                            ),
                                    )
                                },
                            )
                            .children(tool_card_token_pill(detail.as_deref().unwrap_or("")))
                            .when(crate::fixture_control_targets::background_tasks(), |el| {
                                el.relative()
                                    .children(crate::fixture_control_targets::marker(
                                        || key.clone(),
                                        || Some(self.session_id.clone()),
                                    ))
                            }),
                    )
                    .when(expanded, |el| {
                        el.child(
                            div()
                                .id(("background-task-output-scroll", index))
                                .debug_selector(|| "background-task-output".into())
                                .ml(px(24.0))
                                .mt_1()
                                .p_3()
                                .rounded_lg()
                                .max_h(px(expandable::MAX_HEIGHT))
                                .bg(Theme::global().CODE_BG)
                                .font_family(Theme::global().FONT_MONO)
                                .text_size(px(11.5))
                                .text_color(Theme::global().CODE_TEXT)
                                .child(text_selection::plain(
                                    self.transcript_selection.clone(),
                                    format!("background-detail-{index}"),
                                    background_task::detail_text(
                                        task_id,
                                        summary,
                                        detail.as_deref(),
                                    ),
                                    window,
                                    cx,
                                ))
                                .map(crate::transcript_scroll::vertical),
                        )
                    })
                    .into_any_element()
            }
            Item::Tool {
                call_id,
                name,
                input,
                output,
                done,
                error,
            } => {
                if name == "ask" && *done {
                    return self.render_decision(
                        index,
                        call_id,
                        input,
                        output,
                        error.as_deref(),
                        window,
                        cx,
                    );
                }
                let (offset, opacity, animating) = self
                    .arriving_tools
                    .get(call_id)
                    .map(|started_at| {
                        crate::transition::arrival_motion(
                            *started_at,
                            Instant::now(),
                            crate::transition::policy(crate::transition::Transition::ToolArrival)
                                .duration,
                        )
                    })
                    .unwrap_or((0.0, 1.0, false));
                if animating {
                    window.request_animation_frame();
                }
                let is_codemode = classify::is_codemode(name);
                let codemode = (is_codemode && *done)
                    .then(|| self.codemode_summaries.get(call_id).map(Box::as_ref))
                    .flatten();
                let failed = error.is_some()
                    || (is_codemode && classify::wrapper_failed(codemode, None, output));
                let nested = codemode.and_then(|summary| {
                    codemode_card::single_swarm(summary).map(|call| (summary, call))
                });
                if !display.screenshot_mode {
                    if let Some(call) = nested
                        .map(|(_, call)| swarm_card::from_codemode(call, input))
                        .or_else(|| swarm_card::parse(name, input, output))
                    {
                        return self.render_swarm_card(
                            index, call_id, &call, name, input, *done, output, error.as_deref(),
                            nested, offset, opacity, window, cx,
                        );
                    }
                }
                if self.clean_chat_with_display(&display) && let Some(summary) = codemode {
                    return div()
                        .children(Self::render_external_codemode_messages(summary))
                        .into_any_element();
                }
                let expanded = self.expanded_regions.contains(call_id)
                    ^ (name == "agentgrep" && display.show_agentgrep_output);
                let detail_progress = self
                    .tool_detail_motion
                    .get(call_id)
                    .map(|motion| {
                        let mut motion = *motion;
                        motion.sample(Instant::now())
                    })
                    .unwrap_or(if expanded { 1.0 } else { 0.0 });
                let detail_visible = expanded || self.tool_detail_motion.contains_key(call_id);
                let summary = if display.screenshot_mode {
                    if *done {
                        "finished".into()
                    } else {
                        "running".into()
                    }
                } else {
                    tool_row_summary(name, input)
                };
                let toggle_key = call_id.clone();
                let bg_task_for_click = if name == "bg" && *done {
                    serde_json::from_str::<serde_json::Value>(input)
                        .ok()
                        .and_then(|v| v.get("task_id").and_then(|v| v.as_str()).map(str::to_owned))
                } else {
                    None
                };
                let keyboard_key = call_id.clone();
                let bg_task_for_keyboard = bg_task_for_click.clone();
                let row_focus = self
                    .swarm_row_focus
                    .borrow_mut()
                    .entry(call_id.clone())
                    .or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone();
                let click_focus = row_focus.clone();
                let has_detail = true;
                let (token_label, token_color) = tool_output_token_badge(output);
                let call_id = call_id.clone();
                let running_veil = (!*done && !failed).then(|| {
                    gpui::AnyView::from(self.running_tool_tickers.borrow_mut().get(
                        &format!("pulse:{call_id}"),
                        crate::tool_icon::running_veil(
                            Theme::global().panel_background(self.surface_focused),
                        ),
                        cx,
                    ))
                });
                div()
                    .id(("tool", index))
                    .debug_selector(|| "tool-inline".into())
                    .flex()
                    .ml(px(offset))
                    .opacity(opacity)
                    // Transcript rows live in a fixed-height flex column. Once
                    // it overflows, flex items shrink by default, which can
                    // squash tool rows instead of letting the column scroll.
                    .flex_none()
                    .flex_col()
                    .overflow_hidden()
                    .when_some(codemode.filter(|_| nested.is_none() && !display.screenshot_mode), |el, summary| {
                        el.children(Self::render_external_codemode_messages(summary))
                    })
                    .child(crate::tool_icon::animate_running(
                        div()
                            .id(("tool-header-click", index))
                            .debug_selector(|| "tool-header".into())
                            .cursor_pointer()
                            .track_focus(&row_focus)
                            .on_key_down(cx.listener(
                                move |this, event: &gpui::KeyDownEvent, _, cx| {
                                    if event.keystroke.key == "enter" && !event.is_held {
                                        if !this.expanded_regions.contains(&keyboard_key) && let Some(task_id) = &bg_task_for_keyboard {
                                            this.request_background_output(task_id);
                                        }
                                        this.toggle_region(&keyboard_key, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                window.focus(&click_focus, cx);
                                if !this.expanded_regions.contains(&toggle_key) && let Some(task_id) = &bg_task_for_click {
                                    this.request_background_output(task_id);
                                }
                                this.toggle_region(&toggle_key, cx);
                                cx.stop_propagation();
                            }))
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .px_1()
                            .py(px(2.0))
                            .text_size(px(14.0))
                            // Match the token pill's 14px line plus 2px padding per side.
                            .line_height(px(18.0))
                            .child(crate::tool_icon::render_badge(name, *done, failed))
                            .when(!summary.is_empty(), |el| {
                                el.child(
                                    div()
                                        .debug_selector(|| "tool-summary".into())
                                        .id(("tool-summary-text", index))
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(Theme::global().TOOL_TEXT)
                                        .child(summary),
                                )
                            })
                            .when_some(tool_output::background_output(name, output), |el, result| {
                                el.when_some(tool_bg_status(name, output), |el, status| el.child(div().flex_none().text_size(px(11.0)).text_color(Theme::global().TEXT_FAINT).child(status)))
                                  .when_some(output.lines().find_map(|line| line.strip_prefix("Duration: ")), |el, duration| el.child(div().flex_none().text_size(px(11.0)).text_color(Theme::global().TEXT_FAINT).child(duration.to_owned())))
                                  .when_some(result.exit_code.filter(|code| *code != 0), |el, code| el.child(div().debug_selector(|| "tool-bg-exit-code".into()).flex_none().px_2().rounded_full().bg(Theme::global().INLINE_CODE_BG).text_color(Theme::global().ERROR).child(format!("exit {code}"))))
                            })
                            .children(self.render_tool_clock(
                                index,
                                &call_id,
                                *done || failed,
                                cx,
                            ))
                            // The token pill shares the row's click target.
                            .when(has_detail && !display.screenshot_mode, |el| {
                                el.child(
                                    div()
                                        .id(("tool-output-toggle", index))
                                        .debug_selector(|| "tool-output-size".into())
                                        .flex_none()
                                        .ml_auto()
                                        .px_2()
                                        .py(px(2.0))
                                        .rounded_full()
                                        .bg(Theme::global().INLINE_CODE_BG)
                                        .hover(|style| style.bg(Theme::global().TOOL_BORDER))
                                        .cursor_pointer()
                                        .text_size(px(10.0))
                                        .line_height(px(14.0))
                                        .text_color(token_color)
                                        .child(token_label),
                                )
                            }),
                        *done,
                        failed,
                        running_veil,
                    ))
                    .when(detail_visible && !display.screenshot_mode && has_detail, |el| {
                        let scroll = self
                            .swarm_scrolls
                            .borrow_mut()
                            .entry(call_id.clone())
                            .or_default()
                            .clone();
                        let applet_instance = crate::applet_runtime::get(cx)
                            .host
                            .borrow()
                            .tool_card(&self.session_id, &call_id)
                            .map(|mounted| mounted.instance.id.clone());
                        let mut supplement = if name == "ask" && *done {
                            Some(
                                div()
                                    .debug_selector(|| "ask-result".into())
                                    .child(self.render_ask_answer(
                                        call_id.as_str(),
                                        ask::result_text(input, output, error.as_deref()),
                                        cx,
                                    ))
                                    .into_any_element(),
                            )
                        } else if let Some(response) = error
                            .is_none()
                            .then(|| user_response::parse(name, input))
                            .flatten()
                        {
                            Some(user_response::render(
                                &response,
                                index,
                                None,
                                &self.transcript_selection,
                                self.media_preview_handler(cx),
                                window,
                                cx,
                            ))
                        } else if let Some(instance) = applet_instance {
                            crate::applet_surface::card_with_tasks(
                                &instance,
                                true,
                                &self.applet_selection,
                                Some((&self.session_id, &|app| self.applet_session_tasks(app))),
                                window,
                                cx,
                            )
                        } else if name == "todo" && *done && error.is_none() {
                            parse_todo_tool_output(output).map(|payload| {
                                render_todo_card_with_style(
                                    &payload,
                                    false,
                                    &format!("todo-{index}"),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                )
                            })
                        } else {
                            None
                        };
                        if supplement.is_none()
                            && display.diff_display != crate::config::DiffDisplay::Off
                        {
                            supplement = self.render_edit_metadata(
                                &call_id,
                                name,
                                input,
                                output,
                                *done,
                                error.as_deref(),
                                cx,
                            );
                        }
                        if supplement.is_none() {
                            let toggle_id = call_id.clone();
                            if let Some(compose) = gmail_draft_card::parse(name, input) {
                                let compose =
                                    gmail_draft_card::merge_output(compose, output, *done);
                                supplement = Some(gmail_draft_card::render(
                                    index,
                                    &compose,
                                    &gmail_draft_card::outcome(output, *done),
                                    error.as_deref(),
                                    tool_card_token_pill(output),
                                    self.superseded_drafts().get(&call_id).copied(),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            } else if let Some(view) =
                                gmail_read_card::parse(name, input, output, *done, error.as_deref())
                            {
                                supplement = Some(gmail_read_card::render(
                                    index,
                                    &view,
                                    tool_card_token_pill(output),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            } else if let Some(view) =
                                mcp_list_card::parse(name, input, output, *done, error.as_deref())
                            {
                                supplement = Some(mcp_list_card::render(
                                    index,
                                    &view,
                                    tool_card_token_pill(output),
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_region(&toggle_id, cx);
                                        cx.stop_propagation();
                                    }),
                                    &self.transcript_selection,
                                    window,
                                    cx,
                                ));
                            }
                        }
                        let height = if self.expandable_viewport_height > px(0.) {
                            (f32::from(self.expandable_viewport_height) * 0.6)
                                .clamp(1., expandable::MAX_HEIGHT)
                        } else {
                            expandable::MAX_HEIGHT
                        };
                        el.child(
                            div()
                                .debug_selector(|| "tool-detail".into())
                                .opacity(detail_progress)
                                .relative()
                                .ml(px(24.0))
                                .mt_1()
                                .mb_1()
                                .p_3()
                                .rounded_lg()
                                .bg(Theme::global().CODE_BG)
                                .font_family(Theme::global().FONT_MONO)
                                .text_size(px(11.5))
                                .line_height(relative(1.45))
                                .text_color(Theme::global().CODE_TEXT)
                                .child(
                                    div()
                                        .id(format!("tool-detail-{index}-scroll"))
                                        .debug_selector(move || {
                                            format!("tool-detail-{index}-scroll")
                                        })
                                        .w_full()
                                        .min_w_0()
                                        .max_h(px(height))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(div().debug_selector(|| "tool-timing-detail".into()).mb_2().text_color(Theme::global().TEXT_FAINT).child(self.tool_timing_detail(&call_id)))
                                                .when_some(tool_output::bash_exit_code(name, output).filter(|code| *code != 0), |el, code| {
                                                    el.child(div().debug_selector(|| "tool-detail-exit-code".into()).self_start().mb_2().px_2().py_1().rounded_full().bg(Theme::global().INLINE_CODE_BG).text_color(if code == 0 { Theme::global().OK } else { Theme::global().ERROR }).child(format!("exit {code}")))
                                                })
                                                .when(!*done && error.is_none() && self.has_running_tool(), |el| {
                                                el.child(
                                                    div()
                                                        .id(("tool-background", index))
                                                        .debug_selector(|| "tool-background".into())
                                                        .flex_none()
                                                        .px_2()
                                                        .py(px(2.0))
                                                        .rounded_full()
                                                        .bg(Theme::global().INLINE_CODE_BG)
                                                        .hover(|style| style.bg(Theme::global().TOOL_BORDER))
                                                        .cursor_pointer()
                                                        .text_size(px(10.0))
                                                        .line_height(px(14.0))
                                                        .text_color(Theme::global().TEXT_FAINT)
                                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                                            cx.stop_propagation();
                                                            this.background_running_tool(cx);
                                                        }))
                                                        .child("Background  Alt+B"),
                                                )
                                            })
                                            .children(supplement)
                                                .child(if is_codemode && *done {
                                                    self.render_codemode_detail(
                                                        index, &call_id, input, output, error.as_deref(),
                                                        codemode, None, window, cx,
                                                    )
                                                } else if let Some((args, result)) = tool_output::bash_sections(name, input, output, error.as_deref()) {
                                                    div().flex().flex_col()
                                                        .child(div().debug_selector(|| "tool-bash-command".into()).child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .when(!result.is_empty(), |el| el.child(div().debug_selector(|| "tool-bash-output-code".into()).p_2().rounded_lg().bg(Theme::global().INLINE_CODE_BG).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-bash-output-{index}"), result, window, cx))))
                                                        .into_any_element()
                                                } else if let Some(result) = tool_output::background_output(name, output) {
                                                    div().flex().flex_col().gap_2()
                                                        .when_some(result.preview, |el, preview| el.child(div().debug_selector(|| "tool-bg-output-code".into()).p_2().rounded_lg().bg(Theme::global().INLINE_CODE_BG).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-bg-preview-{index}"), preview, window, cx))))
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else if let Some(lines) = tool_output::json_lines(name, output, &call_id, &self.expanded_regions) {
                                                    let args = tool_output::ToolDetail::arguments(input);
                                                    let mut toggle_index = 0usize;
                                                    let rows = lines.into_iter().enumerate().map(|(line_index, line)| {
                                                        let toggle = line.toggle.map(|key| {
                                                            let selector = format!("tool-json-toggle-{toggle_index}");
                                                            toggle_index += 1;
                                                            div().id(SharedString::from(format!("json-toggle-{index}-{line_index}"))).debug_selector(move || selector.clone().into())
                                                                .w(px(18.0)).flex_none().cursor_pointer().text_color(Theme::global().TEXT_FAINT)
                                                                .child(if line.collapsed { "▸" } else { "▾" })
                                                                .on_click(cx.listener(move |this, _, _, cx| { cx.stop_propagation(); this.toggle_line(&key, cx); })).into_any_element()
                                                        }).unwrap_or_else(|| div().w(px(18.0)).flex_none().into_any_element());
                                                        div().flex().child(toggle).child(text_selection::plain(self.transcript_selection.clone(), format!("tool-json-{index}-{line_index}"), line.text, window, cx)).into_any_element()
                                                    }).collect::<Vec<_>>();
                                                    div().debug_selector(|| "tool-detail-json".into()).font_family(Theme::global().FONT_MONO).flex().flex_col()
                                                        .when(!args.is_empty(), |el| el.child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .children(rows)
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else if let Some(markdown) = tool_output::markdown_output(name, output) {
                                                    let args = tool_output::ToolDetail::arguments(input);
                                                    div().debug_selector(|| "tool-detail-markdown".into())
                                                        .flex().flex_col().gap_2()
                                                        .when(!args.is_empty(), |el| el.child(tool_output::selectable_arguments(args, index, self.transcript_selection.clone(), window, cx)))
                                                        .child(div().font_family(Theme::global().FONT_AI).child(markdown::render(&markdown, tool_output::markdown_row(index), &self.transcript_selection, window, cx)))
                                                        .when_some(error.as_ref(), |el, error| el.child(text_selection::plain(self.transcript_selection.clone(), format!("tool-detail-error-{index}"), format!("Error\n{}", strip_ansi(error)), window, cx)))
                                                        .into_any_element()
                                                } else {
                                                    div().debug_selector(|| "tool-argument-table".into()).child(div().debug_selector({ let selector = tool_output::selector(name, input, output); move || selector.into() }).child(tool_output::selectable_detail(
                                                        name, input, tool_output::ToolDetail::new(name, input, output, error.as_deref()),
                                                        format!("tool-detail-{index}"),
                                                        self.transcript_selection.clone(), window, cx,
                                                    ))).into_any_element()
                                                })
                                                .child(
                                                    div()
                                                        .debug_selector(|| "tool-detail-end".into())
                                                        .child(""),
                                                ),
                                        )
                                        .map(|body| crate::transcript_scroll::tracked_vertical(body, &scroll)),
                                )
                                .children((crate::fixture_control_targets::transcript_scroll_fixture() && call_id == "scroll-fixture").then(|| {
                                    crate::fixture_control_targets::marker(|| "scroll:tool".into(), || Some(self.session_id.clone())).unwrap()
                                }))
                                .child(crate::scrollbar::vertical_with_track(
                                    &scroll,
                                    "tool-detail-scrollbar",
                                )),
                        )
                    })
                    .into_any_element()
            }
            Item::Error(message) => self.render_recovery_error(index, message, false, window, cx),
            Item::RuntimeError { message, .. } => {
                self.render_recovery_error(index, message, true, window, cx)
            }
        }
    }

    /// Flip one disclosure region (`expandable::Open`). A tool card fades its
    /// detail in and out; every other region appears at once.
    pub(crate) fn toggle_region(&mut self, key: &str, cx: &mut Context<Self>) {
        let was_expanded = self.expanded_regions.contains(key);
        if !self.expanded_regions.remove(key) {
            self.expanded_regions.insert(key.to_owned());
        }
        let duration = crate::transition::policy(crate::transition::Transition::Overlay).duration;
        if cx.reduce_motion() || duration.is_zero() {
            self.tool_detail_motion.remove(key);
        } else {
            self.tool_detail_motion
                .entry(key.to_owned())
                .or_insert_with(|| {
                    crate::transition::AnimatedValue::new(
                        if was_expanded { 1.0 } else { 0.0 },
                        duration,
                    )
                })
                .set(if was_expanded { 0.0 } else { 1.0 }, Instant::now());
        }
        self.transcript_measurements.invalidate();
        cx.notify();
    }

    /// Flip one disclosure region that has no animated detail: a notice body,
    /// a background task, a swarm line or an error card's raw message.
    pub(super) fn request_background_output(&self, task_id: &str) {
        if !self.is_side_document() && self.preview_state.is_none() {
            self.bridge.send(Command::RuntimeCommand {
                session_id: self.session_id.clone(),
                command: jcode_sdk::api::RuntimeCommand::BackgroundOutput,
                args: vec![task_id.to_owned()],
            });
        }
    }

    pub(crate) fn toggle_line(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded_regions.remove(key) {
            self.expanded_regions.insert(key.to_owned());
        }
        self.transcript_measurements.invalidate();
        cx.notify();
    }
}

impl Panel {
    pub fn new(
        session_id: String,
        title: Option<String>,
        working_dir: Option<String>,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let rig_feed = crate::openrig_feed::get(cx);
        cx.observe(&rig_feed, |panel, _, cx| panel.refresh_rig_hud(cx))
            .detach();
        let rig_host_id = (crate::harness::screenshot_mode()
            && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RIG_HUD").is_some())
        .then(|| "host-fixture-local".to_owned());
        let rig_identity = rig_host_id.as_deref().and_then(|host| {
            rig_hud::identify(rig_feed.read(cx).fleet()?, &session_id, Some(host))
        });
        let send_bridge = bridge.clone();
        let send_session = session_id.clone();
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(
                cx,
                "Type something…",
                move |content, images, _window, _app| {
                    send_bridge.send(Command::Send {
                        session_id: send_session.clone(),
                        content,
                        images,
                        attachments: Vec::new(),
                    });
                },
            )
            .with_example_prompts();
            input.enable_context_picker();
            input
        });
        // Local echo is appended by the workspace when submit fires; simplest
        // is to observe our own input entity... but the closure above has no
        // panel access. Instead the workspace routes sends through the panel.
        Self::observe_draft_images(&input, cx);
        let display_title = title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| short_id(&session_id));
        let transcript_list = ListState::new(0, ListAlignment::Top, px(600.));
        let transcript_selection = cx.new(TextSelection::new_scoped);
        cx.observe(&transcript_selection, |panel, selection, cx| {
            let dragging = selection.read(cx).is_dragging();
            if dragging && !panel.transcript_dragging {
                panel.cancel_transcript_momentum();
                panel.stick_to_bottom = false;
                panel.release_startup_preview();
            }
            panel.transcript_dragging = dragging;
            panel.transcript_measurements.invalidate();
            cx.notify();
        })
        .detach();
        // The panel owns this ListState. A strong entity in its persistent
        // callback would keep the panel and its transcript alive after close
        // or hot reload, even when no window references it anymore.
        let panel_entity = cx.entity().downgrade();
        transcript_list.set_scroll_handler(move |event, _, cx| {
            let _ = panel_entity.update(cx, |panel, cx| {
                panel.release_startup_preview();
                if event.is_scrolled && !event.is_following_tail && event.visible_range.start == 0 {
                    panel.request_older_history();
                }
                let stick_to_bottom =
                    event.is_following_tail && !panel.transcript_selection.read(cx).is_dragging();
                if panel.stick_to_bottom != stick_to_bottom {
                    panel.stick_to_bottom = stick_to_bottom;
                    cx.notify();
                }
            });
        });
        let fixture = screenshot_fixture_for(&session_id);
        let tool_card_fixture = crate::harness::screenshot_mode()
            && matches!(fixture.as_deref(), Some("tool-cards" | "tool-cards-details"));
        let (mut fixture_tool_durations, mut fixture_tool_wall_times) = demo_tool_times(fixture.as_deref());
        let (items, fixture_codemode_summaries) = if tool_card_fixture {
            let history = codemode_card::screenshot_history();
            let codemode = codemode_card::history_summaries(&history).collect();
            for (id, (start, finish, duration)) in history_tool_times(&history) {
                fixture_tool_wall_times.insert(id.clone(), (start, finish));
                if let Some(duration) = duration {
                    fixture_tool_durations.insert(id, duration);
                }
            }
            (crate::panel::history_items::build(history, Vec::new()).0, codemode)
        } else {
            (demo_items(fixture.as_deref()), HashMap::new())
        };
        let items: Transcript = items.into();
        // `JCODE_DESKTOP_SCREENSHOT_EXPANDED=1` opens every disclosure in the
        // fixture, so the capture shows the expanded text rather than only the
        // collapsed rows.
        let expanded_regions =
            if tool_card_fixture && fixture.as_deref() == Some("tool-cards-details") {
                HashSet::from(["fixture-codemode-multi".into(), "codemode-calls:fixture-codemode-multi".into()])
            } else if crate::harness::screenshot_mode()
                && (expandable::Regions::screenshot_expanded() || fixture.as_deref() == Some("btw-clean-expanded"))
            {
                expandable::Regions::all(&items)
            } else {
                HashSet::new()
            };
        let streaming_fixture = crate::harness::screenshot_mode()
            && session_id == "screenshot-fixture"
            && matches!(
                std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref(),
                Ok("streaming" | "mermaid")
            );
        let usage_fixture =
            crate::harness::screenshot_mode() && fixture.as_deref() == Some("tokens");
        let effort_fixture = usage_fixture
            || (crate::fixture_control_targets::enabled()
                && fixture.as_deref() == Some("mode-picker"));
        let activity_status = if crate::harness::screenshot_mode() {
            match fixture.as_deref() {
                Some("orb-working" | "orb-todos") => "running",
                Some("orb-thinking") => "thinking",
                Some("orb-tools") => "running_tools",
                _ => "idle",
            }
        } else {
            "idle"
        };
        if crate::harness::screenshot_mode()
            && matches!(fixture.as_deref(), Some("btw-normal" | "btw-clean-expanded"))
        {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.show_saved_side_question(side_question::inline::screenshot_thread(), cx);
                });
            });
        }
        if crate::harness::screenshot_mode() && fixture.as_deref() == Some("ask") {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let questions: Vec<_> = (0..4).map(|i| serde_json::json!({
                    "header": format!("Section {}", i + 1),
                    "question": format!("Section {}: Which option works best for this project?", i + 1),
                    "options": (0..5).map(|o| serde_json::json!({"label": format!("Option {}", o + 1), "description": "A longer description to show how the answer wraps across multiple lines in a compact card."})).collect::<Vec<_>>()
                })).collect();
                let event = serde_json::json!({"ev":"ask_request", "session_id":"screenshot-fixture", "request_id":"screenshot-ask", "tool_call_id":"screenshot-tool", "questions":questions});
                if let Ok(event) = serde_json::from_value::<ApiEvent>(event) {
                    let _ = panel.update(cx, |panel, cx| {
                        panel.apply(&event, cx);
                        if let Some(card) = &mut panel.ask_card {
                            card.toggle(0, 0);
                            card.open_comment(0, 0);
                        }
                    });
                }
            });
        }
        if crate::harness::screenshot_mode() && fixture.as_deref() == Some("mode-picker") {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| panel.seed_mode_picker_fixture(cx));
            });
        }
        if session_id == "screenshot-fixture" && crate::fixture_control_targets::voice_recording() {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| panel.seed_inline_voice_fixture(cx));
            });
        }
        if crate::harness::screenshot_mode() && fixture.as_deref() == Some("image-prompt") {
            let panel = cx.weak_entity();
            cx.defer(move |cx| {
                let _ = panel.update(cx, |panel, cx| {
                    let image = image_preview::fixture_image();
                    let attachment = (image.media_type, image.data);
                    panel.seed_screenshot_expanded_queue_image(attachment.clone());
                    panel.input.update(cx, |input, cx| {
                        input.recall_prompt(
                            "Draft [#image 2] and [#image 1] with more text",
                            vec![attachment.clone(), attachment],
                            cx,
                        );
                    });
                    cx.notify();
                });
            });
        }
        let activity_owner = cx.entity();
        let queue_items_scroll = gpui::ScrollHandle::new();
        let focus_handle = cx.focus_handle();
        let openrig = (crate::harness::screenshot_mode() && fixture.as_deref() == Some("openrig"))
            .then(|| {
                let focus = focus_handle.clone();
                let session = session_id.clone();
                cx.new(|_| openrig::State::screenshot_fixture(focus, session))
            });
        Self {
            session_id,
            title: display_title.into(),
            working_dir,
            status: activity_status.into(),
            turn_outcome: None,
            connection_phase: String::new(),
            continuing_after_reload: false,
            model: effort_fixture.then(|| "gpt-5.6-sol".into()),
            provider: effort_fixture.then(|| "openai".into()),
            auth_method: usage_fixture.then(|| "oauth".into()),
            account_label: usage_fixture.then(|| "work@example.com".into()),
            reasoning_effort: effort_fixture.then(|| "high".into()),
            skills: Vec::new(),
            active_skill: None,
            pinned_modes: Vec::new(),
            mode_multiselect: false,
            mode_catalog: Default::default(),
            pending_model: None,
            pending_effort: None,
            pending_save_requests: 0,
            context_tokens: usage_fixture.then_some(100_000),
            context_usage: context_usage::State::default(),
            memory_activity: None,
            response_stats: response_stats::Tracker::default(),
            cache_telemetry: cache_timer::card::CacheTelemetry::default(),
            items,
            live_assistant_output: None,
            streaming_text: if streaming_fixture {
                "I’m checking the implementation and updating the active panel indicators as the response arrives…".into()
            } else {
                String::new()
            },
            streaming_reasoning: String::new(),
            text_reveal: Default::default(),
            reasoning_reveal: Default::default(),
            #[cfg(test)]
            animate_stream_in_tests: false,
            sound_events: crate::sound_events::SoundEvents::default(),
            activity_spinner: cx.new(|cx| activity::Spinner::for_panel(Self::activity_readings(&activity_owner), cx)),
            swarm_workers: Vec::new(),
            background_jobs: Default::default(),
            pending_awaits: Vec::new(),
            pending_await_refresh: None,
            tab_dependency_waiting: false,
            worker_record: None,
            swarm_list_open: false,
            swarm_list_focus: cx.focus_handle(),
            swarm_finished_open: false,
            swarm_finished_focus: cx.focus_handle(),
            swarm_picker_rows: Vec::new(),
            swarm_list_scroll: ScrollHandle::new(),
            swarm_trigger_focus: cx.focus_handle(),
            swarm_marks: Default::default(),
            swarm_usage: Default::default(),
            swarm_row_focus: Default::default(),
            latest_activity_spinner: cx
                .new(|cx| activity::Spinner::for_panel(Self::activity_readings(&activity_owner), cx)),
            tab_outline: cx.new(|cx| tab_outline::TabOutline::new(
                    Live::of(&activity_owner, |panel: &Panel, _| panel.tab_ring()),
                    cx,
                )),
            // The completed-plan fixture shows a turn that finished unseen.
            finished_unseen: crate::harness::screenshot_mode()
                && fixture.as_deref() == Some("todos-completed"),
            sidebar_spinner: cx
                .new(|cx| activity::Spinner::for_sidebar(Self::activity_readings(&activity_owner), cx)),
            surface_focused: true,
            input,
            voice: voice_state::VoiceState::default(),
            image_pane_open: false,
            session_image_gallery: Default::default(),
            session_image_base: 0,
            draft_has_images: false,
            image_pane_selected: None,
            image_pane_scroll: ScrollHandle::new(),
            image_pane_focus: cx.focus_handle().tab_stop(true),
            image_pane_stacked: false,
            image_preview: None,
            image_preview_focus_pending: false,
            pending_history_image: None,
            history_image_requests: std::cell::RefCell::new(HashMap::new()),
            image_preview_dimensions: None,
            diff_review: None,
            edit_previews: diff_review::EditPreviews::default(),
            image_preview_zoom: 1.0,
            image_preview_scroll: gpui::ScrollHandle::new(),
            image_preview_drag: None,
            focus_handle,
            rewind_selection: None,
            transcript_list,
            clean_chat_override: match fixture.as_deref().filter(|_| crate::harness::screenshot_mode()) {
                Some("external-dm-clean" | "btw-clean" | "btw-clean-expanded") => Some(true),
                Some("btw-normal") => Some(false),
                Some("external-dm") => Some(false),
                _ => None,
            },
            pending_approval: None,
            routing_chip: None,
            routing_gate: None,
            expandable_viewport_height: px(0.),
            transcript_row_count: 0,
            transcript_measurements: Default::default(),
            flicker_diagnostics: Default::default(),
            startup_layout: None,
            offscreen_prompt: None,
            offscreen_prompt_clip: None,
            expanded_pinned_prompt: None,
            swarm_scrolls: Default::default(),
            publish_tracker: false,
            transcript_selection,
            transcript_text_document: Default::default(),
            transcript_dragging: false,
            transcript_wheel_glide: WheelGlide::default(),
            transcript_wheel_frame: None,
            transcript_wheel_frame_pending: false,
            tail_glide: Default::default(),
            stick_to_bottom: fixture.as_deref() != Some("transcript-scroll"),
            transcript_end_visible: true,
            composer_dock_height: 0.0,
            steady_activity: Default::default(),
            transcript_end_pad: 0.0,
            pending_history_scroll: None,
            bridge,
            rig_feed,
            rig_host_id,
            rig_demand: None,
            rig_identity,
            preview_state: None,
            demo: false,
            history_loaded: false,
            history_paging: None,
            reconnect_response: None,
            restored_transcript: false,
            provisional_items: 0,
            provisional_task: None,
            awaiting_persisted: false,
            draft_session: DraftSession::Requested,
            draft_config: DraftConfig::default(),
            speed: speed::SessionSpeed::default(),
            expanded_regions,
            tool_detail_motion: HashMap::new(),
            prompt_queue: queue::PromptQueue::default(),
            pending_users: VecDeque::new(),
            accepted_users: HashMap::new(),
            user_times: HashMap::new(),
            arriving_tools: HashMap::new(),
            applet_positions: HashMap::new(),
            applet_selection: cx.new(TextSelection::new),
            applet_generation: (u64::MAX, 0, false),
            applet_rows: Vec::new(),
            applet_rows_version: 0,
            transcript_derived: None,
            control_reveal: None,
            tool_started: HashMap::new(),
            tool_durations: fixture_tool_durations,
            tool_wall_times: fixture_tool_wall_times,
            codemode_summaries: fixture_codemode_summaries,
            running_tool_tickers: Default::default(),
            superseded_drafts: Default::default(),
            unfinished_work: None,
            unfinished_session_opener: None,
            side_document: None,
            applet: None,
            todoist: (crate::harness::screenshot_mode() && fixture.as_deref() == Some("todos"))
                .then(|| TodoistPanelState::fixture(80)),
            openrig,
            orchestration: (crate::harness::screenshot_mode()
                && fixture.as_deref() == Some("orchestration"))
            .then(orchestration::State::fixture),
            recovery_picker_open: false,
            ask_card: None,
            queued_asks: VecDeque::new(),
            ask_focus_return: None,
            ask_recovery: None,
            answered_asks: HashSet::new(),
            unavailable_asks: HashMap::new(),
            ask_attachment_id: 0,
            queue_items_scroll: queue_items_scroll.clone(),
            queue_edit_anchor: gpui::ScrollAnchor::for_handle(queue_items_scroll),
            pending_ask_draft: None,
            side_question: None,
            side_question_history: None,
            side_question_resize: crate::input::VerticalResize::collapsible(),
            composer_overlay_room: Default::default(),
            model_picker_open: false,
            picker_search: None,
            picker_triggers: std::array::from_fn(|_| cx.focus_handle()),
            composer_popover_watch: None,
            composer_popover_trigger: None,
            focus_pending: false,
            transcript_only: false,
            search_terms: Vec::new(),
            login: None,
            available_models: Vec::new(),
        }
    }

    /// Wire the input's submit to also echo locally. Called once after
    /// creation, when we have the panel entity.
    pub fn connect_input(panel: &Entity<Panel>, cx: &mut App) {
        if panel.read(cx).is_side_document() {
            return;
        }
        let weak = panel.downgrade();
        panel.update(cx, |this, cx| {
            let cancel_weak = weak.clone();
            let change_weak = weak.clone();
            let speed_weak = weak.clone();
            let session_image_base = this.session_image_base;
            this.input = cx.new(|cx| {
                let mut input = PromptInput::new_with_queue_files(
                    cx,
                    "Type something…",
                    move |kind, content, images, files, queued, _window, app| {
                        let Some(panel) = weak.upgrade() else { return };
                        if kind != crate::input::SubmissionKind::Command {
                            panel.update(app, |this, cx| {
                                if this.preview_state.is_some() {
                                    return;
                                }
                                if !content.trim().is_empty()
                                    || !images.is_empty()
                                    || !files.is_empty()
                                    || (!queued && this.has_visible_queued_prompts())
                                {
                                    let restore = match kind {
                                        crate::input::SubmissionKind::Skill(name) => Some(this.active_skill.replace(name)),
                                        _ => None,
                                    };
                                    this.submit_or_queue_with_files(
                                        content, images, files, queued, cx,
                                    );
                                    if let Some(previous) = restore {
                                        this.active_skill = previous;
                                    }
                                }
                            });
                            return;
                        }
                        Panel::dispatch_composer_command(&panel, content, _window, app);
                    },
                )
                .with_empty_submit()
                .with_standalone_btw()
                .with_example_prompts()
                .with_on_model_speed(move |choice, app| {
                    let _ =
                        speed_weak.update(app, |panel, cx| panel.choose_model_speed(choice, cx));
                })
                .with_ghost_completions({
                    let weak = change_weak.clone();
                    move |app| {
                        weak.upgrade()
                            .map(|panel| panel.read(app).completion_context())
                            .unwrap_or_default()
                    }
                })
                .with_on_change(move |content, app| {
                    if let Some(panel) = change_weak.upgrade() {
                        panel.update(app, |this, cx| {
                            if content == "/model" || content.starts_with("/model ") {
                                this.seed_draft_models(cx);
                            }
                            if (content == "/model" || content.starts_with("/model "))
                                && (!this.is_pending_session() || !this.available_models.is_empty())
                            {
                                this.model_picker_open = true;
                                cx.notify();
                            } else if this.model_picker_open && !content.starts_with("/model ") {
                                this.close_model_picker(cx);
                            }
                        });
                    }
                })
                .with_on_overlay_cancel(move |app| {
                    let mut handled = false;
                    if let Some(panel) = cancel_weak.upgrade() {
                        panel.update(app, |this, cx| {
                            if this.recovery_picker_open {
                                this.recovery_picker_open = false;
                                handled = true;
                            } else if this.model_picker_open {
                                this.close_model_picker(cx);
                                handled = true;
                            } else if !this.is_pending_session()
                                && (this.activity_active()
                                    || this.has_unconfirmed_stop()
                                    || !this.pending_users.is_empty())
                            {
                                this.prompt_queue.paused = true;
                                this.sound_events.cancel();
                                this.request_user_stop(cx);
                                handled = true;
                            }
                            cx.notify();
                        });
                    }
                    handled
                });
                input.set_session_image_base(session_image_base, cx);
                input.set_speed_picker(this.speed_picker(), cx);
                input.enable_context_picker();
                input
            });
            Self::observe_draft_images(&this.input, cx);
            if std::env::var("JCODE_DESKTOP_SCREENSHOT").as_deref() == Ok("1")
                && std::env::var("JCODE_DESKTOP_SCREENSHOT_MODELS").as_deref() == Ok("1")
            {
                let names = [
                    "anthropic:sonnet-review".to_string(),
                    "google:gemini-review".to_string(),
                ]
                .into_iter()
                .chain((1..=12).map(|index| format!("openai:atlas-{index:02}")));
                let routes = names
                    .map(|model| {
                        let provider = model.split(':').next().unwrap().to_string();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let usage = match provider.as_str() {
                            "anthropic" => Some(jcode_sdk::ModelUsage {
                                count: 42,
                                last_used_unix_secs: Some(now - 7200),
                                tracking_started_unix_secs: Some(now - 86400 * 14),
                                selection_count: 5,
                                last_selected_unix_secs: Some(now - 7200),
                            }),
                            "google" => Some(jcode_sdk::ModelUsage {
                                count: 7,
                                last_used_unix_secs: Some(now - 86400 * 3),
                                tracking_started_unix_secs: Some(now - 86400 * 14),
                                selection_count: 2,
                                last_selected_unix_secs: Some(now - 86400 * 3),
                            }),
                            _ => None,
                        };
                        jcode_sdk::ModelRouteInfo {
                            speed_options: Vec::new(),
                            reasoning_efforts: None,
                            api_method: format!("{provider}-api-key"),
                            model,
                            provider,
                            available: true,
                            detail: String::new(),
                            usage,
                        }
                    })
                    .collect();
                this.apply(
                    &ApiEvent::RuntimeInfo {
                        speed_mode: Default::default(),
                        speed_options: Vec::new(),
                        session_id: this.session_id.clone(),
                        provider: Some("openai".into()),
                        model: Some("openai:atlas-01".into()),
                        reasoning_effort: None,
                        auth_method: None,
                        account_label: None,
                        routes,
                    },
                    cx,
                );
            }
            if this.is_pending_session() {
                this.draft_session = DraftSession::Unrequested;
                this.status = "Starting session · you can type now".into();
                this.input
                    .update(cx, |input, cx| input.set_pending_session(true, cx));
            }
        });
    }

    pub fn snapshot(&self, cx: &App) -> PanelSnapshot {
        let offset = self
            .document_scroll_offset(cx)
            .unwrap_or_else(|| self.transcript_list.scroll_px_offset_for_scrollbar());
        PanelSnapshot {
            draft_config: self.draft_config.clone(),
            clean_chat_override: self.clean_chat_override,
            transcript: None,
            turn_outcome: self.turn_outcome,
            finished_unseen: self.finished_unseen,
            image_pane_open: self.image_pane_open,
            side_document: self.document_snapshot(cx),
            prompt_queue: self.prompt_queue.clone(),
            session_id: self.session_id.clone(),
            title: self.title.to_string(),
            working_dir: self.working_dir.clone(),
            draft: self.input.read(cx).snapshot(),
            ask_draft: self
                .ask_card
                .as_ref()
                .map(|card| card.draft(cx))
                .or_else(|| self.pending_ask_draft.clone()),
            scroll_x: f32::from(offset.x),
            scroll_y: f32::from(offset.y),
            stick_to_bottom: self.stick_to_bottom,
            startup_layout: self
                .startup_layout
                .clone()
                .filter(|layout| layout.committed),
            terminal_resource_id: None,
            terminal_output_cursor: None,
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: PanelSnapshot, cx: &mut Context<Self>) {
        if self.restore_side_document_snapshot(&snapshot, cx) {
            return;
        }
        if let Some(transcript) = snapshot.transcript {
            self.restore_transcript(transcript);
        } else {
            self.turn_outcome = snapshot.turn_outcome;
        }
        self.finished_unseen = snapshot.finished_unseen;
        self.clean_chat_override = snapshot.clean_chat_override;
        self.image_pane_open = snapshot.image_pane_open;
        self.image_pane_selected = None;
        self.prompt_queue = snapshot.prompt_queue;
        self.prompt_queue.replay_pending_on_connect = true;
        self.pending_ask_draft = snapshot.ask_draft;
        if let (Some(draft), Some(card)) = (&self.pending_ask_draft, &self.ask_card)
            && draft.matches_questions(&card.questions)
        {
            let draft = self.pending_ask_draft.take().expect("checked above");
            self.ask_card
                .as_mut()
                .expect("checked above")
                .restore_draft(draft, cx);
        }
        self.title = snapshot.title.into();
        self.working_dir = snapshot.working_dir;
        if self.is_pending_session() {
            self.draft_config = snapshot.draft_config;
            if let Some(model) = self.draft_config.model.clone() {
                self.pick_draft_model(&model, cx);
            }
            self.reasoning_effort.clone_from(&self.draft_config.effort);
            self.pinned_modes.clone_from(&self.draft_config.modes);
            self.sync_effort_menu(cx);
            self.sync_mode_menu(cx);
        }
        self.stick_to_bottom = snapshot.stick_to_bottom;
        self.startup_layout = snapshot.startup_layout;
        self.pending_history_scroll =
            (!snapshot.stick_to_bottom).then_some((snapshot.scroll_x, snapshot.scroll_y));
        self.transcript_list
            .set_offset_from_scrollbar(point(px(snapshot.scroll_x), px(snapshot.scroll_y)));
        self.input.update(cx, |input, cx| {
            input.restore(snapshot.draft, cx);
            input.set_working_dir(self.working_dir.as_deref());
        });
        self.schedule_ask_recovery(cx);
    }

    pub(super) fn observe_draft_images(input: &Entity<PromptInput>, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        gpui::App::observe(cx, input, move |input, cx| {
            let has_images = input.read(cx).has_draft_images();
            if owner
                .read_with(cx, |panel, _| panel.draft_has_images != has_images)
                .unwrap_or(false)
            {
                let _ = owner.update(cx, |panel, cx| {
                    panel.draft_has_images = has_images;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn new_accounts(
        source_session: &str,
        preview: Option<PreviewState>,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            format!("accounts://{source_session}"),
            Some("accounts".into()),
            None,
            bridge,
            cx,
        );
        panel.items.clear();
        panel.preview_state = preview;
        panel.open_login_picker(cx);
        panel
    }

    pub(crate) fn choose_account_model(&mut self, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        self.open_recovery_models(cx);
    }

    pub(crate) fn can_refresh_account_runtime(&self) -> bool {
        self.can_fork()
            && self.todoist.is_none()
            && self.orchestration.is_none()
            && self.openrig.is_none()
    }

    pub(crate) fn is_accounts_panel(&self) -> bool {
        self.session_id.starts_with("accounts://")
    }

    pub fn new_unfinished_work(
        sessions: Vec<crate::harness::UnfinishedSession>,
        session_opener: SessionOpener,
        bridge: Bridge,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(
            "unfinished-work".into(),
            Some("unfinished work".into()),
            None,
            bridge,
            cx,
        );
        panel.unfinished_work = Some(sessions);
        panel.unfinished_session_opener = Some(session_opener);
        panel
    }

    pub fn set_unfinished_work(
        &mut self,
        sessions: Vec<crate::harness::UnfinishedSession>,
        cx: &mut Context<Self>,
    ) {
        if self.unfinished_work.as_ref() != Some(&sessions) {
            self.unfinished_work = Some(sessions);
            cx.notify();
        }
    }
}

impl Panel {
    pub(super) fn dispatch_composer_command(
        panel: &Entity<Self>,
        command: String,
        window: &mut Window,
        app: &mut App,
    ) {
        use crate::{commands, ui_actions};

        if Self::dispatch_workspace_command(panel, &command, window, app) {
            return;
        }
        if commands::is_resume_command(&command) {
            window.dispatch_action(Box::new(ui_actions::OpenResume), app);
            return;
        }
        if commands::is_accounts_command(&command) || command == "/subscribe" {
            window.dispatch_action(
                Box::new(ui_actions::OpenAccounts {
                    source: panel.entity_id(),
                    login_command: None,
                }),
                app,
            );
            return;
        }
        if let Some(command) = commands::reset_command(&command) {
            let provider = match command {
                commands::ResetCommand::OpenAi => crate::accounts::ResetProvider::OpenAi,
                commands::ResetCommand::Claude => crate::accounts::ResetProvider::Claude,
                commands::ResetCommand::Current => panel.read(app).reset_provider(),
            };
            window.dispatch_action(Box::new(ui_actions::OpenUsageReset { provider }), app);
            return;
        }
        match command.as_str() {
            "/applets" => window.dispatch_action(Box::new(ui_actions::OpenAppletShowcase), app),
            "/onboarding-sim" | "/onboarding-preview" => {
                window.dispatch_action(Box::new(ui_actions::ToggleOnboardingSimulator), app);
            }
            "/changelog" => window.dispatch_action(Box::new(ui_actions::OpenChangelog), app),
            "/publish" if panel.read(app).publish_checkout().is_some() => {
                window.dispatch_action(
                    Box::new(ui_actions::PublishDesktop {
                        source: panel.entity_id(),
                    }),
                    app,
                );
            }
            _ if command.split_whitespace().next() == Some("/login") => {
                window.dispatch_action(
                    Box::new(ui_actions::OpenAccounts {
                        source: panel.entity_id(),
                        login_command: Some(command),
                    }),
                    app,
                );
            }
            _ => panel.update(app, |this, cx| {
                this.handle_slash_command(&command, cx);
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn handle_slash_command_for_test(
        &mut self,
        content: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        self.handle_slash_command(content, cx)
    }

    pub(super) fn handle_slash_command(&mut self, content: &str, cx: &mut Context<Self>) -> bool {
        if self.is_side_document() {
            return true;
        }
        self.transcript_measurements.invalidate();
        if self.preview_state.is_some() {
            return self.handle_preview_command(content, cx);
        }
        let trimmed = content.trim();
        if let Some(mode) = crate::speed_icon::command_tier(trimmed) {
            self.choose_speed(mode, cx);
            return true;
        }
        if self.session_deferred() && trimmed.split_whitespace().next() == Some("/mode") {
            self.select_draft_mode(trimmed["/mode".len()..].trim(), cx);
            return true;
        }
        if self.handle_skill_invocation(trimmed, cx) {
            return true;
        }
        if trimmed == "/rewind" {
            self.start_rewind(cx);
            return true;
        }
        if self.handle_clean_chat_command(trimmed, cx) {
            return true;
        }
        if self.handle_special_command(trimmed, cx)
            || self.handle_recovery_command(trimmed, cx)
            || self.handle_runtime_command(trimmed, cx)
            || self.handle_desktop_command(trimmed, cx)
        {
            self.stick_to_bottom = true;
            self.transcript_list.scroll_to_end();
            return true;
        }
        if matches!(trimmed, "/improve stop" | "/refactor stop") {
            self.pause_queue_for_stop();
            self.request_user_stop(cx);
            self.items.push(Item::Assistant("Stopped the active workflow. Queued prompts are preserved and paused, and no continuation was submitted.".into()));
            cx.notify();
            return true;
        }
        if matches!(trimmed, "/improve status" | "/refactor status") {
            let payload = self.latest_todo_payload().unwrap_or_default();
            self.items.push(Item::Todos(payload));
            self.items.push(Item::Assistant(format!(
                "Session workflow status: {}. Use `{}` to continue existing work.",
                self.status,
                trimmed.replace("status", "resume")
            )));
            cx.notify();
            return true;
        }
        if let Some(prompt) = jcode_desktop_model::command_workflows::workflow_prompt(trimmed) {
            self.submit_or_queue(prompt, Vec::new(), true, cx);
            return true;
        }
        if self.handle_acct_command(trimmed, cx) {
            return true;
        }
        if self.login_command(trimmed, cx) {
            return true;
        }
        if let Some(command) = queue::PokeCommand::parse(trimmed) {
            match command {
                Ok(command) => self.handle_poke_command(command, cx),
                Err(usage) => self.items.push(Item::Error(usage)),
            }
            cx.notify();
            return true;
        }
        if let Some((model, effort)) = trimmed
            .strip_prefix("/model ")
            .map(crate::input::parse_model_pick)
            .filter(|(model, _)| !model.is_empty())
        {
            self.close_model_picker(cx);
            if let Some(effort) = effort {
                self.switch_model_with_effort(model, &effort.to_ascii_lowercase(), cx);
            } else if self.is_pending_session() {
                self.pick_draft_model(model, cx);
                self.remember_draft_model(cx);
            } else {
                self.bridge.send(Command::SetModel {
                    session_id: self.session_id.clone(),
                    model: model.to_string(),
                });
                self.items
                    .push(Item::Assistant(format!("Switching model to `{model}`…")));
            }
        } else if let Some(effort) = trimmed.strip_prefix("/effort ").map(str::trim) {
            let effort = effort.to_ascii_lowercase();
            if self.request_effort(&effort, cx) {
                if self
                    .picker_search
                    .as_ref()
                    .is_some_and(|(kind, _)| *kind == PickerKind::Effort)
                {
                    self.picker_search = None;
                    self.focus_pending = true;
                } else {
                    let input = self.input.clone();
                    cx.defer(move |cx| input.update(cx, |input, cx| input.close_effort_menu(cx)));
                }
                if !self.is_pending_session() {
                    self.items.push(Item::Assistant(format!(
                        "Reasoning effort set to `{effort}`."
                    )));
                }
            }
        } else if let Some(title) = trimmed.strip_prefix("/rename ").map(str::trim) {
            if title.is_empty() {
                self.items.push(Item::Error(
                    "Usage: `/rename <session name>` or `/rename --clear`.".into(),
                ));
            } else {
                let title = (title != "--clear").then(|| title.to_string());
                self.run_session_operation(SessionOperation::Rename(title), "Session renamed.");
            }
        } else if trimmed == "/save" || trimmed.starts_with("/save ") {
            let label = trimmed["/save".len()..].trim();
            let label = (!label.is_empty()).then(|| label.to_string());
            self.request_session_saved(true, label);
        } else if trimmed == "/unsave" {
            self.request_session_saved(false, None);
        } else if let Some(target) = trimmed.strip_prefix("/rewind ").map(str::trim) {
            if target == "undo" {
                self.run_session_operation(SessionOperation::RewindUndo, "Rewind undone.");
            } else if let Ok(index) = target.parse::<usize>() {
                if index == 0 {
                    self.items
                        .push(Item::Error("Rewind numbering starts at 1.".into()));
                } else {
                    self.run_session_operation(
                        SessionOperation::Rewind(index),
                        format!("Rewound to message {index}."),
                    );
                }
            } else {
                self.items.push(Item::Error(
                    "Usage: `/rewind <message number|undo>`.".into(),
                ));
            }
        } else {
            match trimmed {
                "/cancel" => {
                    self.request_user_stop(cx);
                }
                "/cls" | "/clear-view" => {
                    self.startup_layout = None;
                    self.image_pane_selected = None;
                    self.items.clear();
                    self.user_times.clear();
                    self.response_stats = response_stats::Tracker::default();
                    self.streaming_text.clear();
                    self.live_assistant_output = None;
                    self.streaming_reasoning.clear();
                }
                "/clear" => self.run_session_operation(
                    SessionOperation::Clear,
                    "Conversation history cleared.",
                ),
                "/compact" => self.run_session_operation(
                    SessionOperation::Compact,
                    "Context compacted.",
                ),
                "/help" | "/commands" | "/?" => {
                    self.items.push(Item::Assistant(help_markdown()))
                }
                "/todos" | "/todo" => {
                    if let Some(payload) = self.latest_todo_payload() {
                        self.items.push(Item::Todos(payload));
                    } else {
                        self.items.push(Item::Todos(TodoCardPayload::default()));
                    }
                }
                "/model" | "/models" => self.open_model_picker_deferred(cx),
                "/update" if crate::updates::paired_configured() => {
                    // The workspace owns the update flow, so the panel asks it.
                    cx.dispatch_action(&crate::ui_actions::CheckForUpdates);
                    self.items.push(Item::Assistant("Checking for a paired CLI and Desktop update…".into()));
                }
                "/update" => {
                    let request = crate::updates::request_now();
                    let message = match request {
                        crate::updates::UpdateRequest::Checking =>
                            "Checking for Jcode Desktop updates…",
                        crate::updates::UpdateRequest::AlreadyChecking =>
                            "Jcode Desktop is already checking for updates.",
                        crate::updates::UpdateRequest::Downloading =>
                            "A Jcode Desktop update is downloading in the background.",
                        crate::updates::UpdateRequest::Restarting =>
                            "Installing the update and restarting Jcode Desktop…",
                        crate::updates::UpdateRequest::Unavailable =>
                            "Automatic updates are unavailable in this build of Jcode Desktop.",
                    };
                    self.items.push(Item::Assistant(message.into()));
                    #[cfg(not(target_os = "macos"))]
                    if request == crate::updates::UpdateRequest::Checking {
                        cx.spawn(async move |this, cx| {
                            loop {
                                // The worker owns IO, never the UI thread. Surface
                                // its terminal result even if the user is idle.
                                let result = match crate::updates::current() {
                                    crate::updates::UpdateState::Finished { message } => Some(Ok(message)),
                                    crate::updates::UpdateState::Failed { message } => Some(Err(message)),
                                    crate::updates::UpdateState::ReadyToRestart { version } => Some(Ok(format!(
                                        "Jcode Desktop {version} is installed. Run `/update` again or click the update chip to restart into it now, or it starts the next time you open Jcode Desktop."
                                    ))),
                                    crate::updates::UpdateState::Idle => break,
                                    _ => None,
                                };
                                if let Some(result) = result {
                                    let _ = this.update(cx, |panel, cx| {
                                        panel.items.push(match result {
                                            Ok(message) => Item::Assistant(message),
                                            Err(message) => Item::Error(message),
                                        });
                                        cx.notify();
                                    });
                                    break;
                                }
                                if this.upgrade().is_none() { break; }
                                cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                            }
                        }).detach();
                    }
                }
                "/effort" => {
                    let ladder = self.effort_ladder();
                    let current = self.reasoning_effort.as_deref().unwrap_or("default");
                    let keys = crate::effort::bound_keys_label(cx)
                        .map(|keys| format!(" or {keys}"))
                        .unwrap_or_default();
                    self.items.push(Item::Assistant(if ladder.is_empty() {
                        "Reasoning effort is not available for this model.".into()
                    } else {
                        format!(
                            "Effort: `{}`. Use `/effort <{}>`{keys} to change it.",
                            crate::effort::spoken(current),
                            ladder.join("|")
                        )
                    }));
                }
                "/rename" => self.items.push(Item::Error(
                    "Usage: `/rename <session name>` or `/rename --clear`.".into(),
                )),
                "/commit" => self.submit_command_prompt(
                    "Make interactive, logical commits for the current uncommitted work. Inspect git state first, group related changes into coherent commits, preserve unrelated work, validate appropriately, and report the commits created plus remaining changes.",
                    cx,
                ),
                "/publish" => {
                    let item = match self.publish_checkout() {
                        Some(_) => Item::Assistant(
                            "Opened the publish tracker. Commit, push, release, and publish progress appears in its panel.".into(),
                        ),
                        None => Item::Error(
                            "`/publish` is only available in Jcode Desktop self-development sessions.".into(),
                        ),
                    };
                    self.items.push(item);
                }
                "/restart-all" | "/restart-all desktop" => {
                    let restart_server = trimmed == "/restart-all";
                    self.items.push(match crate::restart_spawn::spawn_worker(restart_server) {
                        Ok(()) => Item::Assistant(if restart_server {
                            "Restarting the Jcode server and every Desktop window. Windows, sessions, drafts, and layout come back in a few seconds.".into()
                        } else {
                            "Restarting every Desktop window. Windows, sessions, drafts, and layout come back in a few seconds.".into()
                        }),
                        Err(error) => Item::Error(format!("Could not start the restart: {error}")),
                    });
                }
                "/commit-push" | "/commit-and-push" => self.submit_command_prompt(
                    "Make logical commits for the current uncommitted work, preserving unrelated work and validating appropriately. Then push to the tracking branch without force-pushing, and report the commits and push result.",
                    cx,
                ),
                _ if trimmed.split_whitespace().next().and_then(registered_command).is_some()
                    || jcode_base::skill::SkillRegistry::parse_invocation(trimmed).is_some() => self.items.push(Item::Error(command_unavailable_message(trimmed))),
                _ => return false,
            }
        }
        self.stick_to_bottom = true;
        self.transcript_list.scroll_to_end();
        cx.notify();
        true
    }

    pub(super) fn handle_skill_invocation(&mut self, content: &str, cx: &mut Context<Self>) -> bool {
        self.handle_skill_invocation_with_images(content, Vec::new(), false, cx)
    }

    pub(super) fn handle_skill_invocation_with_images(
        &mut self,
        content: &str,
        images: Vec<(String, String)>,
        queued: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = content.strip_prefix('/') else {
            return false;
        };
        let Some((name, description)) = self
            .skills
            .iter()
            .filter(|(name, _)| {
                rest.strip_prefix(name)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
                    && (name == "selfdev"
                        || crate::commands::registered_command(&format!("/{name}")).is_none())
            })
            .max_by_key(|(name, _)| name.len())
            .cloned()
        else {
            return false;
        };
        let prompt = rest[name.len()..].trim();
        let prompt = match prompt.as_bytes() {
            [b'"', .., b'"'] | [b'\'', .., b'\''] if prompt.len() >= 2 => {
                &prompt[1..prompt.len() - 1]
            }
            _ => prompt,
        }
        .to_string();
        self.active_skill = Some(name.clone());
        self.sync_mode_menu(cx);
        self.items.push(Item::Assistant(format!(
            "Activated skill: {name} - {description}"
        )));
        if !prompt.is_empty() || !images.is_empty() {
            self.submit_or_queue(prompt, images, queued, cx);
        }
        cx.notify();
        true
    }

    /// Compose cards a later Gmail call replaced, recomputed only when the
    /// transcript changes.
    fn superseded_drafts(&self) -> std::rc::Rc<HashMap<String, gmail_draft_card::Superseded>> {
        let revision = self.items.revision();
        let mut cache = self.superseded_drafts.borrow_mut();
        if cache.0 != revision {
            let tools = self.items.iter().filter_map(|item| match item {
                Item::Tool {
                    call_id,
                    name,
                    input,
                    output,
                    done,
                    error,
                } => Some((
                    call_id.as_str(),
                    name.as_str(),
                    input.as_str(),
                    output.as_str(),
                    *done,
                    error.as_deref(),
                )),
                _ => None,
            });
            *cache = (
                revision,
                std::rc::Rc::new(gmail_draft_card::superseded_cards(tools)),
            );
        }
        cache.1.clone()
    }
}

impl Panel {
    /// Hold the runtime session back until the user commits to this draft.
    /// A draft that was already requested keeps its in-flight request.
    pub(crate) fn defer_session(&mut self, cx: &mut Context<Self>) {
        if self.is_pending_session() && self.draft_session == DraftSession::Unrequested {
            self.draft_session = DraftSession::Deferred;
            self.load_draft_model_defaults(cx);
            if self.has_queued_prompts() {
                // Sent before the runtime was reachable: the commitment is
                // already made.
                self.request_deferred_session(cx);
                return;
            }
            self.status = DEFERRED_SESSION_STATUS.into();
            self.seed_draft_models(cx);
            self.load_draft_skills(cx);
            cx.notify();
        }
    }

    pub(super) fn load_draft_model_defaults(&mut self, cx: &mut Context<Self>) {
        if self.draft_config.model.is_some() || self.draft_config.effort.is_some() {
            return;
        }
        let defaults = cx
            .try_global::<RememberedDraftModel>()
            .map(|defaults| defaults.0.clone())
            .unwrap_or_else(|| {
                #[cfg(not(test))]
                {
                    crate::config::get().models.draft_defaults.clone()
                }
                #[cfg(test)]
                crate::config::DraftModelDefaults::default()
            });
        if let Some(model) = defaults.model {
            self.pick_draft_model(&model, cx);
        }
        self.draft_config.effort = defaults.effort.clone();
        self.reasoning_effort = defaults.effort;
        self.sync_effort_menu(cx);
    }

    pub(super) fn remember_draft_model(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        let defaults = crate::config::DraftModelDefaults {
            model: self
                .draft_config
                .model
                .clone()
                .or_else(|| self.model.clone()),
            effort: self.draft_config.effort.clone(),
        };
        if let Err(error) = crate::config::persist_draft_model_defaults(&defaults) {
            self.effort_notice(format!("Could not save new session defaults: {error}"), cx);
            return;
        }
        cx.set_global(RememberedDraftModel(defaults));
    }

    /// The workspace sent, or is about to send, this draft's create request.
    pub(crate) fn mark_session_requested(&mut self) {
        self.draft_session = DraftSession::Requested;
    }

    /// True while nothing has asked the runtime to create this draft's session.
    pub(crate) fn session_deferred(&self) -> bool {
        self.is_pending_session() && self.draft_session == DraftSession::Deferred
    }

    /// Prefer the daemon's unbound catalog to any previously observed session.
    pub(crate) fn seed_draft_models(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        let catalog = cx.try_global::<LocalDraftModels>().cloned().or_else(|| {
            cx.try_global::<KnownModelRoutes>()
                .map(|known| LocalDraftModels {
                    current: None,
                    routes: known.0.clone(),
                })
        });
        let Some(LocalDraftModels { current, routes }) = catalog.filter(|c| !c.routes.is_empty())
        else {
            return;
        };
        if self.draft_config.model.is_none() && current.is_some() {
            self.model = current;
        }
        if cx.try_global::<LocalDraftModels>().is_some() {
            self.sync_draft_speed(&routes);
            self.sync_open_speed_picker(cx);
        }
        let models = available_model_names(&routes);
        self.available_models = models.clone();
        let panel = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = panel.update(cx, |panel, cx| {
                if !panel.session_deferred() {
                    return;
                }
                let current = panel.draft_config.model.clone().or_else(|| panel.model.clone());
                panel.input.update(cx, |input, cx| {
                    input.set_model_routes(models, &routes, current, cx);
                });
                if panel.recovery_picker_open {
                    panel.open_model_picker(cx);
                }
                panel.sync_effort_menu(cx);
                cx.notify();
            });
        });
    }

    pub(crate) fn publish_draft_models(
        current: Option<String>,
        routes: Vec<jcode_sdk::ModelRouteInfo>,
        cx: &mut App,
    ) {
        if !routes.is_empty() {
            cx.set_global(LocalDraftModels { current, routes });
        }
    }

    /// Record a model picked on a draft. Nothing is sent until the session
    /// exists.
    pub(crate) fn pick_draft_model(&mut self, model: &str, cx: &mut Context<Self>) {
        if self.draft_config.model.as_deref().or(self.model.as_deref()) != Some(model) {
            self.draft_config.speed = None;
            self.speed.clear_options();
        }
        self.draft_config.model = Some(model.to_string());
        if let Some(local) = cx.try_global::<LocalDraftModels>() {
            self.sync_draft_speed(&local.routes);
        }
        if self
            .draft_config
            .effort
            .as_deref()
            .is_some_and(|level| !crate::effort::ladder(None, Some(model)).contains(&level))
        {
            self.draft_config.effort = None;
            self.reasoning_effort = None;
        }
        // The runtime reports the bare id once it serves the model. Show the
        // same name now, without the picker's route prefix.
        let shown = match model.split_once(':') {
            Some((route, bare)) if !route.contains('/') && !bare.is_empty() => bare,
            _ => model,
        };
        self.model = Some(shown.to_string());
        // Deferred: `/model` runs inside the composer's own submit callback.
        let input = self.input.clone();
        let current = Some(model.to_owned());
        cx.defer(move |cx| input.update(cx, |input, cx| input.set_current_model(current, cx)));
        self.sync_effort_menu(cx);
        self.sync_open_speed_picker(cx);
        cx.notify();
    }

    /// Send the draft's picked model and effort to its new session, ahead of
    /// the prompts queued behind them.
    pub(super) fn apply_draft_config(&mut self, cx: &mut Context<Self>) {
        let ladder = self.effort_ladder();
        let DraftConfig {
            model,
            effort,
            speed,
            modes,
        } = std::mem::take(&mut self.draft_config);
        let (model, effort, speed) = match (model, effort, speed) {
            (Some(model), effort, Some(speed)) => {
                let safe = self.safe_draft_speed(speed);
                self.send_model_speed(model, effort, safe, cx);
                self.report_draft_speed_fallback(speed, safe, cx);
                (None, None, None)
            }
            config => config,
        };
        if let Some(model) = model {
            self.bridge.send(Command::SetModel {
                session_id: self.session_id.clone(),
                model,
            });
        }
        if let Some(effort) = effort {
            self.reasoning_effort = None;
            self.request_effort_on(&ladder, &effort, cx);
        }
        if let Some(speed) = speed {
            self.apply_draft_speed(speed, cx);
        }
        for mode in modes {
            self.bridge.send(Command::RuntimeCommand {
                session_id: self.session_id.clone(),
                command: jcode_sdk::api::RuntimeCommand::Mode,
                args: vec!["toggle".into(), mode],
            });
        }
    }

    /// Create the session a deferred draft stands for. Called when the user
    /// sends a prompt or reaches for something only a runtime session has.
    pub(crate) fn request_deferred_session(&mut self, cx: &mut Context<Self>) {
        if !self.session_deferred() {
            return;
        }
        self.draft_session = DraftSession::Requested;
        self.status = "Starting session · you can type now".into();
        self.bridge.send(Command::CreateSession {
            working_dir: self.working_dir.clone(),
            request_id: Some(self.session_id.clone()),
        });
        cx.notify();
    }

    /// A draft restored after a restart resumes where it stood: prompts that
    /// were queued need their session now, an untouched editor does not.
    pub(crate) fn resume_draft_session(&mut self, needs_session: bool, cx: &mut Context<Self>) {
        self.draft_session = DraftSession::Deferred;
        if needs_session || self.has_queued_prompts() {
            self.request_deferred_session(cx);
        } else {
            self.status = DEFERRED_SESSION_STATUS.into();
            self.seed_draft_models(cx);
            self.load_draft_skills(cx);
            cx.notify();
        }
    }

    /// Promote in place so focus, selection, undo history, and pasted images
    /// all survive the asynchronous runtime connection.
    pub fn attach_startup_session(
        &mut self,
        session: jcode_sdk::SessionInfo,
        cx: &mut Context<Self>,
    ) {
        self.context_usage = context_usage::State::default();
        self.session_id = session.session_id;
        self.pending_await_refresh = None;
        self.pending_awaits.clear();
        self.refresh_local_pending_awaits(cx);
        self.title = session
            .title
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| short_id(&self.session_id))
            .into();
        // Older bridges report no directory for a session that has not
        // persisted yet. Keep the draft's requested directory in that case.
        if let Some(dir) = session.working_dir.filter(|dir| !dir.trim().is_empty()) {
            self.working_dir = Some(dir);
        }
        self.status = "idle".into();
        self.input.update(cx, |input, cx| {
            input.set_submission_enabled(true, cx);
            input.set_pending_session(!self.history_loaded, cx);
        });
        self.apply_draft_config(cx);
        self.send_queued_prompts(cx);
        cx.notify();
    }
}

impl Panel {
    pub(super) fn restore_tool_times(&mut self, messages: &[jcode_sdk::HistoryMessage]) {
        for (id, (start, finish, duration)) in history_tool_times(messages) {
            self.tool_wall_times.insert(id.clone(), (start, finish));
            if let Some(duration) = duration {
                self.tool_durations.insert(id, duration);
            }
        }
    }

    /// Record when the user prompt at `index` was sent.
    pub(crate) fn stamp_user(&mut self, index: usize) {
        if let Some(at) = response_stats::now_ms() {
            self.user_times.insert(index, at);
        }
    }

    /// Arrival time of the user prompt at `index`, as `HH:MM` local time.
    pub(crate) fn user_clock(&self, index: usize) -> Option<String> {
        self.user_times
            .get(&index)
            .copied()
            .and_then(response_stats::clock_label)
    }

    /// Shift index-keyed prompt times after items were inserted or removed
    /// before them.
    pub(super) fn shift_user_times(&mut self, added: usize, removed: usize) {
        self.user_times = std::mem::take(&mut self.user_times)
            .into_iter()
            .filter_map(|(index, at)| Some((index.checked_sub(removed)? + added, at)))
            .collect();
    }

    /// Paint the locally persisted transcript while the runtime attaches.
    /// Attaching restores the agent before `get_history` can answer, which
    /// takes hundreds of milliseconds. The stored record renders in a few, so
    /// the reader sees the conversation at once and the authoritative reply
    /// swaps in underneath (usually as a no-op when nothing changed).
    pub fn prefetch_persisted_history(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || crate::harness::screenshot_mode() {
            return;
        }
        let id = self.session_id.clone();
        if !crate::persisted_history::is_local_session_id(&id)
            || !jcode_base::session::session_path(&id).is_ok_and(|path| path.exists())
        {
            return;
        }
        self.prefetch_history_with(move || crate::persisted_history::load(&id), cx);
    }

    pub(crate) fn prefetch_history_with(
        &mut self,
        loader: impl FnOnce() -> Result<crate::persisted_history::History, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if !self.can_show_provisional() {
            return;
        }
        self.awaiting_persisted = true;
        let started = Instant::now();
        // Parse, render, and decode images off the UI thread.
        let request = cx.background_executor().spawn(async move {
            loader().map(|(messages, images)| {
                let times = history_tool_times(&messages);
                let codemode: Vec<_> = codemode_card::history_summaries(&messages).collect();
                (crate::panel::history_items::build(messages, images), times, codemode)
            })
        });
        self.provisional_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |panel, cx| {
                panel.provisional_task = None;
                if std::mem::take(&mut panel.awaiting_persisted) {
                    cx.notify();
                }
                let Ok(((items, user_times), tool_times, codemode)) = result else {
                    return;
                };
                if items.is_empty() || !panel.can_show_provisional() {
                    return;
                }
                eprintln!(
                    "jcode desktop: provisional transcript items={} in {:?}",
                    items.len(),
                    started.elapsed()
                );
                panel.provisional_items = items.len();
                panel.user_times = user_times;
                panel.codemode_summaries.extend(codemode);
                for (id, (start, finish, duration)) in tool_times {
                    panel.tool_wall_times.insert(id.clone(), (start, finish));
                    if let Some(duration) = duration {
                        panel.tool_durations.insert(id, duration);
                    }
                }
                *panel.items = items;
                panel.transcript_measurements.invalidate();
                if panel.startup_layout.as_ref().is_some_and(|l| !l.committed) {
                    panel.startup_layout = None;
                }
                if panel.pending_history_scroll.is_none() && panel.stick_to_bottom {
                    panel.transcript_list.scroll_to_end();
                }
                cx.notify();
            });
        }));
    }

    /// Only an untouched, not-yet-hydrated transcript may borrow the disk copy.
    pub(super) fn can_show_provisional(&self) -> bool {
        !self.history_loaded
            && !self.restored_transcript
            && !self.transcript_only
            && !self.is_side_document()
            && self.provisional_items == 0
            && self.items.is_empty()
            && self.pending_users.is_empty()
            && self.streaming_text.is_empty()
            && self.streaming_reasoning.is_empty()
    }

    /// Retire the provisional prefix before the authoritative history lands.
    /// Returns true when the reply matched it exactly and nothing else is
    /// needed, which keeps measurements and scroll position untouched.
    pub(super) fn adopt_authoritative_over_provisional(
        &mut self,
        items: &[Item],
        user_times: &HashMap<usize, i64>,
    ) -> bool {
        self.provisional_task = None;
        self.awaiting_persisted = false;
        let count = std::mem::take(&mut self.provisional_items).min(self.items.len());
        if count == 0 {
            return false;
        }
        if self.items[..count] == *items {
            self.user_times
                .extend(user_times.iter().map(|(index, at)| (*index, *at)));
            return true;
        }
        self.items.drain(..count);
        self.shift_user_times(0, count);
        self.pending_users = self
            .pending_users
            .drain(..)
            .filter_map(|index| index.checked_sub(count))
            .collect();
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .filter_map(|(index, at)| Some((index.checked_sub(count)?, at)))
            .collect();
        self.clear_pinned_prompt();
        self.swarm_scrolls.borrow_mut().clear();
        false
    }

    pub fn load_history(
        &mut self,
        messages: Vec<jcode_sdk::HistoryMessage>,
        images: Vec<jcode_sdk::RenderedImage>,
        cx: &mut Context<Self>,
    ) {
        if self.is_side_document() {
            return;
        }
        self.note_session_image_ordinal(images.iter().filter_map(|image| image.session_image.as_ref()).map(|image| image.ordinal).max(), cx);
        self.restore_tool_times(&messages);
        self.codemode_summaries
            .extend(codemode_card::history_summaries(&messages));
        if self.history_paging.is_none() {
            self.history_paging = history_paging::HistoryPaging::from_history(&messages);
            if self.history_loaded
                && let Some(paging) = &mut self.history_paging
            {
                paging.set_anchor(&self.items);
            }
        }
        self.transcript_measurements.invalidate();
        if self.restored_transcript {
            self.hydrate_restored_prefix(&messages);
            // An empty/behind reply still completes the connection handshake.
            // Keep the cache reconciliation marker independent of readiness.
            self.history_loaded = true;
            self.input
                .update(cx, |input, cx| input.set_pending_session(false, cx));
        }
        if self.history_loaded {
            self.schedule_ask_recovery(cx);
            // History is persisted message state, not a replay of the live turn.
            // Wait for authoritative idle before reconciling it with a saved
            // suffix. Buffered deltas must not be appended to a history prefix.
            self.defer_reconnect_history(&messages);
            self.reconcile_history_images(&messages, images);
            self.send_queued_prompts(cx);
            cx.notify();
            return;
        }
        self.history_loaded = true;
        if !self.is_pending_session() {
            self.input
                .update(cx, |input, cx| input.set_pending_session(false, cx));
        }
        // An established session can paint an empty placeholder before its
        // history arrives. That must not adopt a new conversation's layout.
        if !messages.is_empty()
            && self.pending_users.is_empty()
            && self
                .startup_layout
                .as_ref()
                .is_some_and(|layout| !layout.committed)
        {
            self.startup_layout = None;
        }
        let (mut items, history_times) = crate::panel::history_items::build(messages, images);
        if let Some(paging) = &mut self.history_paging {
            paging.set_anchor(&items);
        }
        if self.adopt_authoritative_over_provisional(&items, &history_times) {
            self.send_queued_prompts(cx);
            cx.notify();
            return;
        }
        // History goes first; anything echoed locally before it arrived is
        // appended, minus the duplicate the server already knows about.
        let mut existing = std::mem::take(&mut self.items);
        existing.retain(|item| match item {
            Item::User(text) => !matches!(items.last(), Some(Item::User(last)) if last == text),
            _ => true,
        });
        let history_len = items.len();
        self.shift_user_times(history_len, 0);
        self.user_times.extend(history_times);
        self.pending_users = self
            .pending_users
            .drain(..)
            .map(|index| index + history_len)
            .collect();
        self.accepted_users = std::mem::take(&mut self.accepted_users)
            .into_iter()
            .map(|(index, at)| (index + history_len, at))
            .collect();
        items.append(&mut existing);
        self.image_pane_selected = None;
        self.clear_pinned_prompt();
        *self.items = items;
        self.schedule_ask_recovery(cx);
        if self.pending_history_scroll.is_none() && self.stick_to_bottom {
            self.transcript_list.scroll_to_end();
        }
        self.send_queued_prompts(cx);
        cx.notify();
    }

    pub(super) fn recover_response(&mut self, response: &str) {
        self.transcript_measurements.invalidate();
        if self.streaming_text == response {
            return;
        }
        if !self.streaming_text.is_empty() && response.starts_with(&self.streaming_text) {
            self.streaming_text = response.to_string();
            return;
        }

        let latest = self
            .items
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, item)| !matches!(item, Item::User(_)))
            .find(|(_, item)| item.assistant_text().is_some_and(|text| !text.trim().is_empty()));
        if let Some((index, item)) = latest {
            if item.assistant_text() == Some(response) {
                return;
            }
            if let Item::Assistant(existing) | Item::AssistantOutput {
                text: existing, stream_id: None, ..
            } = &mut self.items[index]
                && response.starts_with(existing.as_str())
            {
                *existing = response.to_string();
                return;
            }
        }

        self.flush_reasoning();
        self.flush_streaming();
        self.items.push(Item::Assistant(response.to_string()));
    }
}

impl Panel {
    pub(super) fn glide_transcript_wheel(&mut self, pixels: f32, cx: &mut Context<Self>) {
        self.glide_transcript_input(pixels, false, cx);
    }

    pub(super) fn glide_transcript_input(&mut self, pixels: f32, precise: bool, cx: &mut Context<Self>) {
        if !pixels.is_finite() || pixels == 0.0 {
            return;
        }
        if pixels < 0.0
            && self.transcript_list.logical_scroll_top().item_ix == 0
            && self.transcript_list.logical_scroll_top().offset_in_item <= px(0.)
        {
            self.request_older_history();
        }
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_input(pixels, precise);
        if crate::config::get().appearance.reduce_motion || cx.reduce_motion() {
            self.scroll_transcript_direct(-pixels, cx);
            return;
        }
        self.release_startup_preview();
        // `ListState::scroll_by` is a programmatic movement and therefore does
        // not invoke the list's user-scroll callback. Release follow mode here
        // before the first animated frame, or render would pin every step back
        // to the live tail.
        if pixels < 0.0 && self.stick_to_bottom {
            self.stick_to_bottom = false;
            cx.notify();
        }
        if precise {
            self.transcript_wheel_glide.push_input(pixels, true);
        } else {
            self.transcript_wheel_glide.push(pixels);
        }
        self.transcript_wheel_frame
            .get_or_insert_with(|| cx.background_executor().now());
        cx.notify();
    }

    pub(super) fn cancel_transcript_momentum(&mut self) {
        self.transcript_wheel_glide.cancel();
        self.transcript_wheel_frame = None;
    }

    pub(super) fn scroll_transcript_direct(&mut self, delta_y: f32, cx: &mut Context<Self>) {
        self.cancel_transcript_momentum();
        self.release_startup_preview();
        if delta_y > 0.0 && self.stick_to_bottom {
            self.stick_to_bottom = false;
        }
        let current = self.transcript_list.scroll_px_offset_for_scrollbar();
        self.transcript_list
            .set_offset_from_scrollbar(point(current.x, current.y + px(delta_y)));
        if delta_y > 0.0
            && self.transcript_list.logical_scroll_top().item_ix == 0
            && self.transcript_list.logical_scroll_top().offset_in_item <= px(0.)
        {
            self.request_older_history();
        }
        self.resume_transcript_follow_after_scroll(cx);
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_applied(f32::from(
                current.y - self.transcript_list.scroll_px_offset_for_scrollbar().y,
            ));
        cx.notify();
    }

    pub(super) fn schedule_transcript_wheel_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.transcript_wheel_frame.is_none() || self.transcript_wheel_frame_pending {
            return;
        }
        self.transcript_wheel_frame_pending = true;
        let panel = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = panel.update(cx, |panel, cx| {
                panel.transcript_wheel_frame_pending = false;
                let Some(previous) = panel.transcript_wheel_frame else {
                    return;
                };
                let now = cx.background_executor().now();
                let elapsed = now.saturating_duration_since(previous);
                // Do not replay stale momentum after a suspended/hidden window.
                if elapsed > Duration::from_millis(250) {
                    panel.cancel_transcript_momentum();
                    return;
                }
                panel.transcript_wheel_frame = Some(now);
                if !panel.advance_transcript_wheel_by(elapsed, cx) {
                    panel.cancel_transcript_momentum();
                }
            });
        });
    }

    #[cfg(test)]
    pub(super) fn advance_transcript_wheel(&mut self, cx: &mut Context<Self>) -> bool {
        self.advance_transcript_wheel_by(Duration::from_millis(16), cx)
    }

    pub(super) fn advance_transcript_wheel_by(&mut self, elapsed: Duration, cx: &mut Context<Self>) -> bool {
        if elapsed.is_zero() {
            cx.notify();
            return true;
        }
        let Some(step) = self.transcript_wheel_glide.take_step(elapsed) else {
            return false;
        };
        let current = self.transcript_list.scroll_px_offset_for_scrollbar();
        let max = f32::from(self.transcript_list.max_offset_for_scrollbar().y).max(0.0);
        let target = (f32::from(current.y) - step).clamp(-max, 0.0);
        self.transcript_list
            .set_offset_from_scrollbar(point(current.x, px(target)));
        if step < 0.0 && target >= 0.0 {
            self.request_older_history();
        }
        self.resume_transcript_follow_after_scroll(cx);
        self.flicker_diagnostics
            .borrow_mut()
            .scroll_applied(f32::from(current.y) - target);
        // Discard travel into an edge instead of accumulating invisible debt.
        if (step < 0.0 && target >= 0.0) || (step > 0.0 && target <= -max) {
            self.transcript_wheel_glide.cancel();
        }
        cx.notify();
        self.transcript_wheel_glide.is_gliding()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}


#[cfg(test)]
#[path = "panel_expandable_tests.rs"]
mod expandable_tests;
#[cfg(test)]
#[path = "panel_row_state_tests.rs"]
mod row_state_tests;
#[cfg(test)]
#[path = "panel_fresh_session_tests.rs"]
mod fresh_session_tests;
#[cfg(test)]
#[path = "panel_image_flicker_tests.rs"]
mod image_flicker_tests;
#[cfg(test)]
#[path = "panel_image_transcript_tests.rs"]
mod image_transcript_tests;
#[cfg(test)]
#[path = "panel_startup_lifecycle_tests.rs"]
mod startup_lifecycle_tests;
#[cfg(test)]
#[path = "panel_turn_completion_tests.rs"]
mod turn_completion_tests;

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;


/// Transcript fixture for one screenshot panel. `JCODE_DESKTOP_SCREENSHOT_PANEL_TRANSCRIPTS`
/// (comma-separated, by panel index) overrides `JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT` so
/// a multi-panel capture can show varied sessions instead of one repeated fixture.
#[cfg(test)]
#[path = "panel_responsive_tests.rs"]
mod responsive_tests;

#[cfg(test)]
#[path = "panel_derived_chrome_tests.rs"]
mod derived_chrome_tests;

#[cfg(test)]
#[path = "panel_todoist_tests.rs"]
mod todoist_tests;
