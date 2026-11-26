//! The state of one panel's dictation, and every legal way it changes.
//!
//! A panel dictates at most one utterance at a time. `VoiceState` is that
//! utterance's lifecycle (connecting, recording, transcribing, routing), the
//! outcome the panel shows afterwards (an error, Jev's decision, the routing
//! evidence), and the routing wiring the workspace installs for the panel.
//! Nothing outside this module sees how those are kept: the panel asks
//! questions (`phase`, `error`, `levels`) and reports events (`begin`,
//! `recording_started`, `receive`, `end`, `resolve_route`), and the state
//! decides what each event means. Effects on the composer, tasks and the
//! window stay in `panel::voice`, which only calls this interface.
//!
//! Audio and interim text are never snapshotted.
use super::voice_trace::VoiceTrace;
use gpui::{Subscription, Task};
use jcode_base::voice_intent::{self, SessionCandidate, VoiceIntent};
use jcode_desktop_voice::chrome::{global_pill_decision, global_pill_error};
use jcode_desktop_voice::meter::{BARS, LevelMeter};
use jcode_voice::{self as voice, NariEvent, NariRecording, VoiceError};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Jev voice routing is temporarily off. Every finished utterance is sent
/// straight to its panel's agent. Planned return: Jev chooses between the
/// focused panel and one of the other open sessions (see docs/jev-actions.md).
pub(super) const JEV_ROUTING: bool = false;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
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

/// Timestamped voice lifecycle line in the Desktop log. Never transcript text.
pub(super) fn voice_log(message: std::fmt::Arguments) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    eprintln!(
        "[unix {}.{:03}] voice: {message}",
        now.as_secs(),
        now.subsec_millis()
    );
}

/// What a recording stop request reached.
pub(super) enum Stop {
    /// The streaming recording was told to finish.
    Recorder,
    /// A managed SuperWhisper capture, which the panel must stop externally.
    Bridge,
    /// Nothing was recording.
    NotRunning,
}

/// What one provider event means for the panel.
pub(super) enum Received {
    /// The interim words changed. Read them with `transcript`.
    Words,
    /// The provider is done, with the final words or why there are none.
    Finished(Result<String, VoiceError>),
    /// Nothing for the panel to do.
    Ignored,
}

/// The recording that just ended.
pub(super) struct Ended {
    /// Audio length at stop, so final transcription latency is not billed as audio.
    pub(super) audio: Option<Duration>,
    /// The words were being revised in the draft while spoken.
    pub(super) inline: bool,
}

/// Where Jev sent a finished utterance.
pub(super) enum Route {
    /// Send only this utterance to the panel's agent.
    CodingAgent(String),
    QuickAction(voice_intent::QuickAction, String),
    /// An exact member of the bounded session snapshot, never a model-supplied path.
    Session(jcode_sdk::SessionInfo, String),
    /// Keep the words in the draft.
    Draft(String),
}

/// What a panel shows after an attempt.
pub(super) enum Outcome {
    Error(String),
    Decision(String),
}

/// What Jev needs to route a panel's utterances, installed by the workspace.
pub(super) struct Routing {
    /// The sessions Jev may open: a bounded snapshot, most recent first.
    pub(super) sessions: Vec<jcode_sdk::SessionInfo>,
    /// Carries a chosen session to the workspace.
    pub(super) navigation: Subscription,
    /// Carries a chosen quick action to the workspace.
    pub(super) actions: Subscription,
}

