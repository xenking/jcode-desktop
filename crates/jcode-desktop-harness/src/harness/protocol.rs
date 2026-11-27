//! The vocabulary between the UI and the harness threads: what flows up as
//! `Update`s and what the UI asks for as `Command`s.

use std::collections::HashMap;

use jcode_sdk::{ApiEvent, JcodeClient, SessionInfo};

pub use jcode_sdk::HistoryPage as BoundedHistoryPage;

/// Internal key for a gallery metadata reply carried over the image update path.
pub const SESSION_IMAGE_LIST_KEY: &str = "__session_image_list__";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideQuestionTopic {
    pub session_id: String,
    pub question: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideQuestionTurn {
    pub question: String,
    pub answer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SideQuestionThread {
    pub session: SessionInfo,
    pub turns: Vec<SideQuestionTurn>,
}

/// Updates flowing from the harness threads into the UI.
#[derive(Debug)]
pub enum Update {
    DraftModels {
        current: Option<String>,
        routes: Vec<jcode_sdk::ModelRouteInfo>,
    },
    /// Connection lifecycle status line, shown until connected.
    Status(String),
    /// Remote status persists independently of local runtime startup.
    RemoteStatus {
        host: String,
        message: String,
        request_id: Option<String>,
        failed: bool,
        /// Set when a paired machine failed to connect. The owner-facing
        /// sentence comes from it, not from `message`.
        failure: Option<crate::connect_failure::ConnectFailure>,
    },
    RemoteSessions {
        request_id: u64,
        result: Result<Vec<SessionInfo>, String>,
    },
    /// The runtime is up and reachable.
    Connected,
    /// The initial session list, fetched in the background.
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    CatalogTodos(CatalogTodos),
    /// Facts read from each swarm worker's own record: model, effort, spend
    /// and live phase. Sent before the catalog that lists those workers, so a
    /// sidebar row never renders without its model.
    WorkerFacts {
        workers: HashMap<String, jcode_sdk::WorkerMeta>,
    },
    /// A session was created (in reply to `Command::CreateSession`).
    SessionCreated {
        session: SessionInfo,
        /// Correlates the startup draft without consuming another creation.
        request_id: Option<String>,
    },
    /// A session was forked from an existing panel. `prompt` is the question
    /// the fork was opened with: the harness has already delivered it to the
    /// child, so the panel echoes it instead of the runtime replaying it.
    SessionForked {
        session: SessionInfo,
        prompt: Option<String>,
    },
    /// A `/btw` side question started in a hidden fork of `parent_session_id`.
    /// The answer streams as ordinary events for the child session; it must
    /// never open a tab of its own.
    SideQuestionStarted {
        parent_session_id: String,
        session: SessionInfo,
        question: String,
    },
    SideQuestionHistory {
        parent_session_id: String,
        request_id: u64,
        result: Result<Vec<SideQuestionTopic>, String>,
    },
    SideQuestionOpened {
        parent_session_id: String,
        request_id: u64,
        result: Result<SideQuestionThread, String>,
    },
    /// History fetched for a session after attach.
    History {
        session_id: String,
        messages: Vec<jcode_sdk::HistoryMessage>,
        images: Vec<jcode_sdk::RenderedImage>,
    },
    HistoryPage {
        session_id: String,
        result: Result<jcode_sdk::ApiEvent, String>,
    },
    BoundedHistory {
        session_id: String,
        page: BoundedHistoryPage,
    },
    RewoundHistory {
        session_id: String,
        page: BoundedHistoryPage,
        bounded: bool,
    },
    BoundedHistoryPage {
        session_id: String,
        result: Result<BoundedHistoryPage, String>,
    },
    HistoryImage {
        session_id: String,
        key: String,
        result: Result<String, String>,
    },
    /// A streaming event for one session. The worker supplies the session id
    /// because some important events (notably errors) do not include one.
    Event {
        session_id: String,
        event: ApiEvent,
    },
    /// Sending a message failed before the harness accepted it.
    SendFailed {
        session_id: String,
        reason: String,
    },
    /// The runtime accepted a submitted prompt on this session connection.
    MessageSubmitted {
        session_id: String,
    },
    AskAttachment {
        session_id: String,
        attachment_id: u64,
        pending_ask_ids: Option<Vec<String>>,
    },
    /// The server accepted or rejected a specific attempt to answer an ask.
    AskResponseSettled {
        session_id: String,
        request_id: String,
        attempt_id: u64,
        result: Result<(), AskResponseError>,
    },
    /// The daemon accepted or refused the answer to a routing gate. A
    /// refusal usually means another client answered first.
    RoutingGateAnswerSettled {
        session_id: String,
        gate_id: String,
        result: Result<(), String>,
    },
    /// The text reply to a routing intent, or why it failed.
    RoutingIntentReply {
        session_id: String,
        result: Result<String, String>,
    },
    CommandFailed {
        session_id: String,
        reason: String,
    },
    QueueMutationCapability {
        session_id: String,
        supported: bool,
    },
    QueueEditingCapability {
        session_id: String,
        supported: bool,
    },
    QueuedPromptEdited {
        session_id: String,
        prompt_id: String,
        result: Result<jcode_sdk::api::QueuedPromptEditOutcome, String>,
    },
    QueueRemovalSettled {
        session_id: String,
        prompt: jcode_sdk::api::QueuedPrompt,
        edit: bool,
        result: Result<jcode_sdk::api::QueuedPromptRemovalOutcome, String>,
    },
    QueuePromptFailed {
        session_id: String,
        client_id: String,
        reason: String,
    },
    /// A reasoning effort request finished. `error` is the provider's refusal.
    EffortSettled {
        session_id: String,
        effort: String,
        error: Option<String>,
    },
    SpeedSettled {
        session_id: String,
        serial: u64,
        result: Result<jcode_sdk::api::SpeedMode, String>,
    },
    /// Reply to `Command::TodoState`. `None` when the runtime cannot serve
    /// todo state (an older bridge) or the read failed.
    TodoState {
        session_id: String,
        state: Option<jcode_sdk::todo::TodoSnapshot>,
    },
    TabWaitingState {
        session_id: String,
        waiting: bool,
    },
    /// Identity reported by the connected daemon, or None when unavailable.
    OpenrigHostIdentity {
        session_id: String,
        self_host_id: Option<String>,
    },
    /// A per-session connection died.
    SessionLost {
        session_id: String,
        reason: String,
    },
    /// The machine answered and does not have this session, in its list or on
    /// disk. Retrying cannot help: the panel and its list entry go away.
    SessionGone {
        session_id: String,
    },
    /// The runtime could not list its models (the request failed, not "no
    /// accounts"). The picker shows the error instead of an empty list.
    ModelCatalogFailed {
        session_id: String,
        reason: String,
    },
    /// The daemon is still publishing the live session's runtime.
    SessionAttaching {
        session_id: String,
    },
    /// A per-session connection was established again.
    SessionConnected {
        session_id: String,
    },
    /// A session was bookmarked or unbookmarked. A label also names it.
    SessionSaved {
        session_id: String,
        saved: bool,
        label: Option<String>,
    },
    /// The daemon rejected a bookmark change for a live session.
    SessionSaveFailed {
        session_id: String,
        saved: bool,
        error: String,
    },
    /// The control connection died; the bridge will retry.
    Disconnected {
        reason: String,
    },
    /// Reply to `Command::SearchSessions`. `generation` identifies the query so
    /// the UI can ignore answers it has already typed past.
    SessionSearch {
        query: String,
        generation: u64,
        result: Result<jcode_sdk::SessionSearchResults, String>,
        elapsed_ms: f64,
    },
}

/// Commands flowing from the UI into the bridge.
pub enum Command {
    /// Final UI handle dropped. Stop workers, including their SSH transports.
    Shutdown,
    RefreshSessions,
    SetCatalogForeground(bool),
    RefreshSessionsInitial,
    /// Search every stored session's titles and transcripts. Answers with
    /// `Update::SessionSearch` carrying the same `generation`.
    SearchSessions {
        query: String,
        generation: u64,
    },
    RefreshRuntime {
        session_id: String,
    },
    /// Credentials for `provider` changed. Tell the daemon on this session's
    /// own connection: the post-login model switch is session-local, so a
    /// notification from any other connection never moves this session.
    AuthChanged {
        session_id: String,
        provider: String,
    },
    CreateSession {
        working_dir: Option<String>,
        request_id: Option<String>,
    },
    /// Create a native session on an SSH host. Paths are sent in SDK JSON,
    /// never interpolated into an SSH command or interpreted locally.
    CreateRemoteSession {
        host: String,
        working_dir: Option<String>,
        request_id: Option<String>,
    },
    ListRemoteSessions {
        host: String,
        request_id: u64,
    },
    /// Open a dedicated connection for this session (attach + stream).
    Watch {
        session_id: String,
    },
    GetCompactedHistory {
        session_id: String,
        visible_messages: usize,
    },
    GetHistoryPage {
        session_id: String,
        before: Option<String>,
    },
    GetHistoryImage {
        session_id: String,
        key: String,
    },
    ListSessionImages {
        session_id: String,
    },
    /// Drop a session's dedicated connection.
    Unwatch {
        session_id: String,
    },
    Send {
        session_id: String,
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
    },
    SendSkill {
        session_id: String,
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
        skill: String,
    },
    QueuePrompt {
        session_id: String,
        prompt: jcode_sdk::api::QueuedPrompt,
    },
    RemoveQueuedPrompt {
        session_id: String,
        prompt: jcode_sdk::api::QueuedPrompt,
        edit: bool,
    },
    /// Hold a queued prompt open for editing, or release the hold.
    SetQueuedPromptEditing {
        session_id: String,
        prompt_id: String,
        editing: bool,
    },
    /// Replace a still-queued prompt in place.
    EditQueuedPrompt {
        session_id: String,
        prompt_id: String,
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
    },
    ReorderQueuedPrompts {
        session_id: String,
        order: Vec<String>,
    },
    SetQueuedPromptMode {
        session_id: String,
        client_id: String,
        mode: jcode_sdk::api::QueuedPromptMode,
    },
    InterruptQueuedPrompt {
        session_id: String,
        client_id: String,
    },
    InterruptQueuedPrompts {
        session_id: String,
        ids: Vec<String>,
    },
    CancelQueuedPrompt {
        session_id: String,
        client_id: String,
    },
    Cancel {
        session_id: String,
    },
    ForceStopTurn {
        session_id: String,
    },
    /// Move the running tool call to the background (the TUI's Alt+B).
    BackgroundTool {
        session_id: String,
    },
    WatchBackgroundTaskOutput {
        session_id: String,
        task_id: Option<String>,
    },
    CancelBackgroundTask {
        session_id: String,
        task_id: String,
    },
    /// Answer an `ask` tool request. `AskResponse::default()` declines.
    AskResponse {
        session_id: String,
        request_id: String,
        attachment_id: u64,
        attempt_id: u64,
        response: jcode_sdk::AskResponse,
    },
    /// Answer a routing spending-limit gate.
    RoutingGateAnswer {
        session_id: String,
        gate_id: String,
        choice: jcode_sdk::RoutingGateChoice,
    },
    /// Set or read a routing intent for the session.
    RoutingIntent {
        session_id: String,
        action: jcode_sdk::RoutingIntentAction,
    },
    Fork {
        session_id: String,
    },
    ForkWithPrompt {
        session_id: String,
        prompt: String,
    },
    /// `/btw`: answer `question` in a hidden fork so the source session's
    /// transcript and prompt cache stay untouched.
    SideQuestion {
        session_id: String,
        question: String,
    },
    ListSideQuestions {
        parent_session_id: String,
        request_id: u64,
    },
    OpenSideQuestion {
        parent_session_id: String,
        session_id: String,
        request_id: u64,
    },
    ContinueSideQuestion {
        session_id: String,
        question: String,
    },
    /// Drop the connection to a dismissed side-question fork. The archived
    /// session stays on disk.
    DismissSideQuestion {
        session_id: String,
    },
    PromoteSideQuestion {
        session_id: String,
    },
    /// Compatibility path for a side question created as a legacy archive.
    RestoreSideQuestion {
        session_id: String,
    },
    Transfer {
        session_id: String,
    },
    /// Read the session's todo state for the auto-poke/quality-gate policy.
    /// Answered with `Update::TodoState`.
    TodoState {
        session_id: String,
    },
    /// Read a plan for the sidebar without owning or attaching to its session.
    ReadTodoSnapshot {
        session_id: String,
        reply: async_channel::Sender<Option<jcode_sdk::todo::TodoSnapshot>>,
    },
    ReadContextBreakdown {
        target: ContextBreakdownTarget,
        reply: async_channel::Sender<ContextInspection>,
    },
    ShakeTools {
        session_id: String,
        action: jcode_sdk::api::ShakeToolsAction,
        reply: async_channel::Sender<Result<jcode_sdk::api::ShakeToolsResult, String>>,
    },
    /// Apply the persistent effects of a follow-up decision.
    AckTodoFollowUp {
        session_id: String,
        effects: jcode_sdk::todo::TodoEffects,
    },
    SetModel {
        session_id: String,
        model: String,
    },
    /// Withdraw a model switch the daemon queued behind the running turn.
    CancelPendingModel {
        session_id: String,
    },
    RuntimeCommand {
        session_id: String,
        command: jcode_sdk::api::RuntimeCommand,
        args: Vec<String>,
    },
    SessionOperation {
        session_id: String,
        operation: SessionOperation,
    },
    /// Pin (bookmark) or unpin a session from the sidebar, open or not. Open
    /// sessions reuse their worker; history rows use a short-lived connection
    /// so pinning never leaves a closed session attached.
    SetSessionSaved {
        session_id: String,
        saved: bool,
    },
    /// Retain a fork child connection and queue its first prompt after adoption.
    ForkedInternal {
        session: SessionInfo,
        client: JcodeClient,
        prompt: String,
    },
    /// Retain a side-question child connection and queue its question.
    SideQuestionInternal {
        parent_session_id: String,
        session: SessionInfo,
        client: JcodeClient,
        question: String,
    },
    /// Internal handoff from the asynchronous creator back to the bridge loop.
    CreatedInternal {
        session: SessionInfo,
        client: JcodeClient,
        request_id: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct ContextInspection {
    pub breakdown: Option<jcode_sdk::api::ContextBreakdown>,
    pub shake_supported: bool,
}

#[derive(Debug)]
pub enum ContextBreakdownTarget {
    Session(String),
    SessionMeter(String),
    Draft {
        model: Option<String>,
        cwd: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub enum SessionOperation {
    Clear,
    Compact,
    SetEffort(String),
    SetSpeed {
        serial: u64,
        mode: jcode_sdk::api::SpeedMode,
    },
    SetModelWithSpeed {
        model: String,
        effort: Option<String>,
        serial: u64,
        mode: jcode_sdk::api::SpeedMode,
    },
    Rename(Option<String>),
    /// Bookmark (`true`) or unbookmark. A label also becomes the title.
    SetSaved(bool, Option<String>),
    Rewind(usize),
    RewindTranscript(jcode_sdk::api::RewindAnchor),
    RewindUndo,
    /// The user acted on an agent applet instance.
    AppletAction {
        instance: String,
        action: jcode_applet_types::Action,
        state: serde_json::Value,
        source_key: Option<String>,
    },
    /// The user closed an agent applet instance.
    CloseApplet(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnfinishedTodo {
    pub content: String,
    pub status: String,
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnfinishedSession {
    pub session_id: String,
    pub title: String,
    pub working_dir: Option<String>,
    pub todos: Vec<UnfinishedTodo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogTodos {
    /// Compact display rows from the same read that produced sidebar progress.
    pub rows: HashMap<String, Vec<(String, String)>>,
    pub progress: HashMap<String, (usize, usize)>,
    pub unfinished: Vec<UnfinishedSession>,
}

#[derive(Clone, Debug)]
pub enum AskResponseError {
    Stale,
    ConnectionChanged,
    Failed(String),
}
