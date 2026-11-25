//! The local jcode daemon: connecting to it, reviving it, and detaching from it.

pub(in crate::harness) mod generation;

use std::cell::Cell;

use std::sync::mpsc::Sender;
use std::time::Duration;

use jcode_sdk::{ConnectOptions, JcodeClient};

use crate::harness::protocol::{Command, Update};
use crate::harness::updates::UpdateSender;
use crate::platform::{jcode_home, screenshot_mode};

pub fn controlled_runtime_socket() -> Option<std::path::PathBuf> {
    if !screenshot_mode()
        || std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() != Ok("1")
        || std::env::var("JCODE_TEST_SESSION").as_deref() != Ok("1")
    {
        return None;
    }
    let socket =
        std::path::PathBuf::from(std::env::var_os("JCODE_DESKTOP_CONTROL_RUNTIME_SOCKET")?);
    let home = jcode_home()?;
    (socket.parent() == Some(home.as_path()) && socket.is_absolute()).then_some(socket)
}

pub(crate) fn connect(client_name: &str) -> jcode_sdk::Result<JcodeClient> {
    if let Some(socket) = controlled_runtime_socket() {
        return JcodeClient::connect(ConnectOptions {
            socket_path: Some(socket),
            client_name: format!("jcode-desktop-{client_name}/{}", crate::client_version()),
            ensure_runtime: false,
            supports_user_response: false,
            supports_applets: true,
            ..Default::default()
        });
    }
    let attempt = || -> jcode_sdk::Result<JcodeClient> {
        let client = JcodeClient::connect(ConnectOptions {
            socket_path: Some(generation::local_bridge_socket()?),
            client_name: format!("jcode-desktop-{client_name}/{}", crate::client_version()),
            ensure_runtime: false,
            // Temporarily disabled until inline tool responses have broader testing.
            supports_user_response: false,
            supports_applets: true,
            ..Default::default()
        })?;
        generation::verify_local_bridge(client.runtime_identity.as_ref())?;
        generation::verify_capabilities(&client)?;
        Ok(client)
    };
    match attempt() {
        Err(error)
            if matches!(
                error.kind,
                jcode_sdk::ErrorKind::ConnectFailed | jcode_sdk::ErrorKind::HandshakeFailed
            ) =>
        {
            // The runtime was started once at bridge startup. If the harness
            // API bridge later dies (a self-dev restart interrupted halfway,
            // a crash), its socket file is left behind with nothing listening
            // and every panel retried the dead socket forever. Respawn the
            // runtime here instead. The bridge holds a single-instance lock,
            // so concurrent respawns from several panels are harmless, and the
            // throttle keeps a genuinely broken install from fork-looping.
            if !revive_local_runtime() {
                return Err(error);
            }
            attempt()
        }
        other => other,
    }
}

/// Minimum spacing between runtime respawn attempts from reconnect paths.
pub(super) const RUNTIME_REVIVE_INTERVAL: Duration = Duration::from_secs(2);

/// Respawn the harness API bridge when the daemon is up but the bridge is not.
/// Returns whether a revive was attempted and succeeded.
///
/// A missing daemon is left alone on purpose: a self-dev reload takes the
/// daemon socket away for a moment, and spawning a competing `jcode serve`
/// from every reconnecting panel would fight the handoff.
pub(super) fn revive_local_runtime() -> bool {
    if !generation::daemon_allows_bridge_revive() {
        return false;
    }
    static LAST_ATTEMPT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let Ok(mut last) = LAST_ATTEMPT.lock() else {
            return false;
        };
        if !revive_due(*last, std::time::Instant::now()) {
            return false;
        }
        *last = Some(std::time::Instant::now());
    }
    let Ok(options) = generation::launch_options() else {
        return false;
    };
    let _starting = RUNTIME_START.lock().unwrap_or_else(|e| e.into_inner());
    // The SDK treats an existing socket file as a running bridge, so a dead
    // bridge's leftover file would make this a no-op forever.
    if let Some(socket) = &options.api_socket {
        crate::platform::clear_stale_socket(socket);
    }
    eprintln!("jcode desktop: reviving harness API bridge");
    match jcode_sdk::ensure_runtime(&options, &|_| {}) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("jcode desktop: could not revive local runtime: {error}");
            false
        }
    }
}

/// Serializes every local runtime start in this process. Startup and a panel's
/// reconnect can otherwise both see no API socket and each spawn a bridge.
/// Unix bridges drop the duplicate through their flock, but Windows bridges
/// have no such lock, so two bridges would stay alive. Whoever waits here sees
/// the socket the first caller brought up and returns without spawning.
pub(super) static RUNTIME_START: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn revive_due(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|last| now.saturating_duration_since(last) >= RUNTIME_REVIVE_INTERVAL)
}

/// Gracefully detach idle attachments, including from older runtimes that use
/// the ownership flag for crash detection. Current runtimes classify abrupt
/// disconnects as crashes only when they interrupt an unfinished turn.
pub(super) struct GracefulDetach<'a> {
    client: &'a JcodeClient,
    session_id: &'a str,
    /// Owned by the worker, which knows whether a turn is in flight.
    processing: &'a Cell<bool>,
}

impl<'a> GracefulDetach<'a> {
    pub(super) fn new(
        client: &'a JcodeClient,
        session_id: &'a str,
        processing: &'a Cell<bool>,
    ) -> Self {
        Self {
            client,
            session_id,
            processing,
        }
    }
}

impl Drop for GracefulDetach<'_> {
    fn drop(&mut self) {
        // A desktop window disappearing during a live turn is an interruption,
        // not an idle detach. The runtime persists interrupted work as `crashed`;
        // startup restoration can then surface and resume it.
        if should_detach_on_drop(self.processing.get()) {
            let _ = self.client.detach_session(self.session_id);
        }
    }
}

pub(super) fn should_detach_on_drop(processing: bool) -> bool {
    !processing
}

pub(super) fn start_local_runtime(
    updates: UpdateSender,
    commands: &Sender<Command>,
) -> Option<JcodeClient> {
    // A self-dev reload deliberately takes the runtime socket away for a short
    // time. Keep this bridge (and therefore the GPUI/Wayland process) alive
    // while it comes back instead of turning a transient failure into a dead
    // desktop window.
    loop {
        if updates
            .send(Update::Status("starting jcode runtime...".into()))
            .is_err()
        {
            return None;
        }
        if controlled_runtime_socket().is_some() {
            if connect("control-runtime").is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        let started = generation::launch_options().and_then(|options| {
            let _starting = RUNTIME_START.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(socket) = &options.api_socket {
                crate::platform::clear_stale_socket(socket);
            }
            jcode_sdk::ensure_runtime(&options, &|status| {
                let _ = updates.send(Update::Status(status.to_string()));
            })
        });
        match started {
            Ok(()) => {
                generation::retire_stale_bridges();
                break;
            }
            Err(error) => {
                let _ = updates.send(Update::Disconnected {
                    reason: format!("{error}; retrying"),
                });
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
    // Neither persisted history scanning nor a busy daemon's list request
    // should delay creating the first interactive session.
    let _ = commands.send(Command::RefreshSessionsInitial);
    let _ = updates.send(Update::Connected);
    let client = connect("draft-models").ok()?;
    if let Ok((current, routes)) = client.list_draft_models() {
        let _ = updates.send(Update::DraftModels { current, routes });
    }
    Some(client)
}