/// A canned state for offline previews and screenshots.
pub(super) enum Preview {
    TimedOut,
    /// Jev is still choosing.
    Routing(VoiceTrace),
    /// Jev chose, and `decision` says what.
    Routed {
        trace: VoiceTrace,
        decision: String,
    },
    Connecting,
    Recording,
    Listening,
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
    meter: LevelMeter,
    /// Set when this attempt is over. Tasks hold a clone to notice.
    canceled: Arc<AtomicBool>,
    started: Option<Instant>,
    // Audio length at stop, so final transcription latency is not billed as audio.
    audio: Option<Duration>,
    error: Option<String>,
    task: Option<Task<()>>,
    timer: Option<Task<()>>,
    sessions: Option<Vec<jcode_sdk::SessionInfo>>,
    navigation: Option<Subscription>,
    actions: Option<Subscription>,
    decision: Option<String>,
    trace: Option<VoiceTrace>,
    trace_expanded: bool,
    trace_preview: bool,
    /// Why stop was requested, for the end-of-recording log. `None` at the
    /// end means the recording ended on its own (duration cap or provider).
    stop_reason: Option<&'static str>,
    /// The end of this attempt was already logged.
    end_logged: bool,
    /// Provider and model this attempt started, for messages and the log.
    provider: Option<String>,
    /// Why the provider stream stopped early. The words so far are kept.
    interrupted: Option<VoiceError>,
}

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

// What the panel asks.
impl VoiceState {
    pub(super) fn phase(&self) -> Phase {
        self.phase
    }

    pub(super) fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub(super) fn is_hold(&self) -> bool {
        self.hold_capture
    }

    pub(super) fn is_global(&self) -> bool {
        self.global_capture
    }

    /// The words are being revised in the draft as they are spoken.
    pub(super) fn is_inline(&self) -> bool {
        self.inline_draft
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn decision(&self) -> Option<&str> {
        self.decision.as_deref()
    }

    pub(super) fn has_trace(&self) -> bool {
        self.trace.is_some()
    }

    /// A copy of the routing evidence, for a card to draw or a workspace to
    /// carry to another panel.
    pub(super) fn trace_card(&self) -> Option<VoiceTrace> {
        self.trace.clone()
    }

    pub(super) fn trace_expanded(&self) -> bool {
        self.trace_expanded
    }

    /// The trace is an offline preview, so the card says so.
    pub(super) fn trace_preview(&self) -> bool {
        self.trace_preview
    }

    pub(super) fn levels(&self) -> [f32; BARS] {
        self.meter.levels()
    }

    /// The interim words while recording, the final ones while routing.
    pub(super) fn transcript(&self) -> &str {
        &self.live_transcript
    }

    /// This attempt's identity. Background work keeps it and presents it back
    /// to learn whether the attempt it started for is still the current one.
    pub(super) fn attempt(&self) -> Arc<AtomicBool> {
        self.canceled.clone()
    }

    pub(super) fn owns(&self, attempt: &Arc<AtomicBool>) -> bool {
        Arc::ptr_eq(attempt, &self.canceled)
    }

    /// `attempt` is the current one, was not canceled, and is in one of `phases`.
    pub(super) fn is_live(&self, attempt: &Arc<AtomicBool>, phases: &[Phase]) -> bool {
        self.owns(attempt) && !attempt.load(Ordering::SeqCst) && phases.contains(&self.phase)
    }

    /// A finished utterance has somewhere to go besides the draft.
    pub(super) fn can_route(&self) -> bool {
        self.sessions.is_some() || !JEV_ROUTING
    }

    /// What the OS pill shows for an unfocused hold, or `None` when `attempt`
    /// is not the current global hold.
    pub(super) fn global_snapshot(
        &self,
        attempt: &Arc<AtomicBool>,
    ) -> Option<crate::global_voice_overlay::Snapshot> {
        if !self.owns(attempt) || !self.global_capture {
            return None;
        }
        let (title, decided) = match self.phase {
            Phase::Idle => match (&self.error, &self.decision) {
                (Some(error), _) => (global_pill_error(error), true),
                (None, Some(decision)) => (global_pill_decision(decision), true),
                (None, None) => ("Added to draft".into(), true),
            },
            Phase::Checking => ("Connecting…".into(), false),
            Phase::Recording => ("Listening".into(), false),
            Phase::Transcribing => ("Transcribing…".into(), false),
            Phase::Routing => ("Jev is choosing…".into(), false),
        };
        Some(crate::global_voice_overlay::Snapshot {
            title,
            levels: (self.phase == Phase::Recording).then(|| self.meter.levels()),
            decided,
        })
    }

    /// The provider's event signal, to wake as soon as a transcript arrives.
    pub(super) fn event_ready(
        &self,
    ) -> Option<impl std::future::Future<Output = ()> + Send + 'static + use<>> {
        self.recording.as_ref().map(NariRecording::event_ready)
    }

