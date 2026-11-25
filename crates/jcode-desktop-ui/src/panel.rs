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
#[path = "panel_events.rs"]
mod events;
#[path = "panel_rows.rs"]
mod rows;
#[path = "panel_construct.rs"]
mod construct;
#[path = "panel_slash.rs"]
mod slash;
#[path = "panel_draft.rs"]
mod draft;
#[path = "panel_history_load.rs"]
mod history_load;
#[path = "panel_todoist.rs"]
mod todoist;
#[path = "panel_scroll.rs"]
mod scroll;
#[path = "panel_render.rs"]
mod render;
#[path = "panel_items.rs"]
mod items;
pub use items::{Item, PanelSnapshot, TodoCardPayload, Transcript, TranscriptImage};
use items::*;
#[path = "panel_live.rs"]
pub(crate) mod live;
use live::Live;
#[path = "panel_clean_chat.rs"]
mod clean_chat;
#[path = "panel_assistant_output.rs"]
mod assistant_output;
pub use assistant_output::AssistantOutputPhase;

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
