//! One panel's connection thread: its command vocabulary, the handle the
//! coordinator keeps and the pieces the worker is built from.

pub(super) mod disconnect;
pub(super) mod events;
pub(super) mod history;
pub(super) mod oneshot;
pub(super) mod private_route;
pub(super) mod recovery;
pub(super) mod search;
pub(super) mod side_questions;
pub(super) mod spawn_profile;
pub(super) mod tab_wait;
pub(super) mod worker;

use std::collections::{HashMap, VecDeque};

use std::sync::mpsc::{Receiver, Sender, channel};

use jcode_sdk::JcodeClient;

use crate::harness::protocol::{Command, SessionOperation};
use crate::harness::remote::transport;
use crate::harness::session::worker::session_worker_with_transports;
use crate::harness::updates::UpdateSender;

pub(super) enum SessionCommand {
    RefreshRuntime,
    GetCompactedHistory(usize),
    GetHistoryPage(Option<String>),
    GetHistoryImage(String),
    ListSessionImages,
    TodoState,
    AckTodoFollowUp(jcode_sdk::todo::TodoEffects),
    AuthChanged(String),
    Send {
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
    },
    SendSkill {
        content: String,
        images: Vec<(String, String)>,
        attachments: Vec<jcode_sdk::api::Attachment>,
        skill: String,
    },
    QueuePrompt(jcode_sdk::api::QueuedPrompt),
    RemoveQueuedPrompt(jcode_sdk::api::QueuedPrompt, bool),
    SetQueuedPromptEditing(String, bool),
    EditQueuedPrompt(
        String,
        String,
        Vec<(String, String)>,
        Vec<jcode_sdk::api::Attachment>,
    ),
    ReorderQueuedPrompts(Vec<String>),
    SetQueuedPromptMode(String, jcode_sdk::api::QueuedPromptMode),
    InterruptQueuedPrompt(String),
    InterruptQueuedPrompts(Vec<String>),
    CancelQueuedPrompt(String),
    /// Send a side question with its steering hidden from the transcript.
    SendSideQuestion {
        question: String,
        legacy_archive: bool,
    },
    ContinueSideQuestion {
        question: String,
    },
    ReplaySideQuestionHistory,
    Cancel,
    ForceStopTurn,
    BackgroundTool,
    WatchBackgroundTaskOutput(Option<String>),
    CancelBackgroundTask(String),
    AskResponse {
        request_id: String,
        attachment_id: u64,
        attempt_id: u64,
        response: jcode_sdk::AskResponse,
    },
    RoutingGateAnswer {
        gate_id: String,
        choice: jcode_sdk::RoutingGateChoice,
    },
    RoutingIntent(jcode_sdk::RoutingIntentAction),
    Fork,
    ForkWithPrompt {
        prompt: String,
        internal: Sender<Command>,
    },
    SideQuestion {
        question: String,
        internal: Sender<Command>,
    },
    RestoreSession,
    Transfer,
    SetModel(String),
    CancelPendingModel,
    RuntimeCommand(jcode_sdk::api::RuntimeCommand, Vec<String>),
    Operation(SessionOperation),
    Stop,
}

pub(super) fn ensure_session_worker(
    workers: &mut HashMap<String, Sender<SessionCommand>>,
    session_id: String,
    updates: &UpdateSender,
    transports: &transport::RemoteTransports,
) -> Sender<SessionCommand> {
    if let Some(worker) = workers.get(&session_id) {
        return worker.clone();
    }
    let tx = spawn_session_worker(session_id.clone(), updates, transports);
    workers.insert(session_id, tx.clone());
    tx
}

pub(super) fn spawn_session_worker(
    session_id: String,
    updates: &UpdateSender,
    transports: &transport::RemoteTransports,
) -> Sender<SessionCommand> {
    let (tx, rx) = channel::<SessionCommand>();
    let updates = updates.clone();
    let transports = transports.clone();
    std::thread::Builder::new()
        .name(format!("jcode-session-{session_id}"))
        .spawn(move || session_worker_with_transports(session_id, rx, updates, None, transports))
        .expect("spawn session worker");
    tx
}

pub(super) fn spawn_attached_session_worker(
    session_id: String,
    client: JcodeClient,
    updates: &UpdateSender,
    transports: &transport::RemoteTransports,
) -> Sender<SessionCommand> {
    let (tx, rx) = channel::<SessionCommand>();
    let updates = updates.clone();
    let transports = transports.clone();
    std::thread::Builder::new()
        .name(format!("jcode-session-{session_id}"))
        .spawn(move || {
            session_worker_with_transports(session_id, rx, updates, Some(client), transports)
        })
        .expect("spawn attached session worker");
    tx
}

pub(super) fn send_to_session_worker<F>(
    workers: &mut HashMap<String, Sender<SessionCommand>>,
    session_id: String,
    command: SessionCommand,
    mut spawn: F,
) where
    F: FnMut(String) -> Sender<SessionCommand>,
{
    let worker = workers
        .entry(session_id.clone())
        .or_insert_with(|| spawn(session_id.clone()))
        .clone();
    if let Err(error) = worker.send(command) {
        // A worker can disconnect between the map lookup and send. Replace it
        // and retain the user's message instead of silently dropping it.
        let worker = spawn(session_id.clone());
        workers.insert(session_id, worker.clone());
        let _ = worker.send(error.0);
    }
}

#[allow(clippy::large_enum_variant)]
pub(super) enum WorkerCommand {
    Command(SessionCommand),
    Idle,
    Disconnected,
}

pub(super) fn next_worker_command(
    commands: &Receiver<SessionCommand>,
    pending: &mut VecDeque<SessionCommand>,
) -> WorkerCommand {
    if let Some(command) = pending.pop_front() {
        return WorkerCommand::Command(command);
    }
    match commands.try_recv() {
        Ok(command) => WorkerCommand::Command(command),
        Err(std::sync::mpsc::TryRecvError::Empty) => WorkerCommand::Idle,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => WorkerCommand::Disconnected,
    }
}

// Existing socket-pair tests need no live transport pool.