    /// The next provider event, if any is waiting.
    pub(super) fn next_event(&self) -> Option<NariEvent> {
        self.recording.as_ref().and_then(NariRecording::try_event)
    }

    /// The capture worker exited. Events it published last may still be waiting.
    pub(super) fn capture_finished(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(NariRecording::is_finished)
    }
}

// A new attempt, and what ends it.
impl VoiceState {
    /// Drop everything about the last attempt, keeping the routing wiring.
    /// Ending paths log explicitly. Only an unlogged drop reports itself.
    pub(super) fn reset(&mut self) {
        self.phase = Phase::Idle;
        // Session configuration belongs to the panel, not an individual capture.
        let sessions = self.sessions.take();
        let navigation = self.navigation.take();
        let actions = self.actions.take();
        *self = VoiceState::default();
        self.actions = actions;
        self.sessions = sessions;
        self.navigation = navigation;
    }

    /// Start connecting a new attempt. `hold` follows a held key, `global`
    /// a hold made while another window is focused.
    pub(super) fn begin(&mut self, hold: bool, global: bool) {
        self.reset();
        self.phase = Phase::Checking;
        self.hold_capture = hold;
        self.global_capture = global;
    }

    /// The attempt is canceled. `normal` marks an intentional user cancel,
    /// everything else is logged as ABNORMAL so unexpected stops can be found.
    /// Says whether the words were being revised in the draft.
    pub(super) fn cancel(&mut self, normal: bool, reason: &str) -> bool {
        if self.phase != Phase::Idle {
            self.log_end(normal, &format!("canceled: {reason}"), None);
        }
        let inline = self.inline_draft;
        self.reset();
        inline
    }

    /// The panel's own window is closing a finished or failed attempt.
    pub(super) fn fail(&mut self, message: String) {
        self.phase = Phase::Idle;
        self.hold_capture = false;
        self.error = Some(message);
    }

    /// The provider could not start. Names the backend and model that failed.
    pub(super) fn startup_failed(&mut self, error: &VoiceError) {
        self.log_end(false, &format!("failed to start: {error:?}"), None);
        if self.global_capture {
            eprintln!("global voice: recording failed to start: {error}");
        }
        self.phase = Phase::Idle;
        self.hold_capture = false;
        self.error = Some(format!("{}: {error}", voice::configured_label()));
    }

    /// An external dictation command owns the microphone. A managed
    /// SuperWhisper bridge records until stopped, any other command is
    /// already waiting for its transcript.
    pub(super) fn command_started(&mut self, superwhisper: bool, task: Task<()>) {
        voice_log(format_args!("external dictation command started"));
        self.superwhisper_capture = superwhisper;
        self.phase = if superwhisper {
            Phase::Recording
        } else {
            Phase::Transcribing
        };
        self.started = Some(Instant::now());
        self.task = Some(task);
    }

    /// The microphone is open and the provider connected. `timer` drains its
    /// events. `inline` revises the words in the draft as they are spoken.
    pub(super) fn recording_started(
        &mut self,
        recording: NariRecording,
        timer: Task<()>,
        provider: String,
        inline: bool,
    ) {
        voice_log(format_args!(
            "recording started with {provider} ({}{})",
            if self.hold_capture { "hold" } else { "toggle" },
            if self.global_capture { ", global" } else { "" },
        ));
        jcode_voice::timing::mark("ui shows recording");
        if self.global_capture {
            eprintln!("global voice: recording started");
        }
        self.provider = Some(provider);
        self.phase = Phase::Recording;
        self.recording = Some(recording);
        self.started = Some(Instant::now());
        self.inline_draft = inline;
        self.timer = Some(timer);
    }

    /// Sample the microphone level for the meter.
    pub(super) fn sample_meter(&mut self) {
        if self.phase == Phase::Recording {
            let level = self
                .recording
                .as_ref()
                .map_or(0., NariRecording::audio_level);
            self.meter.push(level);
        }
    }

