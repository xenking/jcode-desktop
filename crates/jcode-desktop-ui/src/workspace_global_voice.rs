//! Global holds belong to the last-focused chat, not to the foreground app.
//! This is UI-owned so reload/close drops the input descriptors and microphone.
//!
//! Edges come from one of two platform sources, both feeding the same
//! dictation-only capture owner:
//! * Linux: the opt-in evdev listener (any display server, X11 or Wayland).
//! * macOS/Windows: the host's registered global shortcut, delivered as the
//!   `workspace::BeginGlobalVoiceHold` / `EndGlobalVoiceHold` actions without
//!   activating the window.
use super::*;
#[cfg(target_os = "linux")]
use crate::global_voice_input::{Edge, Listener};
use crate::global_voice_overlay::{self as overlay, Snapshot, VoiceOverlay};
use gpui::{Task, WeakEntity, WindowHandle};
use std::sync::{Arc, atomic::AtomicBool};

#[cfg(target_os = "linux")]
#[path = "workspace_global_voice_cli.rs"]
mod cli;
#[path = "workspace_global_voice_fixture.rs"]
mod fixture;

struct Capture {
    panel: WeakEntity<Panel>,
    attempt: Arc<AtomicBool>,
}

/// Host-delivered holds have no kernel-side deadline, so bound them here too.
/// Reaching it releases the hold, like a key-up: the words spoken so far are
/// delivered, never discarded.
const HOST_HOLD_DEADLINE: Duration = Duration::from_secs(600);
const IDLE_POLL: Duration = Duration::from_millis(40);
const HELD_POLL: Duration = Duration::from_millis(8);

#[derive(Default)]
pub(super) struct State {
    #[cfg(target_os = "linux")]
    listener: Option<Listener>,
    /// Set while a host shortcut hold is down. Used for the safety deadline.
    host_held_since: Option<Instant>,
    task: Option<Task<()>>,
    owner: Option<Capture>,
    overlay: Option<WindowHandle<VoiceOverlay>>,
    finished_at: Option<Instant>,
    permission_task: Option<Task<()>>,
    last_permission_check: Option<Instant>,
    press_serial: u64,
    held: bool,
    /// Set in a window Shift+Copilot or an unfocused press just opened, until
    /// it adopts that hold. Its own first press must never spawn again.
    #[allow(dead_code)]
    adopting_spawned_hold: bool,
    pending_target: Option<WeakEntity<Panel>>,
    /// Whether this Jcode window has focus. The chat's own voice pill is the
    /// single source of truth. The OS pill mirrors it only while unfocused.
    window_active: bool,
    /// Hold recorded for a focused Jcode CLI terminal instead of a chat.
    #[cfg(target_os = "linux")]
    cli: Option<cli::CliCapture>,
    /// Resolving whether the focused window is a Jcode CLI.
    #[cfg(target_os = "linux")]
    cli_resolve: Option<Task<()>>,
}

/// Exactly one voice pill is ever visible. The OS-level pill mirrors capture
/// and its result only while the chat is not in front of you. A focused
/// window shows both in its own in-panel pill, which the panel hides while
/// unfocused, so the two never appear together.
fn os_pill_wanted(window_active: bool, has_status: bool, _decided: bool) -> bool {
    has_status && !window_active
}

/// Whether a host global-shortcut press should use the dictation-only global
/// owner. A focused window keeps its native hold (with intent routing),
/// exactly as the in-app shortcut would, so nothing changes for focused use.
fn host_press_uses_global_owner(window_active: bool, overlay_available: bool) -> bool {
    !window_active && overlay_available
}

/// An unfocused global press opens a fresh single-panel window that records
/// and sends the utterance, so it never lands in whatever chat was last used.
/// A focused press keeps its in-panel hold. A window that was itself just
/// spawned adopts the hold instead of spawning again.
#[cfg(target_os = "linux")]
fn press_spawns_window(window_active: bool, adopting_spawned_hold: bool) -> bool {
    !window_active && !adopting_spawned_hold
}

