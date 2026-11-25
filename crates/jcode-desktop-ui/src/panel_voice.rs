//! Streaming dictation. Audio and interim text are never snapshotted.
use super::*;
use gpui::FutureExt as _;
use jcode_base::voice_intent::{self, SessionCandidate, VoiceIntent};
use jcode_voice::{self as voice, NariEvent, NariRecording, VoiceError};
use std::sync::atomic::{AtomicBool, Ordering};

gpui::actions!(panel_voice, [ToggleVoice]);

/// Jev voice routing is temporarily off. Every finished utterance is sent
/// straight to its panel's agent. Planned return: Jev chooses between the
/// focused panel and one of the other open sessions (see docs/jev-actions.md).
pub(crate) const JEV_ROUTING: bool = false;

pub(crate) fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([gpui::KeyBinding::new(
        "ctrl-shift-v",
        ToggleVoice,
        Some("ChatPanel"),
    )]);
}

#[path = "panel_voice_overlay.rs"]
mod overlay;
pub(crate) use jcode_desktop_voice::tag;
// The button's keycap, tooltip, labels and crossfade hold no panel state.
use jcode_desktop_voice::chrome::*;

#[cfg(test)]
#[path = "panel_global_voice_tests.rs"]
mod global_tests;

/// `[dictation] mode` for a finished utterance. Read per utterance so a
/// Settings change applies to the next one without restarting Desktop.
pub(crate) fn dictation_mode() -> jcode_base::protocol::TranscriptMode {
    jcode_base::config::config().dictation.mode
}

pub(crate) fn superwhisper_bridge_path() -> Option<std::path::PathBuf> {
    Some(jcode_desktop_model::platform_dirs::jcode_home()?.join("bin/superwhisper-dictate.sh"))
}

pub(crate) fn bridge_command(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Checking,
    Recording,
    Transcribing,
    Routing,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Checking => "connecting",
            Phase::Recording => "recording",
            Phase::Transcribing => "transcribing",
            Phase::Routing => "routing",
        }
    }
}

/// Ephemeral routing evidence. Never included in persisted panel snapshots.
#[derive(Clone)]
pub(crate) struct VoiceTrace {
    transcript: String,
    candidates: Vec<SessionCandidate>,
    questions: Vec<voice_intent::VoiceQuestion>,
    answers: Vec<voice_intent::VoiceAnswer>,
    /// Streamed microphone audio, the Nari billing unit. Measured locally.
    audio: Option<Duration>,
    /// Provider-reported Jev usage. `None` while routing or when unreported.
    usage: Option<voice_intent::VoiceUsage>,
}

#[derive(Default)]
pub(super) struct VoiceState {
    phase: Phase,
    // Attempt ownership persists through final transcription, even after key release.
    hold_capture: bool,
    // Unfocused capture driven by the global hold. It routes through Jev like a
    // focused hold, but its status and Jev's decision are mirrored in the OS pill.
    global_capture: bool,
    recording: Option<NariRecording>,
    superwhisper_capture: bool,
    inline_draft: bool,
    live_transcript: String,
    levels: [f32; 24],
    canceled: Arc<AtomicBool>,
    started: Option<Instant>,
    // Audio length at stop, so final transcription latency is not billed as audio.
    audio: Option<Duration>,
    error: Option<String>,
    task: Option<Task<()>>,
    timer: Option<Task<()>>,
    sessions: Option<Vec<jcode_sdk::SessionInfo>>,
    navigation: Option<gpui::Subscription>,
    actions: Option<gpui::Subscription>,
    decision: Option<String>,
    trace: Option<VoiceTrace>,
    trace_expanded: bool,
    trace_preview: bool,
    /// Why stop was requested, for the end-of-recording log. `None` at the
    /// end means the recording ended on its own (duration cap or provider).
    stop_reason: Option<&'static str>,
    /// Loudest sampled microphone level, logged to tell silence from speech.
    peak: f32,
    /// The end of this attempt was already logged.
    end_logged: bool,
    /// Provider and model this attempt started, for messages and the log.
    provider: Option<String>,
    /// Why the provider stream stopped early. The words so far are kept.
    interrupted: Option<VoiceError>,
}

/// Timestamped voice lifecycle line in the Desktop log. Never transcript text.
fn voice_log(message: std::fmt::Arguments) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    eprintln!(
        "[unix {}.{:03}] voice: {message}",
        now.as_secs(),
        now.subsec_millis()
    );
}

impl VoiceState {
    /// Every recording end is logged exactly once, as normal or ABNORMAL.
    fn log_end(&mut self, normal: bool, outcome: &str, chars: Option<usize>) {
        if std::mem::replace(&mut self.end_logged, true) {
            return;
        }
        let audio = self
            .audio
            .or_else(|| self.started.map(|started| started.elapsed()))
            .map_or("-".into(), |audio| format!("{:.1}s", audio.as_secs_f32()));
        voice_log(format_args!(
            "ended {}: {outcome} (phase={}, stop={}, audio={audio}, peak={:.3}, chars={}, {}{})",
            if normal { "normal" } else { "ABNORMAL" },
            self.phase.label(),
            self.stop_reason.unwrap_or("none"),
            self.peak,
            chars.map_or("-".into(), |chars| chars.to_string()),
            if self.hold_capture { "hold" } else { "toggle" },
            if self.global_capture { ", global" } else { "" },
        ));
    }
}

pub(crate) struct VoiceSessionRequested(pub jcode_sdk::SessionInfo, pub String);
impl gpui::EventEmitter<VoiceSessionRequested> for Panel {}

pub(crate) struct VoiceActionRequested(pub voice_intent::QuickAction, pub String);
impl gpui::EventEmitter<VoiceActionRequested> for Panel {}

impl Drop for VoiceState {
    fn drop(&mut self) {
        if self.phase != Phase::Idle {
            self.log_end(false, "panel dropped (chat closed or Desktop reload)", None);
        }
        self.canceled.store(true, Ordering::SeqCst);
        if let Some(recording) = self.recording.take() {
            // NariRecording signals cancellation without joining the native worker.
            drop(recording);
        }
    }
}

impl Panel {
    #[cfg(test)]
    pub(crate) fn resolve_voice_for_test(
        &mut self,
        text: &str,
        result: anyhow::Result<VoiceIntent>,
        cx: &mut Context<Self>,
    ) {
        self.voice.phase = Phase::Routing;
        self.voice.live_transcript = text.into();
        let attempt = self.voice.canceled.clone();
        self.finish_voice_routing(&attempt, result, cx);
    }

    pub(crate) fn configure_voice_navigation(
        &mut self,
        sessions: Vec<jcode_sdk::SessionInfo>,
        subscription: gpui::Subscription,
    ) {
        self.voice.sessions = Some(sessions);
        self.voice.navigation = Some(subscription);
    }

    pub(crate) fn configure_voice_actions(&mut self, subscription: gpui::Subscription) {
        self.voice.actions = Some(subscription);
    }

    pub(crate) fn show_voice_decision(&mut self, message: String, cx: &mut Context<Self>) {
        self.voice.error = None;
        self.voice.decision = Some(message);
        cx.notify();
    }

    /// Refine the decision shown by an unfocused hold's OS pill. Focused
    /// holds already show it on the destination panel.
    pub(crate) fn set_global_voice_decision(&mut self, message: String, cx: &mut Context<Self>) {
        if self.voice.global_capture && self.voice.error.is_none() {
            self.voice.decision = Some(message);
            cx.notify();
        }
    }

    pub(crate) fn voice_trace(&self) -> Option<VoiceTrace> {
        self.voice.trace.clone()
    }

    pub(crate) fn show_voice_trace(&mut self, trace: Option<VoiceTrace>, cx: &mut Context<Self>) {
        self.voice.trace = trace;
        self.voice.trace_expanded = false;
        self.voice.trace_preview = false;
        cx.notify();
    }