    /// Take in one provider event.
    pub(super) fn receive(&mut self, event: NariEvent) -> Received {
        match event {
            NariEvent::Started => Received::Ignored,
            NariEvent::Transcript(text) => {
                self.live_transcript = text;
                Received::Words
            }
            NariEvent::Interrupted(error) => {
                self.interrupted = Some(error);
                Received::Ignored
            }
            NariEvent::Finished(result) => Received::Finished(result),
        }
    }

    /// Ask the recording to finish. The audio length is fixed here, so the
    /// wait for the final transcript is not billed as audio.
    pub(super) fn begin_stop(&mut self, reason: &'static str) -> Stop {
        let how = if let Some(recording) = self.recording.as_ref() {
            recording.stop();
            Stop::Recorder
        } else if self.superwhisper_capture {
            Stop::Bridge
        } else {
            return Stop::NotRunning;
        };
        jcode_voice::timing::release();
        self.stop_reason = Some(reason);
        self.audio = self.started.map(|started| started.elapsed());
        voice_log(format_args!(
            "stop requested: {reason} after {:.1}s",
            self.audio.unwrap_or_default().as_secs_f32()
        ));
        self.phase = Phase::Transcribing;
        how
    }

    /// Log the end of the attempt as abnormal, with `outcome` as the reason.
    pub(super) fn log_abnormal(&mut self, outcome: &str) {
        self.log_end(false, outcome, None);
    }

    /// The external stop did not reach SuperWhisper, so it is still recording.
    pub(super) fn stop_failed(&mut self, attempt: &Arc<AtomicBool>, message: String) -> bool {
        if !self.owns(attempt) || self.phase != Phase::Transcribing {
            return false;
        }
        self.phase = Phase::Recording;
        self.audio = None;
        self.stop_reason = None;
        self.error = Some(message);
        true
    }

    /// Record why the recording ended, unless a stop request already said.
    pub(super) fn note_stop(&mut self, reason: &'static str) {
        self.stop_reason.get_or_insert(reason);
    }

    /// Words already transcribed outlive any error: a limit or failure never
    /// discards them. Returns the result to deliver and, when the provider
    /// stopped early, the note to show next to the kept text.
    pub(super) fn salvage(
        &mut self,
        result: Result<String, VoiceError>,
    ) -> (Result<String, VoiceError>, Option<String>) {
        let result = match result {
            Err(error)
                if error != VoiceError::Cancelled && !self.live_transcript.trim().is_empty() =>
            {
                self.interrupted.get_or_insert(error);
                Ok(self.live_transcript.clone())
            }
            other => other,
        };
        let note = match (&result, self.interrupted.take()) {
            (Ok(text), Some(error)) if !text.trim().is_empty() => Some(format!(
                "{} stopped early: {error}. The words captured before that were kept.",
                self.provider.as_deref().unwrap_or("Voice input")
            )),
            _ => None,
        };
        (result, note)
    }

    /// The recording is over. Logs how it ended and releases the microphone.
    pub(super) fn end(&mut self, result: &Result<String, VoiceError>) -> Ended {
        match result {
            Ok(text) if text.trim().is_empty() => self.log_end(
                self.stop_reason.is_some(),
                "no speech in transcript (silence or filtered prompt echo)",
                Some(0),
            ),
            Ok(text) if self.stop_reason.is_some() => {
                self.log_end(true, "transcribed", Some(text.chars().count()))
            }
            Ok(text) => self.log_end(
                false,
                "recording ended without a stop request (safety cap or provider)",
                Some(text.chars().count()),
            ),
            Err(error) => self.log_end(false, &format!("error: {error:?}"), None),
        }
        self.hold_capture = false;
        self.phase = Phase::Idle;
        let audio = self
            .audio
            .take()
            .or_else(|| self.started.map(|started| started.elapsed()));
        self.started = None;
        self.recording = None;
        self.superwhisper_capture = false;
        self.live_transcript.clear();
        Ended {
            audio,
            inline: std::mem::take(&mut self.inline_draft),
        }
    }

    /// The words are delivered and nothing is left to show.
    pub(super) fn settle(&mut self) {
        self.phase = Phase::Idle;
        self.error = None;
    }