/// Marks a single-panel window opened by Shift+Copilot or an unfocused press.
#[cfg(target_os = "linux")]
const SPAWNED_HOLD_FLAG: &str = "--global-voice-hold";

/// Shift+Copilot: a new single-panel window in `voice.spawn_working_dir`
/// that takes over the current hold. This window only launches it.
#[cfg(target_os = "linux")]
fn spawn_voice_window() -> bool {
    let Ok(executable) = crate::platform::self_executable() else {
        eprintln!("global voice: cannot locate the desktop executable to spawn");
        return false;
    };
    let mut command = std::process::Command::new(executable);
    command.args(spawn_args(std::env::args_os().skip(1)));
    if let Some(dir) = crate::config::get().voice.spawn_working_dir.clone() {
        command.env("JCODE_DESKTOP_WORKING_DIR", dir);
    }
    match command.spawn() {
        Ok(child) => {
            eprintln!("global voice: spawned voice window {}", child.id());
            true
        }
        Err(error) => {
            eprintln!("global voice: could not spawn voice window: {error}");
            false
        }
    }
}

/// Keep this host's hot-reload choice. Never forward launch or voice flags.
#[cfg(target_os = "linux")]
fn spawn_args(own: impl Iterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    // Joins the shared single-panel host. The hold flag and working directory
    // travel with the forwarded per-window launch, so every voice window runs
    // (and hot reloads) in one process instead of accumulating stale hosts.
    let mut args: Vec<std::ffi::OsString> = vec!["--single-panel".into(), SPAWNED_HOLD_FLAG.into()];
    args.extend(own.filter(|arg| arg == "--hot-reload" || arg == "--no-hot-reload"));
    args
}

impl State {
    #[cfg(target_os = "linux")]
    pub(super) fn ready(&self) -> bool {
        self.listener.as_ref().is_some_and(Listener::ready)
    }

    pub(super) fn set_active(&mut self, active: bool) {
        self.window_active = active;
        #[cfg(target_os = "linux")]
        if let Some(listener) = self.listener.as_mut() {
            listener.set_active(active);
        }
    }

    /// Stop accepting global edges after the OS pill proved impossible, so a
    /// Linux evdev hold returns to native focused-key handling.
    fn disable_capture_source(&mut self) {
        #[cfg(target_os = "linux")]
        {
            self.listener = None;
        }
    }