    pub(crate) fn keep_voice_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        self.voice.decision = None;
        self.voice.error = Some(
            "Jev chose: Quick action · Navigation unavailable. Transcript kept in the draft."
                .into(),
        );
        self.input
            .update(cx, |input, cx| input.append_dictation(&tag::wrap(text), cx));
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn voice_decision_for_test(&self) -> Option<&str> {
        self.voice.decision.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn set_voice_checking_for_test(&mut self) {
        self.voice = VoiceState::default();
        self.voice.phase = Phase::Checking;
    }

    #[cfg(test)]
    pub(crate) fn set_voice_hold_checking_for_test(&mut self) {
        self.prepare_voice_attempt(true);
    }

    pub(crate) fn supports_voice(&self) -> bool {
        // Transcription runs on this computer and only the text is sent, so
        // SSH-hosted chats (including Jcode Cloud) take voice like local ones.
        (!self.session_id.contains("://")
            || self.is_pending_session()
            || self.session_id.starts_with("preview://")
            || crate::harness::remote_host(&self.session_id).is_some())
            && !self.is_side_document()
            && self.todoist.is_none()
            && self.orchestration.is_none()
    }

    pub(crate) fn voice_active(&self) -> bool {
        self.voice.phase != Phase::Idle
    }

    pub(crate) fn global_voice_snapshot(
        &self,
        attempt: &Arc<AtomicBool>,
    ) -> Option<crate::global_voice_overlay::Snapshot> {
        if !Arc::ptr_eq(attempt, &self.voice.canceled) || !self.voice.global_capture {
            return None;
        }
        use crate::global_voice_overlay::Snapshot;
        let (title, decided) = match self.voice.phase {
            Phase::Idle => match (&self.voice.error, &self.voice.decision) {
                (Some(error), _) => (global_pill_error(error), true),
                (None, Some(decision)) => (global_pill_decision(decision), true),
                (None, None) => ("Added to draft".into(), true),
            },
            Phase::Checking => ("Connecting…".into(), false),
            Phase::Recording => ("Listening".into(), false),
            Phase::Transcribing => ("Transcribing…".into(), false),
            Phase::Routing => ("Jev is choosing…".into(), false),
        };
        Some(Snapshot {
            title,
            levels: (self.voice.phase == Phase::Recording).then_some(self.voice.levels),
            decided,
        })
    }

    pub(crate) fn cancel_global_voice(
        &mut self,
        attempt: &Arc<AtomicBool>,
        reason: &'static str,
        cx: &mut Context<Self>,
    ) {
        if Arc::ptr_eq(attempt, &self.voice.canceled) && self.voice.global_capture {
            self.cancel_voice(false, reason, cx);
        }
    }

    pub(crate) fn begin_voice_hold(&mut self, cx: &mut Context<Self>) {
        if self.supports_voice() && self.voice.phase == Phase::Idle {
            self.start_voice_capture(true, false, cx);
        }
    }

    pub(crate) fn begin_global_voice_hold(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Arc<AtomicBool>> {
        if self.supports_voice() && !self.voice_active() {
            self.start_voice_capture(true, true, cx);
            return Some(self.voice.canceled.clone());
        }
        None
    }

    pub(crate) fn end_global_voice_hold(
        &mut self,
        attempt: &Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        if Arc::ptr_eq(attempt, &self.voice.canceled) && self.voice.global_capture {
            self.end_voice_hold(cx);
        }
    }

    pub(crate) fn end_voice_hold(&mut self, cx: &mut Context<Self>) {
        if !self.voice.hold_capture {
            return;
        }
        match self.voice.phase {
            Phase::Checking => {
                self.cancel_voice(false, "key released before the microphone was ready", cx)
            }
            Phase::Recording => self.stop_voice("key released", cx),
            Phase::Idle | Phase::Transcribing | Phase::Routing => {}
        }
    }

    pub(crate) fn toggle_voice(&mut self, cx: &mut Context<Self>) {
        if !self.supports_voice() {
            return;
        }
        match self.voice.phase {
            Phase::Idle => self.start_voice(cx),
            Phase::Recording => self.stop_voice("voice toggled off", cx),
            Phase::Checking => self.cancel_voice(false, "voice toggled again while connecting", cx),
            Phase::Transcribing | Phase::Routing => self.cancel_voice(
                false,
                "voice toggled again while transcribing (text discarded)",
                cx,
            ),
        }
    }

    /// `normal` marks an intentional user cancel. Everything else is logged
    /// as ABNORMAL so unexpected stops can be found and fixed.
    pub(super) fn cancel_ask_voice(&mut self, cx: &mut Context<Self>) {
        if self.ask_card.is_some() && self.voice_active() {
            self.cancel_voice(true, "ask answered", cx);
        }
    }

    fn cancel_voice(&mut self, normal: bool, reason: &'static str, cx: &mut Context<Self>) {
        if self.voice.phase != Phase::Idle {
            self.voice
                .log_end(normal, &format!("canceled: {reason}"), None);
        }
        if self.voice.inline_draft {
            self.input.update(cx, |input, cx| input.end_live_dictation(None, cx));
        }
        self.reset_voice_attempt();
        self.clear_voice_preview(cx);
        cx.notify();
    }

    fn reset_voice_attempt(&mut self) {
        // Ending paths log explicitly. Only an unlogged drop reports itself.
        self.voice.phase = Phase::Idle;
        // Session configuration belongs to the panel, not an individual capture.
        let sessions = self.voice.sessions.take();
        let navigation = self.voice.navigation.take();
        let actions = self.voice.actions.take();
        self.voice = VoiceState::default();
        self.voice.actions = actions;
        self.voice.sessions = sessions;
        self.voice.navigation = navigation;
    }

    fn prepare_voice_attempt(&mut self, hold_capture: bool) {
        self.reset_voice_attempt();
        self.voice.phase = Phase::Checking;
        self.voice.hold_capture = hold_capture;
    }

    fn start_voice(&mut self, cx: &mut Context<Self>) {
        self.start_voice_capture(false, false, cx);
    }

    fn start_voice_capture(
        &mut self,
        hold_capture: bool,
        global_capture: bool,
        cx: &mut Context<Self>,
    ) {
        if self.preview_state.is_some()
            || (crate::harness::screenshot_mode()
                && !(self.session_id == "screenshot-fixture"
                    && crate::fixture_control_targets::allow_soniox_recording()))
        {
            self.voice.error = Some("Microphone access is disabled in offline previews.".into());
            cx.notify();
            return;
        }
        if self.voice.phase != Phase::Idle {
            return;
        }
        jcode_voice::timing::begin();
        self.prepare_voice_attempt(hold_capture);
        self.voice.global_capture = global_capture;
        // Starting is nonblocking: the microphone opens and the provider
        // handshake runs in the background while PCM buffers. The press is
        // the start of recording, so the UI shows Listening immediately.
        let attempt = self.voice.canceled.clone();
        let (backend, _) = voice::configured_backend();
        match backend {
            voice::DictationBackend::Command => return self.start_command_voice(&attempt, cx),
            voice::DictationBackend::Subscription => {
                voice_log(format_args!(
                    "subscription backend has no streaming capture"
                ));
                self.voice.phase = Phase::Idle;
                self.voice.hold_capture = false;
                self.voice.error = Some(
                    "Jcode subscription voice is not streamed yet. Pick Nari, Soniox, or an external command in Settings."
                        .into(),
                );
                cx.notify();
                return;
            }
            _ => {}
        }
        // `[dictation] backend` and `model` decide the provider. Only `auto`
        // chooses (Nari, else Soniox), and the label says so.
        let result = NariRecording::start_configured_plan(attempt.clone()).map(|(recording, plan)| {
            self.voice.provider = Some(if plan.auto {
                format!("auto: {}", plan.label())
            } else {
                plan.label()
            });
            recording
        });
        self.finish_voice_startup(&attempt, result, cx);
    }

    /// `[dictation] backend = "command"`: the external command owns the
    /// microphone (SuperWhisper, whisper.cpp, ...) and prints the transcript.
    /// A managed SuperWhisper bridge also supports stopping its capture.
    fn start_command_voice(&mut self, attempt: &Arc<AtomicBool>, cx: &mut Context<Self>) {
        let cfg = jcode_base::config::config().dictation.clone();
        let command = cfg.command.trim().to_owned();
        if command.is_empty() {
            self.voice.phase = Phase::Idle;
            self.voice.hold_capture = false;
            self.voice.error =
                Some("Set an external dictation command in Settings > Voice.".into());
            cx.notify();
            return;
        }
        voice_log(format_args!("external dictation command started"));
        self.voice.superwhisper_capture = cfg!(target_os = "macos")
            && superwhisper_bridge_path().is_some_and(|path| command == bridge_command(&path));
        self.voice.phase = if self.voice.superwhisper_capture {
            Phase::Recording
        } else {
            Phase::Transcribing
        };
        self.voice.started = Some(Instant::now());
        // The managed bridge bounds its own recording and stops SuperWhisper at
        // that limit, so a fixed Desktop deadline must not cut a long dictation.
        let timeout_secs = if self.voice.superwhisper_capture {
            0
        } else {
            cfg.timeout_secs
        };
        let task = cx.background_executor().spawn(async move {
            jcode_desktop_harness::network::run(async move {
                voice::dictation::run_command(&command, timeout_secs).await
            })
            .await
            .map_err(anyhow::Error::msg)?
        });
        let attempt = attempt.clone();
        self.voice.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |panel, cx| {
                if !Arc::ptr_eq(&attempt, &panel.voice.canceled)
                    || attempt.load(Ordering::SeqCst)
                    || !matches!(panel.voice.phase, Phase::Recording | Phase::Transcribing)
                {
                    return;
                }
                panel.voice.stop_reason.get_or_insert("external command finished");
                match result {
                    Ok(text) => panel.finish_voice(Ok(text), cx),
                    Err(error) => {
                        panel.voice.log_end(false, "external command failed", None);
                        panel.finish_voice(Err(VoiceError::CaptureFailed), cx);
                        panel.voice.error = Some(format!("Dictation command failed: {error:#}"));
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    fn finish_voice_startup(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: Result<NariRecording, VoiceError>,
        cx: &mut Context<Self>,
    ) {
        if !Arc::ptr_eq(attempt, &self.voice.canceled)
            || attempt.load(Ordering::SeqCst)
            || self.voice.phase != Phase::Checking
        {
            // Dropping a late successful recording cancels its native worker.
            return;
        }
        match result {
            Ok(recording) => {
                voice_log(format_args!(
                    "recording started with {} ({}{})",
                    self.voice.provider.as_deref().unwrap_or("?"),
                    if self.voice.hold_capture {
                        "hold"
                    } else {
                        "toggle"
                    },
                    if self.voice.global_capture {
                        ", global"
                    } else {
                        ""
                    },
                ));
                jcode_voice::timing::mark("ui shows recording");
                if self.voice.global_capture {
                    eprintln!("global voice: recording started");
                }
                self.voice.phase = Phase::Recording;
                self.voice.recording = Some(recording);
                self.voice.started = Some(Instant::now());
                if !self.voice.global_capture
                    && self.ask_voice_input().is_none()
                    && dictation_mode() == jcode_base::protocol::TranscriptMode::Insert
                {
                    self.begin_inline_voice(cx);
                } else {
                    self.input.update(cx, |input, cx| {
                        input.set_voice_preview(Some(String::new()), cx);
                    });
                }
                self.tick_voice(cx);
            }
            Err(error) => {
                self.voice
                    .log_end(false, &format!("failed to start: {error:?}"), None);
                if self.voice.global_capture {
                    eprintln!("global voice: recording failed to start: {error}");
                }
                self.voice.phase = Phase::Idle;
                self.voice.hold_capture = false;
                // Name the backend and model that failed. Nothing else was tried.
                self.voice.error = Some(format!("{}: {error}", voice::configured_label()));
            }
        }
        cx.notify();
    }

    fn tick_voice(&mut self, cx: &mut Context<Self>) {
        const METER: Duration = Duration::from_millis(50);
        let attempt = self.voice.canceled.clone();
        self.voice.timer = Some(cx.spawn(async move |this, cx| {
            let mut last_level = Instant::now();
            loop {
                // Wake as soon as the recording publishes an event (the final
                // transcript above all), not on the next meter tick.
                let ready = this
                    .read_with(cx, |panel, _| {
                        panel
                            .voice
                            .recording
                            .as_ref()
                            .map(NariRecording::event_ready)
                    })
                    .ok()
                    .flatten();
                match ready {
                    Some(ready) => {
                        let _ = ready.with_timeout(METER, cx.background_executor()).await;
                    }
                    None => cx.background_executor().timer(METER).await,
                }
                let sample_level = last_level.elapsed() >= METER;
                if sample_level {
                    last_level = Instant::now();
                }
                let keep_ticking = this
                    .update(cx, |panel, cx| {
                        if !Arc::ptr_eq(&attempt, &panel.voice.canceled) || !panel.voice_active() {
                            return false;
                        }
                        if sample_level && panel.voice.phase == Phase::Recording {
                            let level = panel
                                .voice
                                .recording
                                .as_ref()
                                .map_or(0., NariRecording::audio_level);
                            panel.voice.levels.rotate_left(1);
                            panel.voice.levels[23] = level;
                            panel.voice.peak = panel.voice.peak.max(level);
                        }
                        // Drain a bounded batch so a fast provider cannot monopolize the UI.
                        for _ in 0..64 {
                            let event = panel
                                .voice
                                .recording
                                .as_ref()
                                .and_then(NariRecording::try_event);
                            let Some(event) = event else { break };
                            panel.apply_voice_event(event, cx);
                            if !panel.voice_active() || panel.voice.phase == Phase::Routing {
                                return false;
                            }
                        }
                        if panel
                            .voice
                            .recording
                            .as_ref()
                            .is_some_and(NariRecording::is_finished)
                        {
                            // Re-check after observing worker completion so a final event
                            // published just after the first drain is not lost.
                            while let Some(event) = panel
                                .voice
                                .recording
                                .as_ref()
                                .and_then(NariRecording::try_event)
                            {
                                panel.apply_voice_event(event, cx);
                                if !panel.voice_active() || panel.voice.phase == Phase::Routing {
                                    return false;
                                }
                            }
                            voice_log(format_args!("worker exited without a result"));
                            // finish_voice keeps any words already transcribed.
                            panel.finish_voice(Err(VoiceError::CaptureFailed), cx);
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_ticking {
                    break;
                }
            }
        }));
    }

    pub(super) fn seed_inline_voice_fixture(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_content("Before after".into(), cx);
            input.set_caret("Before ".len(), cx);
        });
        self.voice.phase = Phase::Recording;
        self.begin_inline_voice(cx);
        self.apply_voice_event(NariEvent::Transcript("spoken wor".into()), cx);
    }

    fn begin_inline_voice(&mut self, cx: &mut Context<Self>) {
        self.voice.inline_draft = true;
        self.input.update(cx, |input, cx| input.begin_live_dictation(cx));
    }

    fn apply_voice_event(&mut self, event: NariEvent, cx: &mut Context<Self>) {
        match event {
            NariEvent::Started => {}
            NariEvent::Transcript(text) => {
                self.voice.live_transcript = text;
                let live = &self.voice.live_transcript;
                if self.voice.inline_draft {
                    self.input.update(cx, |input, cx| input.revise_live_dictation(live, cx));
                } else {
                    let target = self.ask_voice_input().unwrap_or_else(|| self.input.clone());
                    target.update(cx, |input, cx| input.set_voice_preview(Some(live.clone()), cx));
                }
                cx.notify();
            }
            NariEvent::Interrupted(error) => self.voice.interrupted = Some(error),
            NariEvent::Finished(result) => self.finish_voice(result, cx),
        }
    }

    /// Drop the dim recognition preview. The final words replace it, either in
    /// the draft or in the chat.
    fn clear_voice_preview(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_voice_preview(None, cx);
            input.set_voice_preview_hint(None, cx);
        });
        if let Some(target) = self.ask_voice_input() {
            target.update(cx, |input, cx| {
                input.set_voice_preview(None, cx);
                input.set_voice_preview_hint(None, cx);
            });
        }
    }

    /// `/dictate` with `backend = "command"` or an external command: the same
    /// `[dictation] mode` rules as the built-in microphone.
    pub(crate) fn apply_command_transcript(
        &mut self,
        run: &jcode_voice::dictation::DictationRun,
        cx: &mut Context<Self>,
    ) {
        use jcode_base::protocol::TranscriptMode;
        let text = run.text.trim();
        if text.is_empty() {
            self.items
                .push(Item::Error("Dictation command returned no words.".into()));
            return;
        }
        match run.mode {
            TranscriptMode::Insert => self
                .input
                .update(cx, |input, cx| input.insert_dictation(text, cx)),
            TranscriptMode::Append => self
                .input
                .update(cx, |input, cx| input.append_dictation(&tag::wrap(text), cx)),
            TranscriptMode::Replace => self.input.update(cx, |input, cx| {
                input.replace_dictation(&tag::wrap(text), cx)
            }),
            TranscriptMode::Send => {
                self.submit_or_queue(tag::wrap(text), Vec::new(), false, cx);
                return;
            }
        }
        self.focus_composer_after_voice(cx);
        cx.notify();
    }

    /// Put finished words in the draft for editing, per `[dictation] mode`.
    /// Nothing is submitted and the composer keeps keyboard focus.
    #[allow(dead_code)]
    fn keep_voice_in_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        let mode = dictation_mode();
        self.keep_voice_in_draft_with(text, mode, cx);
    }

    fn keep_voice_in_draft_with(
        &mut self,
        text: &str,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        use jcode_base::protocol::TranscriptMode;
        self.voice.phase = Phase::Idle;
        self.voice.error = None;
        self.voice.trace = None;
        self.voice.live_transcript.clear();
        // A focused hold shows the draft itself. Only the unfocused OS pill
        // needs a confirmation label.
        self.voice.decision = self
            .voice
            .global_capture
            .then(|| "Inserted into the composer".into());
        match mode {
            TranscriptMode::Insert => self
                .input
                .update(cx, |input, cx| input.insert_dictation(text, cx)),
            TranscriptMode::Append => self
                .input
                .update(cx, |input, cx| input.append_dictation(&tag::wrap(text), cx)),
            TranscriptMode::Replace => self.input.update(cx, |input, cx| {
                input.replace_dictation(&tag::wrap(text), cx)
            }),
            // `send` is handled before this point; never silently send here.
            TranscriptMode::Send => self
                .input
                .update(cx, |input, cx| input.insert_dictation(text, cx)),
        }
        self.focus_composer_after_voice(cx);
        jcode_voice::timing::mark("ui kept transcript in draft");
        cx.notify();
    }

    /// Hand typing back to the composer after dictation, so the user edits the
    /// words without reaching for the mouse. Deferred: dictation finishes
    /// inside an entity update, where no window is borrowed.
    fn focus_composer_after_voice(&self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        cx.defer(move |app| {
            let Some(panel) = weak.upgrade() else {
                return;
            };
            let Some(window) = app.active_window() else {
                return;
            };
            let _ = window.update(app, |_, window, app| {
                panel.update(app, |panel, cx| panel.focus_input(window, cx));
            });
        });
    }

    fn stop_voice(&mut self, reason: &'static str, cx: &mut Context<Self>) {
        if self.session_id == "screenshot-fixture" && crate::fixture_control_targets::voice_recording()
        {
            self.voice.stop_reason = Some(reason);
            self.apply_voice_event(NariEvent::Finished(Ok("spoken words".into())), cx);
            return;
        }
        if let Some(recording) = self.voice.recording.as_ref() {
            recording.stop();
        } else if self.voice.superwhisper_capture {
            let attempt = self.voice.canceled.clone();
            let stop = cx.background_executor().spawn(async move {
                std::process::Command::new("/usr/bin/open")
                    .args(["-g", "superwhisper://record/stop"])
                    .status()
                    .is_ok_and(|status| status.success())
            });
            cx.spawn(async move |this, cx| {
                if !stop.await {
                    let _ = this.update(cx, |panel, cx| {
                        if Arc::ptr_eq(&attempt, &panel.voice.canceled)
                            && panel.voice.phase == Phase::Transcribing
                        {
                            panel.voice.phase = Phase::Recording;
                            panel.voice.audio = None;
                            panel.voice.stop_reason = None;
                            panel.voice.error = Some("Could not stop SuperWhisper. Use its own recording shortcut.".into());
                            cx.notify();
                        }
                    });
                }
            }).detach();
        } else {
            return;
        }
        jcode_voice::timing::release();
        self.voice.stop_reason = Some(reason);
        self.voice.audio = self.voice.started.map(|started| started.elapsed());
        voice_log(format_args!(
            "stop requested: {reason} after {:.1}s",
            self.voice.audio.unwrap_or_default().as_secs_f32()
        ));
        self.voice.phase = Phase::Transcribing;
        cx.notify();
    }

    fn finish_voice(&mut self, result: Result<String, VoiceError>, cx: &mut Context<Self>) {
        let mode = if self.voice.inline_draft {
            jcode_base::protocol::TranscriptMode::Insert
        } else {
            dictation_mode()
        };
        self.finish_voice_with_mode(result, mode, cx);
    }

    /// Words already transcribed outlive any error: a limit or failure never
    /// discards them. The reason is reported next to the kept text.
    fn finish_voice_with_mode(
        &mut self,
        result: Result<String, VoiceError>,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        let result = match result {
            Err(error)
                if error != VoiceError::Cancelled && !self.voice.live_transcript.trim().is_empty() =>
            {
                self.voice.interrupted.get_or_insert(error);
                Ok(self.voice.live_transcript.clone())
            }
            other => other,
        };
        let note = match (&result, self.voice.interrupted.take()) {
            (Ok(text), Some(error)) if !text.trim().is_empty() => Some(format!(
                "{} stopped early: {error}. The words captured before that were kept.",
                self.voice.provider.as_deref().unwrap_or("Voice input")
            )),
            _ => None,
        };
        self.finish_voice_text(result, mode, cx);
        if let Some(note) = note {
            voice_log(format_args!("kept transcript after interruption: {note}"));
            if self.voice.phase == Phase::Idle && self.voice.error.is_none() {
                self.voice.error = Some(note);
                cx.notify();
            }
        }
    }

    fn finish_voice_text(
        &mut self,
        result: Result<String, VoiceError>,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        jcode_voice::timing::mark("ui received final transcript");
        match &result {
            Ok(text) if text.trim().is_empty() => self.voice.log_end(
                self.voice.stop_reason.is_some(),
                "no speech in transcript (silence or filtered prompt echo)",
                Some(0),
            ),
            Ok(text) if self.voice.stop_reason.is_some() => {
                self.voice
                    .log_end(true, "transcribed", Some(text.chars().count()))
            }
            Ok(text) => self.voice.log_end(
                false,
                "recording ended without a stop request (safety cap or provider)",
                Some(text.chars().count()),
            ),
            Err(error) => self
                .voice
                .log_end(false, &format!("error: {error:?}"), None),
        }
        self.voice.hold_capture = false;
        self.voice.phase = Phase::Idle;
        let audio = self
            .voice
            .audio
            .take()
            .or_else(|| self.voice.started.map(|started| started.elapsed()));
        self.voice.started = None;
        self.voice.recording = None;
        self.voice.superwhisper_capture = false;
        self.voice.live_transcript.clear();
        self.clear_voice_preview(cx);
        let was_inline = std::mem::take(&mut self.voice.inline_draft);
        if was_inline {
            let final_text = result.as_ref().ok().map(String::as_str);
            self.input.update(cx, |input, cx| input.end_live_dictation(final_text, cx));
        }
        match result {
            Ok(text) if !text.trim().is_empty() => {
                if was_inline {
                    self.voice.error = None;
                    self.focus_composer_after_voice(cx);
                    cx.notify();
                    return;
                }
                if !self.voice.global_capture && let Some(input) = self.ask_voice_input() {
                    input.update(cx, |input, cx| input.insert_dictation(&text, cx));
                    self.voice.error = None;
                    cx.notify();
                    return;
                }
                if self.voice.global_capture {
                    self.voice.phase = Phase::Transcribing;
                    let attempt = self.voice.canceled.clone();
                    let check = cx
                        .background_executor()
                        .spawn(crate::global_voice_session::permitted());
                    self.voice.task = Some(cx.spawn(async move |this, cx| {
                        let allowed = check.await;
                        let _ = this.update(cx, |panel, cx| {
                            panel
                                .finish_global_voice_text(&attempt, text, audio, allowed, mode, cx);
                        });
                    }));
                    cx.notify();
                    return;
                }
                // `[dictation] mode` decides. Only an explicit `send` submits:
                // voice is an input method, so the default leaves the words in
                // the composer at the caret for the user to edit and send.
                if mode != jcode_base::protocol::TranscriptMode::Send {
                    self.keep_voice_in_draft_with(&text, mode, cx);
                    return;
                }
                if self.voice.sessions.is_some() || !JEV_ROUTING {
                    self.route_voice(text, audio, cx);
                    return;
                }
                self.input.update(cx, |input, cx| {
                    input.append_dictation(&tag::wrap(&text), cx);
                });
                self.voice.error = None;
                self.focus_composer_after_voice(cx);
            }
            Ok(_) => self.voice.error = Some("No speech was detected. Try recording again.".into()),
            Err(error) => self.voice.error = Some(error.to_string()),
        }
        if self.voice.global_capture
            && let Some(error) = &self.voice.error
        {
            eprintln!("global voice: finished without text: {error}");
        }
        cx.notify();
    }

    fn finish_global_voice_text(
        &mut self,
        attempt: &Arc<AtomicBool>,
        text: String,
        audio: Option<Duration>,
        allowed: bool,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        if !Arc::ptr_eq(attempt, &self.voice.canceled)
            || attempt.load(Ordering::SeqCst)
            || !self.voice.global_capture
            || self.voice.phase != Phase::Transcribing
        {
            return;
        }
        if allowed && let Some(input) = self.ask_voice_input() {
            input.update(cx, |input, cx| input.insert_dictation(&text, cx));
            self.voice.phase = Phase::Idle;
            self.voice.error = None;
            self.voice.decision = Some("Inserted into the answer".into());
            cx.notify();
        } else if allowed && mode != jcode_base::protocol::TranscriptMode::Send {
            // Unfocused hold into the draft: the words appear in the composer
            // the user comes back to, and the OS pill says so.
            eprintln!(
                "global voice: kept {} transcript chars in the draft",
                text.chars().count()
            );
            self.keep_voice_in_draft_with(&text, mode, cx);
        } else if allowed && (self.voice.sessions.is_some() || !JEV_ROUTING) {
            // Length only. Never log transcript content.
            eprintln!(
                "global voice: sending {} transcript chars",
                text.chars().count()
            );
            self.route_voice(text, audio, cx);
        } else if allowed {
            eprintln!(
                "global voice: inserted {} transcript chars",
                text.chars().count()
            );
            self.input.update(cx, |input, cx| {
                input.append_dictation(&tag::wrap(&text), cx)
            });
            self.voice.phase = Phase::Idle;
            self.voice.error = None;
            self.focus_composer_after_voice(cx);
            cx.notify();
        } else {
            self.cancel_global_voice(attempt, "session check denied the transcript", cx);
        }
    }

    /// Send only this utterance, never text or attachments already in the
    /// composer. Sent ASAP like Enter: an active turn is steered.
    fn send_voice_directly(&mut self, text: String, cx: &mut Context<Self>) {
        self.voice.phase = Phase::Idle;
        self.voice.error = None;
        self.voice.trace = None;
        self.voice.live_transcript.clear();
        // Focused holds show the sent prompt in the transcript. Only the
        // unfocused OS pill needs a confirmation label.
        self.voice.decision = self.voice.global_capture.then(|| "Sent to agent".into());
        self.submit_or_queue(tag::wrap(&text), Vec::new(), false, cx);
        jcode_voice::timing::mark("ui sent transcript");
        cx.notify();
    }

    fn route_voice(&mut self, text: String, audio: Option<Duration>, cx: &mut Context<Self>) {
        if !JEV_ROUTING {
            self.send_voice_directly(text, cx);
            return;
        }
        self.voice.phase = Phase::Routing;
        self.voice.live_transcript = text.clone();
        self.voice.error = None;
        let attempt = self.voice.canceled.clone();
        let candidates = self
            .voice
            .sessions
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|s| SessionCandidate {
                id: s.session_id.clone(),
                title: match jcode_core::id::extract_session_name(&s.session_id) {
                    Some(name) => format!("{name}: {}", s.title.as_deref().unwrap_or_default()),
                    None => s.title.clone().unwrap_or_default(),
                },
                working_dir: s.working_dir.clone(),
            })
            .collect::<Vec<_>>();
        let questions = match voice_intent::describe_questions(&text, &candidates) {
            Ok(questions) => questions,
            Err(error) => {
                self.finish_voice_routing(&attempt, Err(error), cx);
                return;
            }
        };
        self.voice.trace = Some(VoiceTrace {
            transcript: text.clone(),
            candidates: candidates.clone(),
            questions,
            answers: Vec::new(),
            audio,
            usage: None,
        });
        let work = cx.background_executor().spawn(async move {
            jcode_desktop_harness::network::run(async move {
                voice_intent::classify_with_report(&text, &candidates).await
            })
            .await
            .map_err(anyhow::Error::msg)?
        });
        self.voice.task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |panel, cx| {
                panel.finish_voice_report(&attempt, result, cx);
            });
        }));
        cx.notify();
    }

    fn finish_voice_report(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: anyhow::Result<voice_intent::VoiceClassification>,
        cx: &mut Context<Self>,
    ) {
        // A canceled or superseded request cannot overwrite the visible evidence.
        if !Arc::ptr_eq(attempt, &self.voice.canceled)
            || attempt.load(Ordering::SeqCst)
            || self.voice.phase != Phase::Routing
        {
            return;
        }
        let intent = result.map(|report| {
            if let Some(trace) = &mut self.voice.trace {
                trace.answers = report.answers;
                trace.usage = report.usage;
            }
            report.intent
        });
        self.finish_voice_routing(attempt, intent, cx);
    }

    fn finish_voice_routing(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: anyhow::Result<VoiceIntent>,
        cx: &mut Context<Self>,
    ) {
        if !Arc::ptr_eq(attempt, &self.voice.canceled)
            || attempt.load(Ordering::SeqCst)
            || self.voice.phase != Phase::Routing
        {
            return;
        }
        self.voice.phase = Phase::Idle;
        let mut target = None;
        let mut quick_action = None;
        let mut coding_agent = false;
        let error = match result {
            Ok(VoiceIntent::CodingAgent) => {
                coding_agent = true;
                None
            }
            Ok(VoiceIntent::QuickAction(action)) => {
                if self.voice.actions.is_some() {
                    quick_action = Some(action);
                    None
                } else {
                    Some("Jev chose: Quick action · Navigation unavailable. Transcript kept in the draft.".into())
                }
            }
            Ok(VoiceIntent::Dictation) => None,
            Ok(VoiceIntent::OpenSession(id)) => {
                target = self
                    .voice
                    .sessions
                    .as_ref()
                    .and_then(|sessions| sessions.iter().find(|s| s.session_id == id))
                    .cloned();
                target.is_none().then(|| "Jev could not match a session in your last 20. Transcript kept in the draft.".to_string())
            }
            Err(error) => Some(format!(
                "Jev routing unavailable: {error}. Transcript kept in the draft."
            )),
        };
        let text = std::mem::take(&mut self.voice.live_transcript);
        self.voice.error = error;
        if coding_agent {
            // Send only this utterance, never text or attachments already in the composer.
            // Send ASAP like Enter: an active turn is steered, not queued behind.
            self.voice.decision = Some("Jev chose: Coding agent · Sent now".into());
            self.submit_or_queue(tag::wrap(&text), Vec::new(), false, cx);
        } else if let Some(action) = quick_action {
            self.voice.decision = Some("Jev chose: Quick action · Navigation".into());
            cx.emit(VoiceActionRequested(action, text));
        } else if let Some(session) = target {
            self.voice.decision = Some("Jev chose: Quick action · Open session".into());
            // Emit only an exact member of the bounded snapshot, never a model-supplied path.
            cx.emit(VoiceSessionRequested(session, text));
        } else {
            self.input.update(cx, |input, cx| {
                input.append_dictation(&tag::wrap(&text), cx)
            });
        }
        cx.notify();
    }

    /// While Jev routes a finished utterance, show it above the composer as a
    /// prompt that is on its way. Coding-agent routes then land as a normal
    /// prompt card, so the wait is visible from release to send.
    pub(super) fn render_voice_pending(&self) -> Option<gpui::AnyElement> {
        if self.voice.phase != Phase::Routing || self.voice.live_transcript.trim().is_empty() {
            return None;
        }
        let theme = Theme::global();
        let pulse = gpui::Animation::new(Duration::from_millis(1200))
            .repeat()
            .with_easing(gpui::pulsating_between(0.45, 1.0));
        Some(
            div()
                .id("voice-pending-prompt")
                .debug_selector(|| "voice-pending-prompt".into())
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .mx_2()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(theme.prompt_background(0))
                .text_size(px(12.))
                .child(gpui::AnimationExt::with_animation(
                    div()
                        .debug_selector(|| "voice-pending-status".into())
                        .flex_none()
                        .text_color(theme.ACCENT)
                        .child("Jev routing…"),
                    "voice-pending-pulse",
                    pulse,
                    |el, opacity| el.opacity(opacity),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.TEXT_USER)
                        .child(self.voice.live_transcript.clone()),
                )
                .into_any_element(),
        )
    }

    /// The shortcut is in the tooltip, never in place of the microphone.
    pub(super) fn render_voice_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let phase = self.voice.phase;
        let active = phase == Phase::Recording;
        let label = match phase {
            Phase::Idle => "Start voice",
            Phase::Checking => "Cancel microphone connection",
            Phase::Recording => "Stop voice",
            Phase::Transcribing | Phase::Routing => "Cancel voice request",
        };
        let tooltip_status = voice_tooltip(label, &self.status_line());
        let icon_color = if active { theme.BG } else { theme.TEXT_DIM };
        let size = VOICE_BUTTON_SIZE;
        let button = crate::keyboard::button(div().id("voice-toggle"))
            .debug_selector(|| "voice-toggle".into())
            .occlude()
            .relative()
            .flex_none()
            .h(px(size))
            .min_w(px(size))
            .px(px(5.5))
            .rounded_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if active {
                theme.ERROR
            } else {
                theme.prompt_background(usize::MAX)
            })
            .when(!active, |el| el.hover(|el| el.bg(theme.USER_BG)))
            .tooltip(move |_, cx| cx.new(|_| VoiceTooltip(tooltip_status.clone())).into())
            .on_click(cx.listener(|panel, _, window, cx| {
                // All controls use the same action path. Workspace captures
                // this before the panel fallback to stop the existing owner,
                // even if this button belongs to another conversation.
                panel.focus_input(window, cx);
                window.dispatch_action(Box::new(ToggleVoice), cx);
                cx.stop_propagation();
            }))
            .children(
                (crate::fixture_control_targets::voice_recording()
                    || crate::fixture_control_targets::voice_soniox())
                    .then(|| {
                        crate::fixture_control_targets::marker(
                            || "composer.voice".into(),
                            || Some(self.session_id.clone()),
                        )
                    })
                    .flatten(),
            )
            .child({
                let icon = if active {
                    div().absolute().inset_0().flex().items_center().justify_center()
                        .debug_selector(|| "voice-recording-indicator".into())
                        .child(div().debug_selector(|| "voice-stop-icon".into())
                            .size(px(10.)).rounded(px(1.)).bg(icon_color))
                } else {
                    voice_microphone_face(icon_color.into())
                };
                if active {
                    gpui::AnimationExt::with_animation(
                        icon,
                        "voice-recording-pulse",
                        gpui::Animation::new(Duration::from_millis(1200)).repeat().with_easing(gpui::pulsating_between(0.5, 1.0)),
                        |el, opacity| el.opacity(opacity),
                    ).into_any_element()
                } else {
                    icon.into_any_element()
                }
            });
        div()
            .flex_none()
            .h(px(size))
            .child(button)
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "panel_voice_live_tests.rs"]
mod live_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn voice_end_clears_composer_even_when_ask_target_changes(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "voice-ask".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.input.update(cx, |input, cx| input.set_voice_preview(Some(String::new()), cx));
            let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"voice-ask","request_id":"r","tool_call_id":"t","questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}"#).unwrap();
            panel.apply(&event, cx);
            panel.apply_voice_event(NariEvent::Finished(Ok("answer".into())), cx);
            assert!(!panel.voice_active());
            assert!(panel.input.read(cx).voice_preview().is_none(), "composer preview must clear even if the active target changed");
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
    }

    #[gpui::test]
    fn voice_end_after_reconnect_clears_composer(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "voice-reconnect".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.input.update(cx, |input, cx| input.set_voice_preview(Some(String::new()), cx));
            panel.connection_lost("runtime connection closed; reconnecting", cx);
            let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"voice-reconnect","request_id":"r","tool_call_id":"t","questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}"#).unwrap();
            panel.apply(&event, cx);
            panel.apply_voice_event(NariEvent::Finished(Ok("answer".into())), cx);
            assert!(!panel.voice_active());
            assert!(panel.input.read(cx).voice_preview().is_none());
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
    }

    #[gpui::test]
    fn voice_report_retains_actual_answers_and_keeps_dictation_text(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            let text = "maybe switch somewhere";
            panel
                .input
                .update(cx, |input, cx| input.set_content("typed work".into(), cx));
            panel.voice.phase = Phase::Routing;
            panel.voice.live_transcript = text.into();
            panel.voice.trace = Some(VoiceTrace {
                transcript: text.into(),
                candidates: Vec::new(),
                questions: voice_intent::describe_questions(text, &[]).unwrap(),
                answers: Vec::new(),
                audio: None,
                usage: None,
            });
            let attempt = panel.voice.canceled.clone();
            panel.finish_voice_report(
                &attempt,
                Ok(voice_intent::VoiceClassification {
                    intent: VoiceIntent::Dictation,
                    answers: vec![voice_intent::VoiceAnswer {
                        id: "coding_agent".into(),
                        probability: 0.91,
                    }],
                    usage: Some(voice_intent::VoiceUsage {
                        input_tokens: 1200,
                        output_tokens: 7,
                        requests: 1,
                    }),
                }),
                cx,
            );
            let trace = panel.voice.trace.as_ref().unwrap();
            assert_eq!(trace.transcript, text);
            assert_eq!(trace.questions.len(), 6);
            assert_eq!(trace.answers[0].probability, 0.91);
            assert_eq!(trace.usage.unwrap().input_tokens, 1200);
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                format!("typed work\n{}", tag::wrap("maybe switch somewhere"))
            );
            // A duplicate completion cannot replace the evidence or append again.
            panel.finish_voice_report(&attempt, Err(anyhow::anyhow!("late failure")), cx);
            assert_eq!(
                panel.voice.trace.as_ref().unwrap().answers[0].probability,
                0.91
            );
            panel.cancel_voice(true, "cancel button", cx);
            assert!(panel.voice.trace.is_none());
            panel.finish_voice_report(
                &attempt,
                Ok(voice_intent::VoiceClassification {
                    intent: VoiceIntent::CodingAgent,
                    answers: Vec::new(),
                    usage: None,
                }),
                cx,
            );
            assert!(panel.voice.trace.is_none());
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                format!("typed work\n{}", tag::wrap("maybe switch somewhere"))
            );
        });
    }

    #[gpui::test]
    fn voice_report_failure_keeps_questions_without_fabricating_answers(
        cx: &mut gpui::TestAppContext,
    ) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.voice.phase = Phase::Routing;
            panel.voice.live_transcript = "keep this".into();
            panel.voice.trace = Some(VoiceTrace {
                transcript: "keep this".into(),
                candidates: Vec::new(),
                questions: voice_intent::describe_questions("keep this", &[]).unwrap(),
                answers: Vec::new(),
                audio: None,
                usage: None,
            });
            let attempt = panel.voice.canceled.clone();
            panel.finish_voice_report(&attempt, Err(anyhow::anyhow!("provider unavailable")), cx);
            assert_eq!(panel.voice.trace.as_ref().unwrap().questions.len(), 6);
            assert!(panel.voice.trace.as_ref().unwrap().answers.is_empty());
            assert!(
                panel
                    .voice
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("provider unavailable")
            );
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                tag::wrap("keep this")
            );
        });
    }

    #[gpui::test]
    fn voice_coding_agent_sends_only_transcript_once_and_preserves_draft(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| {
            let mut panel = Panel::new("voice-agent".into(), None, None, bridge, cx);
            panel.history_loaded = true;
            panel.status = "idle".into();
            panel
        });
        panel.update(cx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("unfinished typed work".into(), cx)
            });
            panel.resolve_voice_for_test("debug this failure", Ok(VoiceIntent::CodingAgent), cx);
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                "unfinished typed work"
            );
            let attempt = panel.voice.canceled.clone();
            panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { session_id, content, images, .. })
            if session_id == "voice-agent" && *content == tag::wrap("debug this failure") && images.is_empty())
        );
        assert!(
            commands.try_recv().is_err(),
            "a duplicate completion must never send twice"
        );
    }

    #[gpui::test]
    fn voice_routing_shows_pending_prompt_until_sent(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new("voice-pending".into(), None, None, bridge, cx);
            panel.history_loaded = true;
            panel.status = "idle".into();
            panel
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-pending-prompt").is_none());
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Routing;
            panel.voice.live_transcript = "fix the flaky test".into();
            cx.notify();
        });
        vcx.run_until_parked();
        let pending = vcx
            .debug_bounds("voice-pending-prompt")
            .expect("routing shows the utterance as it waits to send");
        let composer = vcx.debug_bounds("prompt-editor").unwrap();
        assert!(
            pending.bottom() <= composer.top(),
            "{pending:?} {composer:?}"
        );
        assert!(commands.try_recv().is_err(), "nothing sent while routing");
        panel.update(vcx, |panel, cx| {
            let attempt = panel.voice.canceled.clone();
            panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-pending-prompt").is_none());
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if *content == tag::wrap("fix the flaky test"))
        );
    }

    #[gpui::test]
    fn voice_coding_agent_steers_while_busy(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| {
            let mut panel = Panel::new("voice-agent".into(), None, None, bridge, cx);
            panel.history_loaded = true;
            panel.status = "running".into();
            panel
        });
        panel.update(cx, |panel, cx| {
            panel.resolve_voice_for_test("then add tests", Ok(VoiceIntent::CodingAgent), cx);
        });
        // Sent while still running, never held for the turn to finish. The
        // harness delivers a Send during an active turn as an urgent steer.
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { content, .. }) if *content == tag::wrap("then add tests"))
        );
    }

    #[gpui::test]
    fn voice_hold_repeat_and_early_release_cancel_only_owned_startup(
        cx: &mut gpui::TestAppContext,
    ) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel
                .input
                .update(cx, |input, cx| input.set_content("keep".into(), cx));
            panel.voice.sessions = Some(Vec::new());
            // Seed the same state as production after its offline-preview guard.
            panel.prepare_voice_attempt(true);
            let attempt = panel.voice.canceled.clone();
            panel.begin_voice_hold(cx);
            panel.begin_voice_hold(cx);
            assert!(Arc::ptr_eq(&attempt, &panel.voice.canceled));
            assert!(panel.voice.phase == Phase::Checking);
            assert!(panel.voice.error.is_none());
            panel.end_voice_hold(cx);
            panel.end_voice_hold(cx);
            assert!(attempt.load(Ordering::SeqCst));
            assert!(panel.voice.phase == Phase::Idle);
            assert!(!panel.voice.hold_capture);
            assert!(panel.voice.sessions.is_some());
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep");

            // Late startup completion cannot resurrect a canceled capture or
            // change a newer attempt. No recording or microphone is constructed.
            panel.finish_voice_startup(&attempt, Err(VoiceError::Cancelled), cx);
            assert!(panel.voice.phase == Phase::Idle);
            assert!(panel.voice.error.is_none());
            panel.prepare_voice_attempt(true);
            let current = panel.voice.canceled.clone();
            panel.finish_voice_startup(&attempt, Err(VoiceError::Network), cx);
            assert!(Arc::ptr_eq(&current, &panel.voice.canceled));
            assert!(panel.voice.phase == Phase::Checking);
            assert!(panel.voice.error.is_none());
            current.store(true, Ordering::SeqCst);
            panel.finish_voice_startup(&current, Err(VoiceError::Network), cx);
            assert!(panel.voice.phase == Phase::Checking);
            assert!(panel.voice.error.is_none());
        });
    }

    #[gpui::test]
    fn voice_hold_does_not_take_over_click_recording_or_routing(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            for phase in [
                Phase::Checking,
                Phase::Recording,
                Phase::Transcribing,
                Phase::Routing,
            ] {
                panel.voice.phase = phase;
                panel.voice.hold_capture = false;
                panel.voice.live_transcript = "existing click request".into();
                let attempt = panel.voice.canceled.clone();
                panel.begin_voice_hold(cx);
                panel.end_voice_hold(cx);
                assert!(panel.voice.phase == phase);
                assert!(Arc::ptr_eq(&attempt, &panel.voice.canceled));
                assert!(!attempt.load(Ordering::SeqCst));
                assert_eq!(panel.voice.live_transcript, "existing click request");
            }
        });
    }

    #[gpui::test]
    fn voice_hold_send_mode_routes_final_transcription_without_touching_typed_draft(
        cx: &mut gpui::TestAppContext,
    ) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.voice.sessions = Some(Vec::new());
            panel.prepare_voice_attempt(true);
            panel.voice.phase = Phase::Recording;
            panel
                .input
                .update(cx, |input, cx| input.set_content("typed draft".into(), cx));
            panel.apply_voice_event(NariEvent::Transcript("fix the bug".into()), cx);
            assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
            assert!(panel.voice.phase == Phase::Recording);
            panel.finish_voice_with_mode(
                Ok("fix the bug".into()),
                jcode_base::protocol::TranscriptMode::Send,
                cx,
            );
            // Jev routing is off: an explicit `send` still goes straight to the agent.
            assert!(panel.voice.phase == Phase::Idle);
            assert!(!panel.voice.hold_capture);
            assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
            assert!(
                panel.voice.decision.is_none(),
                "the transcript shows the send"
            );
            assert!(
                panel.items.iter().any(
                    |item| matches!(item, Item::User(text) if text == &tag::wrap("fix the bug"))
                )
            );
            assert!(panel.voice.live_transcript.is_empty());
            assert!(!panel.voice_active());
        });
    }

    #[gpui::test]
    fn voice_hold_rejects_unsupported_and_offline_panels(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.session_id = "terminal://1".into();
            panel.begin_voice_hold(cx);
            assert!(!panel.voice_active());
            assert!(panel.voice.error.is_none());
            panel.session_id = "preview://chat".into();
            panel.begin_voice_hold(cx);
            assert!(!panel.voice_active());
            assert!(!panel.voice.hold_capture);
            assert!(
                panel
                    .voice
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("offline previews")
            );
            assert!(panel.voice.recording.is_none());
            assert!(panel.voice.task.is_none());
        });
    }

    #[gpui::test]
    fn voice_recording_indicator_clears_when_capture_stops(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| { panel.voice.phase = Phase::Recording; cx.notify(); });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-recording-indicator").is_some());
        assert!(vcx.debug_bounds("voice-stop-icon").is_some());
        assert!(vcx.debug_bounds("voice-microphone-icon").is_none());
        panel.update(vcx, |panel, cx| { panel.voice.phase = Phase::Transcribing; cx.notify(); });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-recording-indicator").is_none());
        assert!(vcx.debug_bounds("voice-stop-icon").is_none());
        assert!(vcx.debug_bounds("voice-microphone-icon").is_some());
    }

    #[gpui::test]
    fn voice_microphone_hover_shows_platform_shortcut(cx: &mut gpui::TestAppContext) {
        let (_panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        vcx.run_until_parked();
        let button = vcx.debug_bounds("voice-toggle").unwrap();
        vcx.update(|window, cx| window.simulate_mouse_move(button.center(), cx));
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-shortcut-tooltip").is_some());
        let text = voice_tooltip("Start voice", "Ready");
        assert!(text.starts_with(&format!("Start voice · {VOICE_SHORTCUT}")));
        assert!(text.contains("Ctrl+Shift+V toggles"));
        assert!(text.contains(&format!(
            "Hold {VOICE_SHORTCUT} to transcribe, release to finish"
        )));
        #[cfg(target_os = "macos")]
        assert!(text.contains("Command+Shift+M") && !text.contains("Copilot"));
        #[cfg(not(target_os = "macos"))]
        assert!(text.contains("Copilot key"));
    }

    #[gpui::test]
    fn voice_routing_non_navigation_retains_draft_and_never_sends(cx: &mut gpui::TestAppContext) {
        for result in [
            Ok(VoiceIntent::Dictation),
            Ok(VoiceIntent::OpenSession("not-offered".into())),
            Err(anyhow::anyhow!("offline")),
        ] {
            let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
            panel.update(cx, |panel, cx| {
                panel
                    .input
                    .update(cx, |input, cx| input.set_content("typed".into(), cx));
                panel.voice.sessions = Some(Vec::new());
                let before = panel.items.len();
                panel.resolve_voice_for_test("spoken words", result, cx);
                assert_eq!(
                    panel.input.read(cx).content.as_ref(),
                    format!("typed\n{}", tag::wrap("spoken words"))
                );
                assert_eq!(panel.items.len(), before);
                assert!(!panel.voice_active());
                assert!(panel.voice.live_transcript.is_empty());
            });
        }
    }

    #[gpui::test]
    fn canceled_and_superseded_jev_results_cannot_edit_the_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.voice.phase = Phase::Routing;
            panel.voice.live_transcript = "discard".into();
            let attempt = panel.voice.canceled.clone();
            panel.cancel_voice(true, "cancel button", cx);
            panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
            assert!(panel.input.read(cx).content.is_empty());
            assert!(panel.items.is_empty());
            panel.voice.phase = Phase::Routing;
            panel.voice.live_transcript = "new recording".into();
            panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
            assert!(panel.input.read(cx).content.is_empty());
            assert!(panel.items.is_empty());
            assert_eq!(panel.voice.live_transcript, "new recording");
            assert!(
                !serde_json::to_string(&panel.snapshot(cx))
                    .unwrap()
                    .contains("new recording")
            );
        });
    }

    #[gpui::test]
    fn jev_navigation_preserves_composer_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("keep my code request".into(), cx)
            });
            let session = serde_json::from_value(serde_json::json!({
                "session_id": "target", "status": "idle", "title": "PDF renderer"
            }))
            .unwrap();
            panel.voice.sessions = Some(vec![session]);
            panel.resolve_voice_for_test(
                "open PDF renderer",
                Ok(VoiceIntent::OpenSession("target".into())),
                cx,
            );
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                "keep my code request"
            );
            assert!(panel.voice.error.is_none());
        });
    }

    #[gpui::test]
    fn voice_streaming_insert_revises_only_the_spoken_span(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.input.update(cx, |input, cx| {
                input.set_content("до после".into(), cx);
                input.set_caret("до ".len(), cx);
            });
            panel.begin_inline_voice(cx);
            panel.apply_voice_event(NariEvent::Transcript("hello wor".into()), cx);
            assert_eq!(panel.input.read(cx).content.as_ref(), "до hello wor после");
            assert_eq!(panel.input.read(cx).snapshot().content, "до после");
        });
        vcx.update(|window, cx| {
            let input = panel.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.set_caret(0, cx);
                gpui::EntityInputHandler::replace_text_in_range(input, None, "🙂 ", window, cx);
                input.set_caret(input.content.len(), cx);
                gpui::EntityInputHandler::replace_text_in_range(input, None, "!", window, cx);
            });
        });
        panel.update(vcx, |panel, cx| {
            panel.apply_voice_event(NariEvent::Transcript("hello earth".into()), cx);
            assert_eq!(panel.input.read(cx).content.as_ref(), "🙂 до hello earth после!");
            panel.apply_voice_event(NariEvent::Finished(Ok("Hello earth.".into())), cx);
            let input = panel.input.read(cx);
            assert_eq!(input.content.as_ref(), "🙂 до Hello earth. после!");
            assert_eq!(input.snapshot().content, "🙂 до Hello earth. после!");
            assert_eq!(input.caret_offset(), input.content.len());
        });
    }

    #[gpui::test]
    fn voice_error_after_speech_keeps_the_words_and_says_why(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.voice.provider = Some("Soniox (stt-rt-v6)".into());
            panel.input.update(cx, |input, cx| {
                input.set_content("до после".into(), cx);
                input.set_caret("до ".len(), cx);
            });
            panel.begin_inline_voice(cx);
            panel.apply_voice_event(NariEvent::Transcript("ninety seconds of words".into()), cx);
            // The provider ended the stream. Nothing but an error reaches the UI.
            panel.apply_voice_event(NariEvent::Finished(Err(VoiceError::Network)), cx);
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                "до ninety seconds of words после",
                "words captured before the failure must stay in the draft"
            );
            let note = panel.voice.error.clone().expect("the failure is reported");
            assert!(note.contains("Soniox (stt-rt-v6)"), "{note}");
            assert!(note.contains("kept"), "{note}");
        });
    }

    #[gpui::test]
    fn voice_streaming_revisions_preserve_typed_draft_and_stay_out_of_snapshots(
        cx: &mut gpui::TestAppContext,
    ) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel
                .input
                .update(cx, |input, cx| input.set_content("typed draft".into(), cx));
            panel.apply_voice_event(NariEvent::Transcript("hello wor".into()), cx);
            panel.apply_voice_event(NariEvent::Transcript("Hello world.".into()), cx);
            assert_eq!(panel.voice.live_transcript, "Hello world.");
            assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
            let snapshot = serde_json::to_string(&panel.snapshot(cx)).unwrap();
            assert!(!snapshot.contains("Hello world"));
            assert!(!snapshot.contains("live_transcript"));
            // The revision is painted in the composer as dim text: never
            // inserted, never part of the persisted snapshot.
            panel.input.read_with(cx, |input, _| {
                assert_eq!(input.voice_preview(), Some("Hello world."));
            });
            let before = panel.items.len();
            // `send` keeps its old contract: exactly one prompt, draft untouched.
            panel.finish_voice_with_mode(
                Ok("Hello world.".into()),
                jcode_base::protocol::TranscriptMode::Send,
                cx,
            );
            assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
            assert_eq!(panel.items.len(), before + 1, "sent exactly once");
            panel.input.read_with(cx, |input, _| {
                assert!(
                    input.voice_preview().is_none(),
                    "final words replace the preview"
                )
            });
            assert!(panel.voice.live_transcript.is_empty());
            assert!(!panel.voice_active());
        });
    }

    #[gpui::test]
    fn voice_cancel_discards_partial_without_changing_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel
                .input
                .update(cx, |input, cx| input.set_content("keep".into(), cx));
            panel.begin_inline_voice(cx);
            panel.apply_voice_event(NariEvent::Transcript("discard me".into()), cx);
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep discard me");
            let token = panel.voice.canceled.clone();
            panel.cancel_voice(true, "cancel button", cx);
            assert!(token.load(Ordering::SeqCst));
            assert!(panel.voice.live_transcript.is_empty());
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep");
        });
    }

    #[gpui::test]
    fn voice_only_targets_chat_composers(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            for id in [
                "session_chat",
                "startup://draft",
                "startup://draft/next",
                "ssh://jcode-cloud/session_remote",
                "ssh://devbox/session_remote",
            ] {
                panel.session_id = id.into();
                assert!(panel.supports_voice(), "{id}");
            }
            for id in [
                "terminal://1",
                "gmail://inbox",
                "settings://machines",
                "desktop://changelog",
                "file:///note.md",
            ] {
                panel.session_id = id.into();
                assert!(!panel.supports_voice(), "{id}");
                panel.toggle_voice(cx);
                assert!(!panel.voice_active());
                assert!(panel.voice.error.is_none());
            }
        });
    }

    /// Partials are visible while recording as dim composer text, and the
    /// draft itself stays empty: nothing is inserted until the recording ends.
    #[gpui::test]
    fn voice_meter_and_dim_composer_partials_show_while_recording(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.apply_voice_event(NariEvent::Transcript("Streaming Nari words".into()), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
        assert!(vcx.debug_bounds("voice-overlay").is_none());
        let stop = vcx.debug_bounds("voice-stop-icon").expect("stop element");
        assert!(stop.size.width > px(0.) && stop.size.width == stop.size.height);
        assert!(vcx.debug_bounds("voice-shortcut").is_none());
        panel.read_with(vcx, |panel, cx| {
            assert!(panel.input.read(cx).content.is_empty());
            assert_eq!(
                panel.input.read(cx).voice_preview(),
                Some("Streaming Nari words")
            );
        });
    }

    #[gpui::test]
    fn voice_shortcut_toggles_focused_composer_without_sending(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::bind_workspace_keys(cx);
            // Reloading the keymap must not dispatch the toggle twice.
            crate::bind_workspace_keys(cx);
        });
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("keep my draft".into(), cx)
            });
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        });
        vcx.simulate_keystrokes("ctrl-shift-v");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            assert!(
                panel
                    .voice
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("offline previews")
            );
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft");
            assert!(panel.voice.recording.is_none());
        });
        for phase in [Phase::Checking, Phase::Transcribing] {
            panel.update(vcx, |panel, cx| {
                panel.voice.error = None;
                panel.voice.phase = phase;
                cx.notify();
            });
            vcx.simulate_keystrokes("ctrl-shift-v");
            panel.read_with(vcx, |panel, cx| {
                assert!(panel.voice.phase == Phase::Idle);
                assert!(panel.voice.error.is_none(), "toggle must fire only once");
                assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft");
            });
        }
    }

    #[gpui::test]
    fn voice_footer_keeps_toggle_and_spacing_at_all_widths(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        for width in [240., 320., 480., 800., 1440.] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(600.)));
            for phase in [
                Phase::Idle,
                Phase::Checking,
                Phase::Recording,
                Phase::Transcribing,
            ] {
                panel.update(vcx, |panel, cx| {
                    panel.model = Some("gpt-6-astra".into());
                    panel.provider = Some("openai".into());
                    panel.voice.phase = phase;
                    cx.notify();
                });
                vcx.run_until_parked();
                let input = vcx.debug_bounds("prompt-input").unwrap();
                let button = vcx.debug_bounds("voice-toggle").unwrap();
                let icon = vcx
                    .debug_bounds(if phase == Phase::Recording {
                        "voice-stop-icon"
                    } else {
                        "voice-microphone-icon"
                    })
                    .unwrap();
                assert!(vcx.debug_bounds("voice-shortcut").is_none());
                // The voice button sits inside the input box, at its right.
                assert!(
                    button.left() >= input.left()
                        && button.right() <= input.right()
                        && button.top() >= input.top()
                        && button.bottom() <= input.bottom(),
                    "voice at {width}: {button:?} outside {input:?}"
                );
                assert!(
                    input.right() - button.right() < px(16.),
                    "voice hugs the input's right edge"
                );
                assert_eq!(button.size.height, px(VOICE_BUTTON_SIZE));
                assert!(icon.left() >= button.left() && icon.right() <= button.right());
                if phase == Phase::Idle {
                    assert!(
                        vcx.debug_bounds("voice-ready-status").is_none(),
                        "idle Ready status is hidden"
                    );
                }
            }
        }
    }

    #[gpui::test]
    fn voice_controls_render_and_cancel_without_network(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        vcx.simulate_window_resize(handle, gpui::size(px(240.), px(320.)));
        vcx.run_until_parked();
        let button = vcx
            .debug_bounds("voice-toggle")
            .expect("voice button in fresh composer");
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, _| {
            assert!(
                panel
                    .voice
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("offline previews")
            );
            assert!(panel.voice.task.is_none());
        });
        let dismiss = vcx.debug_bounds("voice-cancel").expect("dismiss error");
        vcx.simulate_click(dismiss.center(), gpui::Modifiers::default());
        for phase in [Phase::Checking, Phase::Recording, Phase::Transcribing] {
            panel.update(vcx, |panel, cx| {
                panel
                    .items
                    .push(Item::Assistant("Existing conversation".into()));
                panel.voice.phase = phase;
                panel.voice.started = Some(Instant::now());
                cx.notify();
            });
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("voice-toggle").is_some());
            if phase == Phase::Recording {
                assert!(vcx.debug_bounds("voice-cancel").is_none());
                assert!(vcx.debug_bounds("voice-stop").is_none());
                panel.update(vcx, |panel, cx| panel.cancel_voice(true, "test", cx));
                continue;
            }
            let cancel = vcx
                .debug_bounds("voice-cancel")
                .expect("cancel visible during work");
            vcx.simulate_click(cancel.center(), gpui::Modifiers::default());
            panel.read_with(vcx, |panel, _| assert!(panel.voice.phase == Phase::Idle));
        }
    }

    #[gpui::test]
    fn voice_insertion_is_undoable(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel
                .input
                .update(cx, |input, cx| input.set_content("original".into(), cx));
            panel.finish_voice_with_mode(
                Ok("dictated".into()),
                jcode_base::protocol::TranscriptMode::Insert,
                cx,
            );
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        });
        vcx.simulate_keystrokes("ctrl-z");
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(panel.input.read(cx).content.as_ref(), "original")
        });
    }

    /// `[dictation] mode = "insert"` (the default): the words land at the
    /// caret and nothing is sent, so the user edits and sends them.
    #[gpui::test]
    fn voice_insert_mode_lands_at_the_caret_and_never_sends(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| {
            Panel::new(
                "voice-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("fix the test".into(), cx);
                input.set_caret("fix ".len(), cx);
            });
            let items = panel.items.len();
            panel.finish_voice_with_mode(
                Ok("flaky ".into()),
                jcode_base::protocol::TranscriptMode::Insert,
                cx,
            );
            panel.input.read_with(cx, |input, _| {
                assert_eq!(input.content.as_ref(), "fix flaky the test");
                assert_eq!(input.caret_offset(), "fix flaky ".len());
            });
            assert_eq!(panel.items.len(), items, "insert mode must not send");
            assert!(panel.voice.error.is_none());
            assert_eq!(panel.voice.phase, Phase::Idle);
            assert!(panel.voice.live_transcript.is_empty());
        });
    }

    /// `append` and `replace` also stay in the draft: they never submit.
    #[gpui::test]
    fn voice_append_and_replace_modes_stay_in_the_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| {
            Panel::new(
                "voice-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel
                .input
                .update(cx, |input, cx| input.set_content("draft".into(), cx));
            let items = panel.items.len();
            panel.finish_voice_with_mode(
                Ok("appended".into()),
                jcode_base::protocol::TranscriptMode::Append,
                cx,
            );
            panel.input.read_with(cx, |input, _| {
                assert!(input.content.contains("appended"), "{}", input.content)
            });
            panel.finish_voice_with_mode(
                Ok("replaced".into()),
                jcode_base::protocol::TranscriptMode::Replace,
                cx,
            );
            panel.input.read_with(cx, |input, _| {
                assert!(input.content.contains("replaced"), "{}", input.content);
                assert!(!input.content.contains("draft"), "{}", input.content);
            });
            assert_eq!(panel.items.len(), items, "draft modes must not send");
        });
    }

    #[gpui::test]
    fn focused_ask_comment_receives_shortcut_voice_and_recording_indicator(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("voice-ask", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0)).unwrap();
        let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"voice-ask","request_id":"r","tool_call_id":"t","questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}"#).unwrap();
        panel.update(vcx, |panel, cx| { panel.test_settled_conversation(1, cx); panel.apply(&event, cx); });
        vcx.run_until_parked();
        vcx.update(|window, cx| window.focus(&panel.read(cx).focus_handle.clone(), cx));
        vcx.simulate_keystrokes("2 n");
        vcx.run_until_parked();
        vcx.update(|window, cx| window.dispatch_action(Box::new(ToggleVoice), cx));
        panel.update(vcx, |panel, cx| {
            panel.voice.phase = Phase::Recording;
            panel.apply_voice_event(NariEvent::Transcript("partial".into()), cx);
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.comment_inputs[0][1].read(cx).voice_preview(), Some("partial"));
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("ask-voice-recording-indicator-0-2").is_some());
        panel.update(vcx, |panel, cx| panel.finish_voice_with_mode(Ok("final words".into()), jcode_base::protocol::TranscriptMode::Insert, cx));
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.comment_inputs[0][1].read(cx).snapshot().content, "final words");
            assert!(card.other_inputs[0].read(cx).snapshot().content.is_empty());
        });
    }

    #[gpui::test]
    fn global_voice_to_focused_free_answer_requires_permission(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
            workspace.push_test_panel("voice-ask", cx);
            workspace
        });
        let panel = workspace.read_with(vcx, |workspace, _| workspace.test_panel(0)).unwrap();
        let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"voice-ask","request_id":"r","tool_call_id":"t","questions":[{"question":"Which?","options":[{"label":"A"}]}]}"#).unwrap();
        panel.update(vcx, |panel, cx| { panel.test_settled_conversation(1, cx); panel.apply(&event, cx); });
        vcx.run_until_parked();
        vcx.update(|window, cx| {
            let input = panel.read(cx).ask_card.as_ref().unwrap().other_inputs[0].clone();
            window.focus(&input.read(cx).focus_handle.clone(), cx);
            panel.update(cx, |panel, cx| assert!(panel.capture_focused_ask_voice_target(window, cx)));
        });
        panel.update(vcx, |panel, cx| {
            panel.prepare_voice_attempt(true);
            panel.voice.global_capture = true;
            panel.voice.phase = Phase::Transcribing;
            let attempt = panel.voice.canceled.clone();
            panel.finish_global_voice_text(&attempt, "denied".into(), None, false, jcode_base::protocol::TranscriptMode::Insert, cx);
            assert!(panel.ask_voice_input().unwrap().read(cx).snapshot().content.is_empty());
            panel.prepare_voice_attempt(true);
            panel.voice.global_capture = true;
            panel.voice.phase = Phase::Transcribing;
            let attempt = panel.voice.canceled.clone();
            panel.finish_global_voice_text(&attempt, "answer".into(), None, true, jcode_base::protocol::TranscriptMode::Insert, cx);
        });
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| assert_eq!(panel.ask_voice_input().unwrap().read(cx).snapshot().content, "answer"));
    }

    #[gpui::test]
    fn ask_dictation_in_send_mode_edits_answer_without_sending_chat(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let panel = cx.new(|cx| Panel::new("voice-ask".into(), None, None, bridge, cx));
        let event: jcode_sdk::ApiEvent = serde_json::from_str(r#"{"ev":"ask_request","session_id":"voice-ask","request_id":"r","tool_call_id":"t","questions":[{"question":"Which?","options":[{"label":"A"},{"label":"B"}]}]}"#).unwrap();
        panel.update(cx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("original draft".into(), cx)
            });
            panel.apply(&event, cx);
            panel.finish_voice_with_mode(
                Ok("dictated answer".into()),
                jcode_base::protocol::TranscriptMode::Send,
                cx,
            );
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, cx| {
            let card = panel.ask_card.as_ref().unwrap();
            assert_eq!(card.other[0].as_deref(), Some("dictated answer"));
            assert_eq!(panel.input.read(cx).snapshot().content, "original draft");
        });
        assert!(
            commands
                .try_iter()
                .all(|command| !matches!(command, Command::Send { .. }))
        );
    }

    #[gpui::test]
    fn voice_sends_transcript_without_touching_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| {
            Panel::new(
                "voice-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            panel.load_history(vec![], vec![], cx);
            panel.input.update(cx, |input, cx| {
                input.set_content("Typed while transcribing".into(), cx)
            });
            let items = panel.items.len();
            panel.finish_voice_with_mode(
                Ok("  dictated words  ".into()),
                jcode_base::protocol::TranscriptMode::Send,
                cx,
            );
            assert_eq!(
                panel.input.read(cx).snapshot().content,
                "Typed while transcribing"
            );
            assert!(
                panel.items.iter().skip(items).any(
                    |item| matches!(item, Item::User(text) if text == &tag::wrap("dictated words"))
                ),
                "voice sends only the trimmed utterance"
            );
            let items = panel.items.len();
            panel.finish_voice_with_mode(
                Err(VoiceError::Network),
                jcode_base::protocol::TranscriptMode::Send,
                cx,
            );
            assert_eq!(
                panel.input.read(cx).snapshot().content,
                "Typed while transcribing"
            );
            assert_eq!(panel.items.len(), items);
            assert!(panel.voice.error.is_some());
        });
    }

    #[gpui::test]
    fn voice_cancel_invalidates_pending_work_and_preserves_draft(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| {
            Panel::new(
                "voice-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(cx, |panel, cx| {
            let token = panel.voice.canceled.clone();
            panel.voice.phase = Phase::Checking;
            panel
                .input
                .update(cx, |input, cx| input.set_content("keep me".into(), cx));
            panel.cancel_voice(true, "cancel button", cx);
            assert!(token.load(Ordering::SeqCst));
            assert!(!Arc::ptr_eq(&token, &panel.voice.canceled));
            assert_eq!(panel.input.read(cx).snapshot().content, "keep me");
            assert!(panel.voice.phase == Phase::Idle);
        });
    }

    #[gpui::test]
    fn voice_preview_never_opens_microphone(cx: &mut gpui::TestAppContext) {
        let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(cx, |panel, cx| {
            panel.start_voice(cx);
            assert!(panel.voice.phase == Phase::Idle);
            assert!(panel.voice.recording.is_none());
            assert!(panel.voice.task.is_none());
            assert!(
                panel
                    .voice
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("offline previews")
            );
        });
    }

    /// `[dictation] mode = "replace"` discards a draft, so the discarded draft
    /// must survive as one undo step. A draft restored after a restart has an
    /// empty undo stack, so a replacement that merely overwrites the content
    /// loses the draft for good.
    #[gpui::test]
    fn voice_replacement_is_one_undoable_edit(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel
                .input
                .update(cx, |input, cx| input.set_content("original".into(), cx));
            panel.finish_voice_with_mode(
                Ok("dictated".into()),
                jcode_base::protocol::TranscriptMode::Replace,
                cx,
            );
            panel.input.read_with(cx, |input, _| {
                assert!(!input.content.contains("original"), "{}", input.content);
                assert!(input.content.contains("dictated"), "{}", input.content);
            });
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        });
        vcx.simulate_keystrokes("ctrl-z");
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                "original",
                "one undo restores the replaced draft"
            )
        });
    }

    /// `/dictate` from a command uses the same replacement rule.
    #[gpui::test]
    fn command_transcript_replacement_is_one_undoable_edit(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        panel.update(vcx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("half-typed draft".into(), cx)
            });
            panel.apply_command_transcript(
                &jcode_voice::dictation::DictationRun {
                    text: "spoken words".into(),
                    mode: jcode_base::protocol::TranscriptMode::Replace,
                },
                cx,
            );
            panel.input.read_with(cx, |input, _| {
                assert!(
                    !input.content.contains("half-typed draft"),
                    "{}",
                    input.content
                );
                assert!(input.content.contains("spoken words"), "{}", input.content);
            });
        });
        vcx.update(|window, cx| {
            let focus = panel.read(cx).input.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        });
        vcx.simulate_keystrokes("ctrl-z");
        panel.read_with(vcx, |panel, cx| {
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                "half-typed draft",
                "one undo restores the replaced draft"
            )
        });
    }
}