    /// The words are delivered and `decision` tells the OS pill where.
    pub(super) fn settle_with_decision(&mut self, decision: &str) {
        self.settle();
        self.decision = Some(decision.into());
    }

    /// The words are delivered. A focused hold shows them where they landed,
    /// so only an unfocused one keeps `label` as its confirmation.
    pub(super) fn conclude(&mut self, label: &str) {
        self.phase = Phase::Idle;
        self.error = None;
        self.trace = None;
        self.live_transcript.clear();
        self.decision = self.global_capture.then(|| label.into());
    }

    /// Show why the provider stopped early next to the kept words, unless
    /// something more specific is already shown.
    pub(super) fn note_interruption(&mut self, note: String) -> bool {
        voice_log(format_args!("kept transcript after interruption: {note}"));
        if self.phase == Phase::Idle && self.error.is_none() {
            self.error = Some(note);
            return true;
        }
        false
    }

    /// The unfocused hold is waiting on the logind session check.
    pub(super) fn await_session_check(&mut self, task: Task<()>) {
        self.phase = Phase::Transcribing;
        self.task = Some(task);
    }

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
            self.meter.peak(),
            chars.map_or("-".into(), |chars| chars.to_string()),
            if self.hold_capture { "hold" } else { "toggle" },
            if self.global_capture { ", global" } else { "" },
        ));
    }
}

// Routing a finished utterance through Jev.
impl VoiceState {
    /// Install the sessions Jev may open and the subscriptions that carry its
    /// choices to the workspace. They belong to the panel, not one attempt.
    pub(super) fn wire(&mut self, routing: Routing) {
        self.sessions = Some(routing.sessions);
        self.navigation = Some(routing.navigation);
        self.actions = Some(routing.actions);
    }

    /// Jev starts choosing for `text`. Returns the sessions it may pick from.
    pub(super) fn begin_routing(&mut self, text: &str) -> Vec<SessionCandidate> {
        self.phase = Phase::Routing;
        self.live_transcript = text.into();
        self.error = None;
        self.sessions
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
            .collect()
    }

    /// The questions went out. `task` collects Jev's answers.
    pub(super) fn routing_started(
        &mut self,
        candidates: Vec<SessionCandidate>,
        questions: Vec<voice_intent::VoiceQuestion>,
        audio: Option<Duration>,
        task: Task<()>,
    ) {
        self.trace = Some(VoiceTrace {
            transcript: self.live_transcript.clone(),
            candidates,
            questions,
            answers: Vec::new(),
            audio,
            usage: None,
        });
        self.task = Some(task);
    }

    /// Keep Jev's answers and usage as evidence, and pass its choice on.
    pub(super) fn record_report(
        &mut self,
        report: anyhow::Result<voice_intent::VoiceClassification>,
    ) -> anyhow::Result<VoiceIntent> {
        report.map(|report| {
            if let Some(trace) = &mut self.trace {
                trace.answers = report.answers;
                trace.usage = report.usage;
            }
            report.intent
        })
    }