    fn hide_overlay(&mut self, cx: &mut App) {
        if let Some(handle) = self.overlay.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn close_overlay(&mut self, cx: &mut App) {
        self.hide_overlay(cx);
        self.owner = None;
        self.finished_at = None;
    }

    /// Whether a hold is down, from the in-app key or from the host shortcut.
    fn hold_active(&self) -> bool {
        self.host_held_since.is_some() || self.held
    }

    /// The chat a press aimed at, while it is still open.
    fn target_panel(&self) -> Option<Entity<Panel>> {
        self.pending_target.as_ref().and_then(WeakEntity::upgrade)
    }

    /// A press went down on `target`: the hold is held until released, and a
    /// fresh serial makes every earlier permission answer stale.
    fn begin_press(&mut self, target: Option<WeakEntity<Panel>>) {
        self.pending_target = target;
        self.held = true;
        self.press_serial = self.press_serial.wrapping_add(1);
    }

    /// The host shortcut went down: bound the hold by a deadline.
    fn note_host_hold(&mut self) {
        self.host_held_since = Some(Instant::now());
    }

    /// Whether the host hold outlived its deadline. It is forgotten when it did.
    fn take_expired_host_hold(&mut self) -> bool {
        let expired = self
            .host_held_since
            .is_some_and(|since| since.elapsed() >= HOST_HOLD_DEADLINE);
        if expired {
            self.host_held_since = None;
        }
        expired
    }

    /// The key came up. The chat whose capture it ends and that capture's
    /// attempt, when one is running.
    fn release(&mut self) -> Option<(Entity<Panel>, Arc<AtomicBool>)> {
        self.held = false;
        self.host_held_since = None;
        self.pending_target = None;
        self.owner
            .as_ref()
            .and_then(|owner| Some((owner.panel.upgrade()?, owner.attempt.clone())))
    }

    /// Forget the hold and every answer still pending, and say which capture
    /// to cancel.
    fn abort(&mut self) -> Option<(Entity<Panel>, Arc<AtomicBool>)> {
        self.held = false;
        #[cfg(target_os = "linux")]
        {
            self.cli = None;
            self.cli_resolve = None;
        }
        self.host_held_since = None;
        self.pending_target = None;
        self.press_serial = self.press_serial.wrapping_add(1);
        self.permission_task = None;
        self.owner
            .as_ref()
            .and_then(|owner| Some((owner.panel.upgrade()?, owner.attempt.clone())))
    }

    fn owner_panel(&self) -> Option<Entity<Panel>> {
        self.owner.as_ref().and_then(|owner| owner.panel.upgrade())
    }

    fn attempt(&self) -> Option<Arc<AtomicBool>> {
        self.owner.as_ref().map(|owner| owner.attempt.clone())
    }

    /// A capture began on `panel`; its result pill has not started to age.
    fn start_capture(&mut self, panel: WeakEntity<Panel>, attempt: Arc<AtomicBool>) {
        eprintln!("global voice: capture started");
        self.finished_at = None;
        self.owner = Some(Capture { panel, attempt });
    }

    /// Whether the result pill of a finished capture has been up long enough.
    fn result_pill_expired(&mut self) -> bool {
        let since = self.finished_at.get_or_insert_with(Instant::now);
        since.elapsed() >= Duration::from_secs(5)
    }

    /// Whether the session check is due: something records, none is pending,
    /// and the last one is old enough.
    fn permission_check_due(&self, cli_recording: bool) -> bool {
        (self.owner.is_some() || cli_recording)
            && self.permission_task.is_none()
            && self
                .last_permission_check
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
    }

    /// Starts a session check, which `spawn` runs for the current press serial.
    fn await_permission(&mut self, spawn: impl FnOnce(u64) -> Task<()>) {
        self.last_permission_check = Some(Instant::now());
        self.permission_task = Some(spawn(self.press_serial));
    }

    /// The session check for `serial` answered. `false` when a newer press
    /// made the answer stale.
    fn permission_answered(&mut self, serial: u64) -> bool {
        if self.press_serial != serial {
            return false;
        }
        self.permission_task = None;
        true
    }

    /// Runs the shared poll loop once: `start` is called only when none runs.
    fn run_task(&mut self, start: impl FnOnce() -> Task<()>) -> bool {
        if self.task.is_some() {
            return false;
        }
        self.task = Some(start());
        true
    }

    /// The chat the press aimed at, handed over once.
    fn take_target(&mut self) -> Option<Entity<Panel>> {
        self.pending_target.take().and_then(|panel| panel.upgrade())
    }

    fn has_task(&self) -> bool {
        self.task.is_some()
    }

    fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    /// Shows `snapshot` in the OS pill: updates it, or opens it. `true` when
    /// the pill was opened by this call.
    fn show_snapshot(&mut self, snapshot: Snapshot, cx: &mut App) -> anyhow::Result<bool> {
        match self.overlay {
            Some(handle) => {
                handle.update(cx, |overlay, _, cx| overlay.set_snapshot(snapshot, cx))?;
                Ok(false)
            }
            None => {
                self.overlay = Some(overlay::open(snapshot, cx)?);
                Ok(true)
            }
        }
    }

    fn shutdown(&mut self, cx: &mut App) {
        self.task = None;
        self.permission_task = None;
        #[cfg(target_os = "linux")]
        {
            self.listener = None;
            self.cli = None;
            self.cli_resolve = None;
        }
        self.host_held_since = None;
        if let Some(panel) = self.owner.as_ref().and_then(|owner| owner.panel.upgrade()) {
            panel.update(cx, |panel, cx| {
                panel.cancel_global_voice(
                    &self.owner.as_ref().unwrap().attempt,
                    "global voice listener shut down (window closed or Desktop reload)",
                    cx,
                )
            });
        }
        self.close_overlay(cx);
    }
}

impl Workspace {
    pub(super) fn start_global_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.start_global_voice_fixture(window, cx) {
            return;
        }
        #[cfg(target_os = "linux")]
        self.start_global_voice_listener(window, cx);
        #[cfg(not(target_os = "linux"))]
        let _ = (window, cx);
    }

