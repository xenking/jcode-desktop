//! Harness bridge: SDK connections on background threads, events fanned into
//! GPUI through a channel the workspace polls.
//!
//! The harness API attaches one session per connection (the bridge translates
//! to the daemon's subscribe protocol), so this bridge gives every panel its
//! own connection thread: attach, fetch history, stream events, and serve
//! that panel's commands. Session creation also uses a fresh connection each
//! time, because a connection re-serves its already-attached session.
//!
//! Layers, lowest first. Each knows nothing of the ones above it:
//! `protocol` (the `Update`/`Command` vocabulary), `updates` (ordered
//! delivery), `remote` (session addresses, SSH and cloud connection pools,
//! remote session listing), `local_runtime` (the local daemon), `catalog` (the
//! persisted session store), `session` (one panel's connection thread), `hub`
//! (the coordinator), then this module's `Bridge`.

mod catalog;
mod hub;
mod local_runtime;
mod protocol;
mod remote;
mod screenshot;
mod session;
mod updates;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};

use updates::{UpdateReceiver, runtime_info_session, update_channel};

pub use crate::platform::{jcode_home, screenshot_mode};
pub use catalog::{
    LiveSession, live_sessions, live_sessions_from, merge_persisted_sessions, todo_progress_in,
    unfinished_sessions, unfinished_sessions_in,
};
pub(crate) use local_runtime::connect;
pub use local_runtime::controlled_runtime_socket;
pub use protocol::*;
pub use remote::{is_remote_session, remote_host, session_address};
pub use screenshot::{
    GALLERY_FIXTURE_IMAGES, composer_picker_fixture_runtime,
    gallery_fixture_image_id, routing_fixture_decision, routing_fixture_runtime,
};
pub use session::events::collection_reminder;

#[derive(Clone)]
pub struct Bridge {
    _lifetime: std::sync::Arc<BridgeLifetime>,
    commands: Sender<Command>,
    updates: UpdateReceiver,
}

struct BridgeLifetime(Sender<Command>);

impl Drop for BridgeLifetime {
    fn drop(&mut self) {
        // The coordinator retains an internal command sender for creation
        // handoffs, so channel disconnection alone cannot end its receive loop.
        let _ = self.0.send(Command::Shutdown);
    }
}

impl Bridge {
    fn new(updates: UpdateReceiver, commands: Sender<Command>) -> Self {
        Self {
            _lifetime: std::sync::Arc::new(BridgeLifetime(commands.clone())),
            commands,
            updates,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub async fn read_todo_snapshot(
        &self,
        session_id: String,
    ) -> Option<jcode_sdk::todo::TodoSnapshot> {
        let (reply, result) = async_channel::bounded(1);
        self.send(Command::ReadTodoSnapshot { session_id, reply });
        result.recv().await.ok().flatten()
    }

    pub async fn read_context_breakdown(
        &self,
        target: ContextBreakdownTarget,
    ) -> ContextInspection {
        let (reply, result) = async_channel::bounded(1);
        self.send(Command::ReadContextBreakdown { target, reply });
        result.recv().await.unwrap_or_default()
    }

    pub async fn shake_tools(
        &self,
        session_id: String,
        action: jcode_sdk::api::ShakeToolsAction,
    ) -> Result<jcode_sdk::api::ShakeToolsResult, String> {
        let (reply, result) = async_channel::bounded(1);
        self.send(Command::ShakeTools {
            session_id,
            action,
            reply,
        });
        result
            .recv()
            .await
            .map_err(|_| "The bridge disconnected. Shake outcome unknown.".to_owned())?
    }

    /// Drain every pending update without blocking (test helpers only).
    #[cfg(any(test, feature = "test-support"))]
    pub fn drain(&self) -> Vec<Update> {
        self.drain_up_to(usize::MAX)
    }

    /// Leave excess updates queued so a busy producer cannot monopolize the UI.
    pub fn drain_up_to(&self, limit: usize) -> Vec<Update> {
        self.extend_batch(Vec::new(), limit)
    }

    /// Append up to `limit` more updates to `batch`, collapsing runtime info.
    ///
    /// Each snapshot replaces the previous one for its session. The producer
    /// already coalesces queued snapshots, and this also folds an update the
    /// caller received earlier into the newer snapshots drained here.
    /// Superseded copies do not count toward `limit`.
    pub fn extend_batch(&self, batch: Vec<Update>, limit: usize) -> Vec<Update> {
        const MAX_SCAN: usize = 8192;
        let mut slots: Vec<Option<Update>> = Vec::with_capacity(batch.len());
        let mut runtime_slot: HashMap<String, usize> = HashMap::new();
        let mut live = 0usize;
        let mut push = |slots: &mut Vec<Option<Update>>, live: &mut usize, update: Update| {
            if let Some(session) = runtime_info_session(&update)
                && let Some(previous) = runtime_slot.insert(session.to_owned(), slots.len())
            {
                slots[previous] = None;
                *live -= 1;
            }
            slots.push(Some(update));
            *live += 1;
        };
        for update in batch {
            push(&mut slots, &mut live, update);
        }
        let target = live.saturating_add(limit);
        let mut scanned = 0;
        while live < target && scanned < MAX_SCAN {
            let Ok(update) = self.updates.try_recv() else {
                break;
            };
            scanned += 1;
            push(&mut slots, &mut live, update);
        }
        slots.into_iter().flatten().collect()
    }

    pub async fn recv(&self) -> Option<Update> {
        self.updates.recv().await.ok()
    }
}

/// Spawn the bridge. Returns immediately; connection happens on the thread.
pub fn spawn() -> Bridge {
    let (update_tx, update_rx) = update_channel();
    let (command_tx, command_rx) = channel::<Command>();
    if screenshot_mode() && controlled_runtime_socket().is_none() {
        std::thread::Builder::new()
            .name("jcode-bridge-screenshot".into())
            .spawn(move || screenshot::run(update_tx, command_rx))
            .expect("spawn screenshot bridge thread");
    } else {
        std::thread::Builder::new()
            .name("jcode-bridge".into())
            .spawn({
                let command_tx = command_tx.clone();
                move || hub::run(update_tx, command_rx, command_tx)
            })
            .expect("spawn bridge thread");
    }
    Bridge::new(update_rx, command_tx)
}

/// A bridge with no runtime behind it. Commands are accepted and dropped, so a
/// test can drive the UI without a jcode daemon.
pub fn spawn_inert() -> Bridge {
    let (_update_tx, update_rx) = update_channel();
    let (command_tx, _command_rx) = channel::<Command>();
    // Leak the receiving ends: nothing should observe or service them, and the
    // senders must stay usable for the lifetime of the test.
    std::mem::forget(_command_rx);
    std::mem::forget(_update_tx);
    Bridge::new(update_rx, command_tx)
}

/// A runtime-free bridge whose commands can be asserted by UI acceptance tests.
#[cfg(any(test, feature = "test-support"))]
pub fn spawn_recording() -> (Bridge, std::sync::mpsc::Receiver<Command>) {
    let (_update_tx, update_rx) = update_channel();
    let (command_tx, command_rx) = channel::<Command>();
    std::mem::forget(_update_tx);
    (Bridge::new(update_rx, command_tx), command_rx)
}