    /// Jev chose. Shows what it chose and says where the words go.
    pub(super) fn resolve_route(&mut self, result: anyhow::Result<VoiceIntent>) -> Route {
        self.phase = Phase::Idle;
        let mut target = None;
        let mut quick_action = None;
        let mut coding_agent = false;
        let error = match result {
            Ok(VoiceIntent::CodingAgent) => {
                coding_agent = true;
                None
            }
            Ok(VoiceIntent::QuickAction(action)) => {
                if self.actions.is_some() {
                    quick_action = Some(action);
                    None
                } else {
                    Some("Jev chose: Quick action · Navigation unavailable. Transcript kept in the draft.".into())
                }
            }
            Ok(VoiceIntent::Dictation) => None,
            Ok(VoiceIntent::OpenSession(id)) => {
                target = self
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
        let text = std::mem::take(&mut self.live_transcript);
        self.error = error;
        if coding_agent {
            self.decision = Some("Jev chose: Coding agent · Sent now".into());
            Route::CodingAgent(text)
        } else if let Some(action) = quick_action {
            self.decision = Some("Jev chose: Quick action · Navigation".into());
            Route::QuickAction(action, text)
        } else if let Some(session) = target {
            self.decision = Some("Jev chose: Quick action · Open session".into());
            Route::Session(session, text)
        } else {
            Route::Draft(text)
        }
    }

    /// Show `outcome`, replacing whichever kind was shown before.
    pub(super) fn show(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Error(message) => {
                self.decision = None;
                self.error = Some(message);
            }
            Outcome::Decision(message) => {
                self.error = None;
                self.decision = Some(message);
            }
        }
    }

    /// Refine the decision shown by an unfocused hold's OS pill. Focused
    /// holds already show it on the destination panel.
    pub(super) fn refine_decision(&mut self, message: String) -> bool {
        if self.global_capture && self.error.is_none() {
            self.decision = Some(message);
            return true;
        }
        false
    }

    pub(super) fn show_trace(&mut self, trace: Option<VoiceTrace>) {
        self.trace = trace;
        self.trace_expanded = false;
        self.trace_preview = false;
    }

    pub(super) fn toggle_trace_details(&mut self) {
        self.trace_expanded = !self.trace_expanded;
    }

    /// Replace the state with a canned one, as offline previews need.
    pub(super) fn preview(&mut self, scene: Preview) {
        *self = VoiceState::default();
        match scene {
            Preview::TimedOut => {
                self.error = Some("Voice transcription request timed out".into());
            }
            Preview::Routing(trace) => {
                self.trace = Some(trace);
                self.trace_preview = true;
                self.phase = Phase::Routing;
            }
            Preview::Routed { trace, decision } => {
                self.trace = Some(trace);
                self.trace_preview = true;
                self.decision = Some(decision);
            }
            Preview::Connecting => {
                self.hold_capture = true;
                self.phase = Phase::Checking;
            }
            Preview::Recording => {
                self.hold_capture = true;
                self.phase = Phase::Recording;
            }
            Preview::Listening => {
                self.hold_capture = true;
                self.phase = Phase::Recording;
                self.started = Some(Instant::now());
                self.meter = LevelMeter::preview();
            }
        }
    }

    /// A recording whose words are revised in the draft, for screenshots.
    pub(super) fn fixture_inline_recording(&mut self) {
        self.phase = Phase::Recording;
        self.inline_draft = true;
    }
}

// Seams for tests, which stage states the real events reach only through
// a microphone and a provider.
#[cfg(test)]
impl VoiceState {
    pub(super) fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    pub(super) fn set_hold(&mut self, hold: bool) {
        self.hold_capture = hold;
    }

    pub(super) fn set_global(&mut self, global: bool) {
        self.global_capture = global;
    }

    pub(super) fn set_transcript(&mut self, text: &str) {
        self.live_transcript = text.into();
    }

    pub(super) fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    pub(super) fn set_decision(&mut self, decision: Option<String>) {
        self.decision = decision;
    }

    pub(super) fn set_trace(&mut self, trace: Option<VoiceTrace>) {
        self.trace = trace;
    }

    pub(super) fn trace_mut(&mut self) -> Option<&mut VoiceTrace> {
        self.trace.as_mut()
    }

    pub(super) fn set_trace_expanded(&mut self, expanded: bool) {
        self.trace_expanded = expanded;
    }

    pub(super) fn set_sessions(&mut self, sessions: Option<Vec<jcode_sdk::SessionInfo>>) {
        self.sessions = sessions;
    }

    pub(super) fn set_navigation(&mut self, navigation: Subscription) {
        self.navigation = Some(navigation);
    }

    pub(super) fn set_actions(&mut self, actions: Subscription) {
        self.actions = Some(actions);
    }

    pub(super) fn set_provider(&mut self, provider: &str) {
        self.provider = Some(provider.into());
    }

    pub(super) fn set_levels(&mut self, level: f32) {
        for _ in 0..BARS {
            self.meter.push(level);
        }
    }

    pub(super) fn set_started(&mut self) {
        self.started = Some(Instant::now());
    }

    pub(super) fn has_sessions(&self) -> bool {
        self.sessions.is_some()
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.recording.is_some() || self.task.is_some() || self.timer.is_some()
    }
}