    /// Opt-in evdev capture. Works under both X11 and Wayland because it reads
    /// the kernel, but only where a non-focusing OS pill backend exists.
    #[cfg(target_os = "linux")]
    fn start_global_voice_listener(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Never inspect live input in tests, screenshots or fixture processes.
        if cfg!(test)
            || harness::screenshot_mode()
            || !overlay::available(cx)
            || !crate::config::get().voice.global_hold
            || crate::config::get().voice.global_devices.is_empty()
            || std::env::var("JCODE_DESKTOP_GLOBAL_VOICE").as_deref() == Ok("0")
        {
            return;
        }
        let listener = match Listener::new(&crate::config::get().voice.global_devices) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("global voice unavailable: {error}");
                return;
            }
        };
        eprintln!(
            "global voice: listening on {} configured device(s)",
            crate::config::get().voice.global_devices.len()
        );
        let mut listener = listener;
        // Per-window launch, not process args: voice windows join the shared
        // single-panel host, whose process args belong to its first window.
        if self.launch.args.iter().any(|arg| arg == SPAWNED_HOLD_FLAG) {
            // Shift+Copilot opened this window. Record the hold that is
            // still down, so the opener's key-up finishes this transcript.
            listener.adopt_held_key();
            self.global_voice.adopting_spawned_hold = true;
        }
        self.global_voice.listener = Some(listener);
        self.global_voice.set_active(window.is_window_active());
        self.ensure_global_voice_task(window, cx);
    }

    /// One UI-owned poll loop per workspace, shared by both edge sources.
    /// Dropping the workspace (reload/close) drops the task and microphone.
    fn ensure_global_voice_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.global_voice.has_task() {
            return;
        }
        cx.on_release(|this, cx| this.global_voice.shutdown(cx))
            .detach();
        self.global_voice.run_task(|| cx.spawn_in(window, async move |this, cx| {
            let mut interval = IDLE_POLL;
            loop {
                let slept = Instant::now();
                cx.background_executor().timer(interval).await;
                let late = slept.elapsed().saturating_sub(interval);
                let result = cx.update(|window, cx| {
                    this.update(cx, |this, cx| {
                        if this.global_voice.held && late >= Duration::from_millis(250) {
                            eprintln!(
                                "global voice: key poll stalled {}ms during a hold",
                                late.as_millis()
                            );
                        }
                        this.poll_global_voice(window, cx);
                        // A held key is about to be released: poll faster so
                        // key-up reaches the recorder without a visible lag.
                        interval = if this.global_voice.held {
                            HELD_POLL
                        } else {
                            IDLE_POLL
                        };
                    })
                });
                if !matches!(result, Ok(Ok(()))) {
                    let _ = this.update(cx, |this, cx| this.global_voice.shutdown(cx));
                    break;
                }
            }
        }));
    }

    fn poll_global_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.global_voice.set_active(window.is_window_active());
        #[cfg(target_os = "linux")]
        {
            let eligible = !self.account_sign_in_visible && self.voice_target(cx).is_some();
            if let Some(listener) = self.global_voice.listener.as_mut() {
                listener.set_eligible(eligible);
            }
            let edges = self
                .global_voice
                .listener
                .as_mut()
                .map(Listener::poll)
                .unwrap_or_default();
            for edge in edges {
                match edge {
                    Edge::Press
                        if press_spawns_window(
                            self.global_voice.window_active,
                            std::mem::take(&mut self.global_voice.adopting_spawned_hold),
                        ) =>
                    {
                        // A focused Jcode CLI gets the transcript. Anything
                        // else opens a new voice window, as before.
                        self.global_press_unfocused(window, cx)
                    }
                    Edge::Press => self.global_voice_press(window, cx),
                    Edge::Spawn if !self.single_panel => {
                        // Workspace mode: add a fresh panel here and record
                        // into it, rather than opening a separate window.
                        let dir = crate::config::get()
                            .voice
                            .spawn_working_dir
                            .as_ref()
                            .map(|dir| dir.to_string_lossy().into_owned())
                            .or_else(|| self.default_working_dir());
                        self.open_default_draft(dir, cx);
                        self.global_voice_press(window, cx)
                    }
                    Edge::Spawn => {
                        if !spawn_voice_window() {
                            self.global_voice_press(window, cx)
                        }
                    }
                    Edge::Tap => {
                        self.global_voice.adopting_spawned_hold = false;
                        self.toggle_voice(&ToggleVoice, window, cx)
                    }
                    Edge::Release
                        if self.global_voice.cli_resolve.is_some()
                            || self
                                .global_voice
                                .cli
                                .as_ref()
                                .is_some_and(|c| c.recording()) =>
                    {
                        self.global_voice.cli_resolve = None;
                        self.cli_release()
                    }
                    Edge::Release => self.global_voice_release(cx),
                    Edge::Cancel => {
                        eprintln!("global voice: canceled by input listener");
                        self.cancel_global_voice_capture("input listener canceled the hold", cx)
                    }
                }
            }
        }
        if self.global_voice.take_expired_host_hold() {
            eprintln!("global voice: host shortcut hold exceeded its deadline, finishing it");
            self.global_voice_release(cx);
        }
        #[cfg(target_os = "linux")]
        let cli_recording = self
            .global_voice
            .cli
            .as_ref()
            .is_some_and(|c| c.recording());
        #[cfg(not(target_os = "linux"))]
        let cli_recording = false;
        if self.global_voice.permission_check_due(cli_recording) {
            self.check_global_voice_permission(false, window, cx);
        }
        #[cfg(target_os = "linux")]
        if self.poll_cli_capture(cx) {
            return;
        }
        self.update_global_voice_overlay(cx);
    }

    fn global_voice_press(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        eprintln!("global voice: press");
        let target = self
            .voice_target(cx)
            .and_then(|index| self.slots[index].panel.chat())
            .cloned();
        if window.is_window_active()
            && let Some(panel) = target.as_ref()
        {
            panel.update(cx, |panel, cx| {
                if !panel.voice_active() {
                    panel.capture_focused_ask_voice_target(window, cx);
                }
            });
        }
        self.global_voice.begin_press(target.as_ref().map(Entity::downgrade));
        self.check_global_voice_permission(true, window, cx);
    }

    fn global_voice_release(&mut self, cx: &mut Context<Self>) {
        eprintln!("global voice: release");
        if let Some((panel, attempt)) = self.global_voice.release() {
            panel.update(cx, |panel, cx| panel.end_global_voice_hold(&attempt, cx));
        }
    }

    /// Host global shortcut press (macOS Carbon / Windows RegisterHotKey).
    /// The host never activates the window for this action.
    pub(super) fn begin_global_voice_hold_action(
        &mut self,
        _: &BeginGlobalVoiceHold,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let active = window.is_window_active();
        self.global_voice.set_active(active);
        if !host_press_uses_global_owner(active, overlay::available(cx)) {
            if !active {
                // Never record invisibly: no OS pill exists on this backend.
                eprintln!(
                    "global voice: no non-focusing overlay on the {:?} backend; focus Jcode to dictate",
                    cx.compositor_name()
                );
                return;
            }
            // Focused: behave exactly like the in-app hold shortcut.
            self.begin_voice_hold(&BeginVoiceHold, window, cx);
            return;
        }
        if self.global_voice.held {
            return;
        }
        self.ensure_global_voice_task(window, cx);
        self.global_voice.note_host_hold();
        self.global_voice_press(window, cx);
    }

    pub(super) fn end_global_voice_hold_action(
        &mut self,
        _: &EndGlobalVoiceHold,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        // Release both possible owners. Neither path ever refocuses.
        self.end_voice_hold(&EndVoiceHold, window, cx);
        if self.global_voice.hold_active() {
            self.global_voice_release(cx);
        }
    }

    fn cancel_global_voice_capture(&mut self, reason: &'static str, cx: &mut Context<Self>) {
        if let Some((panel, attempt)) = self.global_voice.abort() {
            panel.update(cx, |panel, cx| {
                panel.cancel_global_voice(&attempt, reason, cx)
            });
        }
        self.global_voice.close_overlay(cx);
    }

    fn check_global_voice_permission(
        &mut self,
        start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let check = cx
            .background_executor()
            .spawn(crate::global_voice_session::permitted());
        self.global_voice.await_permission(|serial| cx.spawn_in(window, async move |this, cx| {
            let allowed = check.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.global_voice.permission_answered(serial) {
                    return;
                }
                if !allowed {
                    eprintln!("global voice: denied by session check");
                    this.cancel_global_voice_capture("session check denied voice", cx);
                } else if start && this.global_voice.held {
                    this.begin_global_voice(window, cx);
                }
            });
        }));
    }

    fn begin_global_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.account_sign_in_visible {
            return;
        }
        if let Some(owner) = self.global_voice.owner_panel()
            && owner.read(cx).voice_active() {
                return;
            }
        self.global_voice.close_overlay(cx);
        let Some(panel) = self.global_voice.take_target() else {
            eprintln!("global voice: no eligible chat to receive the transcript");
            return;
        };
        if !self
            .slots
            .iter()
            .any(|slot| !slot.closing && slot.panel.chat() == Some(&panel))
        {
            return;
        }
        if panel.read(cx).voice_active() {
            return;
        }
        // Prove a visible indicator exists before opening the microphone.
        // Focused: the chat's in-panel pill. Unfocused: a non-focusing OS pill.
        // Unsupported compositors must not record invisibly.
        if os_pill_wanted(self.global_voice.window_active, true, false)
            && !self.show_global_voice_overlay(
                Snapshot {
                    title: "Connecting…".into(),
                    levels: None,
                    decided: false,
                },
                cx,
            )
        {
            // Restore native focused-key handling instead of swallowing
            // every future Copilot press on an unsupported compositor.
            self.global_voice.disable_capture_source();
            return;
        }
        // No activate_window, set_active or focus_input here. The source draft
        // receives the result while the user's other application keeps focus.
        if let Some(attempt) = panel.update(cx, |panel, cx| panel.begin_global_voice_hold(cx)) {
            // Same Jev routing as a focused hold: the last 20 sessions plus
            // quick actions. The OS pill then shows what Jev decided.
            self.configure_voice_navigation(&panel, window, cx);
            self.global_voice.start_capture(panel.downgrade(), attempt);
        } else {
            self.global_voice.close_overlay(cx);
        }
    }

    fn update_global_voice_overlay(&mut self, cx: &mut Context<Self>) {
        let (Some(panel), Some(attempt)) =
            (self.global_voice.owner_panel(), self.global_voice.attempt())
        else {
            self.global_voice.close_overlay(cx);
            return;
        };
        if !self
            .slots
            .iter()
            .any(|slot| !slot.closing && slot.panel.chat() == Some(&panel))
        {
            panel.update(cx, |panel, cx| {
                panel.cancel_global_voice(&attempt, "owning chat panel is closing", cx)
            });
            self.global_voice.close_overlay(cx);
            return;
        }
        let snapshot = panel.read(cx).global_voice_snapshot(&attempt);
        if snapshot.is_none() {
            self.global_voice.close_overlay(cx);
            return;
        }
        if !panel.read(cx).voice_active() {
            if self.global_voice.result_pill_expired() {
                // The result pill expires in both places, so focusing this
                // window later never reveals a stale "Sent to agent".
                panel.update(cx, |panel, cx| {
                    panel.cancel_global_voice(&attempt, "result pill expired", cx)
                });
                self.global_voice.close_overlay(cx);
                return;
            }
        }
        let Some(snapshot) = snapshot else { return };
        let decided = snapshot.decided;
        if !os_pill_wanted(self.global_voice.window_active, true, decided) {
            // Focus moved to this window mid-hold: the in-panel pill takes over.
            self.global_voice.hide_overlay(cx);
            return;
        }
        if decided && !self.global_voice.has_overlay() {
            // Route only, never transcript content.
            eprintln!("global voice: showing decision: {}", snapshot.title);
        }
        // Focus left mid-hold: mirror the in-panel pill at the OS level.
        let shown = self.show_global_voice_overlay(snapshot, cx);
        if !shown && decided {
            // Nothing is recording. Keep the in-panel decision intact.
            self.global_voice.hide_overlay(cx);
        } else if !shown {
            // Never keep recording without a visible indicator.
            panel.update(cx, |panel, cx| {
                panel.cancel_global_voice(&attempt, "OS voice pill could not be shown", cx)
            });
            self.global_voice.close_overlay(cx);
        }
    }

    fn show_global_voice_overlay(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) -> bool {
        match self.global_voice.show_snapshot(snapshot, cx) {
            Ok(opened) => {
                if opened {
                    eprintln!("global voice: OS pill shown");
                }
                true
            }
            Err(error) => {
                eprintln!("global voice overlay unavailable: {error:#}");
                false
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod spawn_tests {
    use super::*;

    #[test]
    fn only_unfocused_non_adopting_presses_spawn_a_window() {
        assert!(press_spawns_window(false, false));
        assert!(!press_spawns_window(true, false));
        assert!(!press_spawns_window(false, true));
        assert!(!press_spawns_window(true, true));
    }

    #[test]
    fn spawned_voice_window_is_single_panel_and_keeps_only_reload_choice() {
        let own = ["--hot-reload", "--resume", "--toggle-voice", "--session=a"]
            .map(std::ffi::OsString::from);
        assert_eq!(
            spawn_args(own.into_iter()),
            ["--single-panel", SPAWNED_HOLD_FLAG, "--hot-reload"].map(std::ffi::OsString::from)
        );
    }
}

#[cfg(test)]
mod pill_tests {
    use super::{host_press_uses_global_owner, os_pill_wanted};

    #[test]
    fn host_shortcut_only_takes_global_path_while_unfocused_with_a_pill() {
        assert!(host_press_uses_global_owner(false, true));
        assert!(
            !host_press_uses_global_owner(true, true),
            "focused keeps its native hold"
        );
        assert!(
            !host_press_uses_global_owner(false, false),
            "never record invisibly"
        );
        assert!(!host_press_uses_global_owner(true, false));
    }

    #[test]
    fn os_pill_mirrors_live_capture_only_while_unfocused() {
        assert!(os_pill_wanted(false, true, false));
        assert!(
            !os_pill_wanted(true, true, false),
            "focused chat already shows its own live pill"
        );
        assert!(!os_pill_wanted(false, false, false));
        assert!(!os_pill_wanted(true, false, false));
    }

    #[test]
    fn decision_uses_one_pill_in_panel_when_focused_or_os_level_otherwise() {
        assert!(os_pill_wanted(false, true, true));
        assert!(
            !os_pill_wanted(true, true, true),
            "a focused window shows the sent confirmation in the panel only"
        );
        assert!(!os_pill_wanted(true, false, true));
    }
}

#[cfg(test)]
mod host_action_tests {
    use super::*;

    fn workspace_with_two_chats(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<Workspace>, &mut gpui::VisualTestContext) {
        cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel("first", cx);
            workspace.push_test_panel("second", cx);
            // Preview panels never open a microphone.
            for slot in &mut workspace.slots {
                slot.panel =
                    PanelItem::Chat(cx.new(|cx| {
                        Panel::new_preview(crate::preview_state::PreviewState::Empty, cx)
                    }));
            }
            workspace
        })
    }

    /// A host hold that outlives its deadline is finished like a key-up, so
    /// the words spoken so far are delivered. Canceling it would discard them.
    #[gpui::test]
    fn host_hold_deadline_finishes_the_dictation_instead_of_canceling_it(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = workspace_with_two_chats(cx);
        let serial = workspace.read_with(vcx, |w, _| w.global_voice.press_serial);
        vcx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.global_voice.held = true;
                workspace.global_voice.host_held_since =
                    Some(Instant::now() - HOST_HOLD_DEADLINE - Duration::from_secs(1));
                workspace.poll_global_voice(window, cx);
            })
        });
        workspace.read_with(vcx, |workspace, _| {
            assert!(!workspace.global_voice.held);
            assert!(workspace.global_voice.host_held_since.is_none());
            // Canceling bumps the serial (it invalidates the attempt). Releasing does not.
            assert_eq!(workspace.global_voice.press_serial, serial);
        });
    }

    // On Linux, GPUI's fake platform reports no compositor, so no overlay
    // backend exists, like GNOME/Mutter Wayland. macOS/Windows always have one.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn unfocused_host_press_without_overlay_never_records_or_steals_focus(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) = workspace_with_two_chats(cx);
        vcx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.set_active(1, cx);
                workspace.focus_active(window, cx);
            })
        });
        vcx.deactivate_window();
        vcx.update(|window, cx| {
            assert!(!window.is_window_active());
            assert!(!overlay::available(cx));
            let press = cx
                .build_action("workspace::BeginGlobalVoiceHold", None)
                .unwrap();
            window.focus_next(cx);
            assert!(window.is_action_available(press.as_ref(), cx));
            window.dispatch_action(press, cx);
        });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            assert!(
                !window.is_window_active(),
                "host press must not activate the window"
            );
            workspace.read_with(cx, |workspace, cx| {
                assert!(!workspace.global_voice.held);
                assert!(workspace.global_voice.owner.is_none());
                assert!(workspace.global_voice.overlay.is_none(), "no OS pill");
                assert_eq!(workspace.active, 1, "press must not move the active chat");
                assert!(
                    workspace
                        .slots
                        .iter()
                        .all(|slot| !slot.panel.read(cx).voice_active())
                );
            });
            let release = cx
                .build_action("workspace::EndGlobalVoiceHold", None)
                .unwrap();
            assert!(window.is_action_available(release.as_ref(), cx));
            window.dispatch_action(release, cx);
        });
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, _| {
            assert!(workspace.global_voice.host_held_since.is_none());
        });
    }

    #[gpui::test]
    fn focused_host_press_uses_in_panel_pill_not_os_pill(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = workspace_with_two_chats(cx);
        vcx.update(|window, _| window.activate_window());
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            assert!(window.is_window_active());
            let press = cx
                .build_action("workspace::BeginGlobalVoiceHold", None)
                .unwrap();
            window.focus_next(cx);
            window.dispatch_action(press, cx);
        });
        vcx.run_until_parked();
        workspace.read_with(vcx, |workspace, _| {
            // Focused: the native hold path ran. No global owner, no OS pill.
            assert!(workspace.global_voice.window_active);
            assert!(!workspace.global_voice.held);
            assert!(workspace.global_voice.host_held_since.is_none());
            assert!(workspace.global_voice.owner.is_none());
            assert!(workspace.global_voice.overlay.is_none());
        });
    }
}
