//! Streaming dictation: the panel's side of it. `VoiceState` decides what each
//! event means, this module performs the effects: it opens the microphone,
//! updates the composer, spawns the tasks that wait on providers and sends
//! what was said. Audio and interim text are never snapshotted.
use super::*;
use super::voice_state::{
    Ended, JEV_ROUTING, Outcome, Phase, Preview, Received, Route, Routing, Stop, voice_log,
};
use gpui::FutureExt as _;
use super::voice_trace::{ActionPill, VoiceTrace, action_pills, cost_lines, cost_summary};
use jcode_base::voice_intent::{self, VoiceIntent};
use jcode_voice::{self as voice, NariEvent, NariRecording, VoiceError};
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use super::voice_state::VoiceState;

gpui::actions!(panel_voice, [ToggleVoice]);

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

pub(crate) struct VoiceSessionRequested(pub jcode_sdk::SessionInfo, pub String);
impl gpui::EventEmitter<VoiceSessionRequested> for Panel {}

pub(crate) struct VoiceActionRequested(pub voice_intent::QuickAction, pub String);
impl gpui::EventEmitter<VoiceActionRequested> for Panel {}

impl Panel {
    #[cfg(test)]
    pub(crate) fn resolve_voice_for_test(
        &mut self,
        text: &str,
        result: anyhow::Result<VoiceIntent>,
        cx: &mut Context<Self>,
    ) {
        self.voice.begin_routing(text);
        let attempt = self.voice.attempt();
        self.finish_voice_routing(&attempt, result, cx);
    }

    /// Install what Jev needs to route this panel's utterances: the sessions
    /// it may open, and the subscriptions that carry its choices out.
    pub(crate) fn configure_voice_routing(
        &mut self,
        sessions: Vec<jcode_sdk::SessionInfo>,
        navigation: gpui::Subscription,
        actions: gpui::Subscription,
    ) {
        self.voice.wire(Routing {
            sessions,
            navigation,
            actions,
        });
    }

    pub(crate) fn show_voice_decision(&mut self, message: String, cx: &mut Context<Self>) {
        self.voice.show(Outcome::Decision(message));
        cx.notify();
    }

    /// Refine the decision shown by an unfocused hold's OS pill. Focused
    /// holds already show it on the destination panel.
    pub(crate) fn set_global_voice_decision(&mut self, message: String, cx: &mut Context<Self>) {
        if self.voice.refine_decision(message) {
            cx.notify();
        }
    }

    pub(crate) fn voice_trace(&self) -> Option<VoiceTrace> {
        self.voice.trace_card()
    }

    pub(crate) fn show_voice_trace(&mut self, trace: Option<VoiceTrace>, cx: &mut Context<Self>) {
        self.voice.show_trace(trace);
        cx.notify();
    }

    pub(crate) fn keep_voice_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        self.voice.show(Outcome::Error(
            "Jev chose: Quick action · Navigation unavailable. Transcript kept in the draft."
                .into(),
        ));
        self.input
            .update(cx, |input, cx| input.append_dictation(&tag::wrap(text), cx));
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn voice_decision_for_test(&self) -> Option<&str> {
        self.voice.decision()
    }

    #[cfg(test)]
    pub(crate) fn set_voice_checking_for_test(&mut self) {
        self.voice = VoiceState::default();
        self.voice.begin(false, false);
    }

    #[cfg(test)]
    pub(crate) fn set_voice_hold_checking_for_test(&mut self) {
        self.voice.begin(true, false);
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
        self.voice.is_active()
    }

    pub(crate) fn global_voice_snapshot(
        &self,
        attempt: &Arc<AtomicBool>,
    ) -> Option<crate::global_voice_overlay::Snapshot> {
        self.voice.global_snapshot(attempt)
    }

    pub(crate) fn cancel_global_voice(
        &mut self,
        attempt: &Arc<AtomicBool>,
        reason: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.voice.owns(attempt) && self.voice.is_global() {
            self.cancel_voice(false, reason, cx);
        }
    }

    pub(crate) fn begin_voice_hold(&mut self, cx: &mut Context<Self>) {
        if self.supports_voice() && self.voice.phase() == Phase::Idle {
            self.start_voice_capture(true, false, cx);
        }
    }

    pub(crate) fn begin_global_voice_hold(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Arc<AtomicBool>> {
        if self.supports_voice() && !self.voice_active() {
            self.start_voice_capture(true, true, cx);
            return Some(self.voice.attempt());
        }
        None
    }

    pub(crate) fn end_global_voice_hold(
        &mut self,
        attempt: &Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        if self.voice.owns(attempt) && self.voice.is_global() {
            self.end_voice_hold(cx);
        }
    }

    pub(crate) fn end_voice_hold(&mut self, cx: &mut Context<Self>) {
        if !self.voice.is_hold() {
            return;
        }
        match self.voice.phase() {
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
        match self.voice.phase() {
            Phase::Idle => self.start_voice_capture(false, false, cx),
            Phase::Recording => self.stop_voice("voice toggled off", cx),
            Phase::Checking => self.cancel_voice(false, "voice toggled again while connecting", cx),
            Phase::Transcribing | Phase::Routing => self.cancel_voice(
                false,
                "voice toggled again while transcribing (text discarded)",
                cx,
            ),
        }
    }

    pub(super) fn cancel_ask_voice(&mut self, cx: &mut Context<Self>) {
        if self.ask_card.is_some() && self.voice_active() {
            self.cancel_voice(true, "ask answered", cx);
        }
    }

    /// `normal` marks an intentional user cancel. Everything else is logged
    /// as ABNORMAL so unexpected stops can be found and fixed.
    fn cancel_voice(&mut self, normal: bool, reason: &'static str, cx: &mut Context<Self>) {
        if self.voice.cancel(normal, reason) {
            self.input.update(cx, |input, cx| input.end_live_dictation(None, cx));
        }
        self.clear_voice_preview(cx);
        cx.notify();
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
            self.voice
                .fail("Microphone access is disabled in offline previews.".into());
            cx.notify();
            return;
        }
        if self.voice.phase() != Phase::Idle {
            return;
        }
        jcode_voice::timing::begin();
        self.voice.begin(hold_capture, global_capture);
        // Starting is nonblocking: the microphone opens and the provider
        // handshake runs in the background while PCM buffers. The press is
        // the start of recording, so the UI shows Listening immediately.
        let attempt = self.voice.attempt();
        let (backend, _) = voice::configured_backend();
        match backend {
            voice::DictationBackend::Command => return self.start_command_voice(&attempt, cx),
            voice::DictationBackend::Subscription => {
                voice_log(format_args!(
                    "subscription backend has no streaming capture"
                ));
                self.voice.fail(
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
            let provider = if plan.auto {
                format!("auto: {}", plan.label())
            } else {
                plan.label()
            };
            (recording, provider)
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
            self.voice
                .fail("Set an external dictation command in Settings > Voice.".into());
            cx.notify();
            return;
        }
        let superwhisper = jcode_desktop_voice::superwhisper::manages(&command);
        // The managed bridge bounds its own recording and stops SuperWhisper at
        // that limit, so a fixed Desktop deadline must not cut a long dictation.
        let timeout_secs = if superwhisper { 0 } else { cfg.timeout_secs };
        let task = cx.background_executor().spawn(async move {
            jcode_desktop_harness::network::run(async move {
                voice::dictation::run_command(&command, timeout_secs).await
            })
            .await
            .map_err(anyhow::Error::msg)?
        });
        let attempt = attempt.clone();
        let waiting = cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |panel, cx| {
                if !panel
                    .voice
                    .is_live(&attempt, &[Phase::Recording, Phase::Transcribing])
                {
                    return;
                }
                panel.voice.note_stop("external command finished");
                match result {
                    Ok(text) => panel.finish_voice(Ok(text), cx),
                    Err(error) => {
                        panel.voice.log_abnormal("external command failed");
                        panel.finish_voice(Err(VoiceError::CaptureFailed), cx);
                        panel
                            .voice
                            .fail(format!("Dictation command failed: {error:#}"));
                        cx.notify();
                    }
                }
            });
        });
        self.voice.command_started(superwhisper, waiting);
        cx.notify();
    }

    fn finish_voice_startup(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: Result<(NariRecording, String), VoiceError>,
        cx: &mut Context<Self>,
    ) {
        if !self.voice.is_live(attempt, &[Phase::Checking]) {
            // Dropping a late successful recording cancels its native worker.
            return;
        }
        match result {
            Ok((recording, provider)) => {
                let inline = !self.voice.is_global()
                    && self.ask_voice_input().is_none()
                    && dictation_mode() == jcode_base::protocol::TranscriptMode::Insert;
                let timer = self.spawn_voice_ticker(cx);
                self.voice.recording_started(recording, timer, provider, inline);
                if inline {
                    self.input.update(cx, |input, cx| input.begin_live_dictation(cx));
                } else {
                    self.input.update(cx, |input, cx| {
                        input.set_voice_preview(Some(String::new()), cx);
                    });
                }
            }
            Err(error) => self.voice.startup_failed(&error),
        }
        cx.notify();
    }

    /// Drain the recording's events and sample its level until the attempt
    /// ends.
    fn spawn_voice_ticker(&self, cx: &mut Context<Self>) -> Task<()> {
        const METER: Duration = Duration::from_millis(50);
        let attempt = self.voice.attempt();
        cx.spawn(async move |this, cx| {
            let mut last_level = Instant::now();
            loop {
                // Wake as soon as the recording publishes an event (the final
                // transcript above all), not on the next meter tick.
                let ready = this
                    .read_with(cx, |panel, _| panel.voice.event_ready())
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
                    .update(cx, |panel, cx| panel.tick_voice(&attempt, sample_level, cx))
                    .unwrap_or(false);
                if !keep_ticking {
                    break;
                }
            }
        })
    }

    /// One wake of the ticker. Says whether the attempt still needs ticking.
    fn tick_voice(
        &mut self,
        attempt: &Arc<AtomicBool>,
        sample_level: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.voice.owns(attempt) || !self.voice_active() {
            return false;
        }
        if sample_level {
            self.voice.sample_meter();
        }
        // Drain a bounded batch so a fast provider cannot monopolize the UI.
        for _ in 0..64 {
            let Some(event) = self.voice.next_event() else {
                break;
            };
            self.apply_voice_event(event, cx);
            if !self.voice_active() || self.voice.phase() == Phase::Routing {
                return false;
            }
        }
        if self.voice.capture_finished() {
            // Re-check after observing worker completion so a final event
            // published just after the first drain is not lost.
            while let Some(event) = self.voice.next_event() {
                self.apply_voice_event(event, cx);
                if !self.voice_active() || self.voice.phase() == Phase::Routing {
                    return false;
                }
            }
            voice_log(format_args!("worker exited without a result"));
            // finish_voice keeps any words already transcribed.
            self.finish_voice(Err(VoiceError::CaptureFailed), cx);
            return false;
        }
        cx.notify();
        true
    }

    pub(super) fn seed_inline_voice_fixture(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_content("Before after".into(), cx);
            input.set_caret("Before ".len(), cx);
        });
        self.begin_inline_voice(cx);
        self.apply_voice_event(NariEvent::Transcript("spoken wor".into()), cx);
    }

    /// Revise the words in the draft as they are spoken, for screenshots and tests.
    fn begin_inline_voice(&mut self, cx: &mut Context<Self>) {
        self.voice.fixture_inline_recording();
        self.input.update(cx, |input, cx| input.begin_live_dictation(cx));
    }

    fn apply_voice_event(&mut self, event: NariEvent, cx: &mut Context<Self>) {
        match self.voice.receive(event) {
            Received::Words => {
                let live = self.voice.transcript().to_owned();
                if self.voice.is_inline() {
                    self.input.update(cx, |input, cx| input.revise_live_dictation(&live, cx));
                } else {
                    let target = self.ask_voice_input().unwrap_or_else(|| self.input.clone());
                    target.update(cx, |input, cx| input.set_voice_preview(Some(live), cx));
                }
                cx.notify();
            }
            Received::Finished(result) => self.finish_voice(result, cx),
            Received::Ignored => {}
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
    fn keep_voice_in_draft_with(
        &mut self,
        text: &str,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        use jcode_base::protocol::TranscriptMode;
        self.voice.conclude("Inserted into the composer");
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
            self.voice.note_stop(reason);
            self.apply_voice_event(NariEvent::Finished(Ok("spoken words".into())), cx);
            return;
        }
        match self.voice.begin_stop(reason) {
            Stop::NotRunning => return,
            Stop::Recorder => {}
            Stop::Bridge => self.stop_superwhisper_capture(cx),
        }
        cx.notify();
    }

    /// Ask SuperWhisper to stop recording. If it cannot, the capture is still
    /// running and says so.
    fn stop_superwhisper_capture(&self, cx: &mut Context<Self>) {
        let attempt = self.voice.attempt();
        let stop = cx.background_executor().spawn(async move {
            std::process::Command::new("/usr/bin/open")
                .args(["-g", "superwhisper://record/stop"])
                .status()
                .is_ok_and(|status| status.success())
        });
        cx.spawn(async move |this, cx| {
            if !stop.await {
                let _ = this.update(cx, |panel, cx| {
                    if panel.voice.stop_failed(
                        &attempt,
                        "Could not stop SuperWhisper. Use its own recording shortcut.".into(),
                    ) {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn finish_voice(&mut self, result: Result<String, VoiceError>, cx: &mut Context<Self>) {
        let mode = if self.voice.is_inline() {
            jcode_base::protocol::TranscriptMode::Insert
        } else {
            dictation_mode()
        };
        self.finish_voice_with_mode(result, mode, cx);
    }

    fn finish_voice_with_mode(
        &mut self,
        result: Result<String, VoiceError>,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        let (result, note) = self.voice.salvage(result);
        self.finish_voice_text(result, mode, cx);
        if let Some(note) = note
            && self.voice.note_interruption(note)
        {
            cx.notify();
        }
    }

    fn finish_voice_text(
        &mut self,
        result: Result<String, VoiceError>,
        mode: jcode_base::protocol::TranscriptMode,
        cx: &mut Context<Self>,
    ) {
        jcode_voice::timing::mark("ui received final transcript");
        let Ended {
            audio,
            inline: was_inline,
        } = self.voice.end(&result);
        self.clear_voice_preview(cx);
        if was_inline {
            let final_text = result.as_ref().ok().map(String::as_str);
            self.input.update(cx, |input, cx| input.end_live_dictation(final_text, cx));
        }
        match result {
            Ok(text) if !text.trim().is_empty() => {
                if was_inline {
                    self.voice.settle();
                    self.focus_composer_after_voice(cx);
                    cx.notify();
                    return;
                }
                if !self.voice.is_global() && let Some(input) = self.ask_voice_input() {
                    input.update(cx, |input, cx| input.insert_dictation(&text, cx));
                    self.voice.settle();
                    cx.notify();
                    return;
                }
                if self.voice.is_global() {
                    let attempt = self.voice.attempt();
                    let check = cx
                        .background_executor()
                        .spawn(crate::global_voice_session::permitted());
                    let task = cx.spawn(async move |this, cx| {
                        let allowed = check.await;
                        let _ = this.update(cx, |panel, cx| {
                            panel
                                .finish_global_voice_text(&attempt, text, audio, allowed, mode, cx);
                        });
                    });
                    self.voice.await_session_check(task);
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
                if self.voice.can_route() {
                    self.route_voice(text, audio, cx);
                    return;
                }
                self.input.update(cx, |input, cx| {
                    input.append_dictation(&tag::wrap(&text), cx);
                });
                self.voice.settle();
                self.focus_composer_after_voice(cx);
            }
            Ok(_) => self
                .voice
                .fail("No speech was detected. Try recording again.".into()),
            Err(error) => self.voice.fail(error.to_string()),
        }
        if self.voice.is_global()
            && let Some(error) = self.voice.error()
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
        if !self.voice.is_live(attempt, &[Phase::Transcribing]) || !self.voice.is_global() {
            return;
        }
        if allowed && let Some(input) = self.ask_voice_input() {
            input.update(cx, |input, cx| input.insert_dictation(&text, cx));
            self.voice.settle_with_decision("Inserted into the answer");
            cx.notify();
        } else if allowed && mode != jcode_base::protocol::TranscriptMode::Send {
            // Unfocused hold into the draft: the words appear in the composer
            // the user comes back to, and the OS pill says so.
            eprintln!(
                "global voice: kept {} transcript chars in the draft",
                text.chars().count()
            );
            self.keep_voice_in_draft_with(&text, mode, cx);
        } else if allowed && self.voice.can_route() {
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
            self.voice.settle();
            self.focus_composer_after_voice(cx);
            cx.notify();
        } else {
            self.cancel_global_voice(attempt, "session check denied the transcript", cx);
        }
    }

    /// Send only this utterance, never text or attachments already in the
    /// composer. Sent ASAP like Enter: an active turn is steered.
    fn send_voice_directly(&mut self, text: String, cx: &mut Context<Self>) {
        self.voice.conclude("Sent to agent");
        self.submit_or_queue(tag::wrap(&text), Vec::new(), false, cx);
        jcode_voice::timing::mark("ui sent transcript");
        cx.notify();
    }

    fn route_voice(&mut self, text: String, audio: Option<Duration>, cx: &mut Context<Self>) {
        if !JEV_ROUTING {
            self.send_voice_directly(text, cx);
            return;
        }
        let attempt = self.voice.attempt();
        let candidates = self.voice.begin_routing(&text);
        let questions = match voice_intent::describe_questions(&text, &candidates) {
            Ok(questions) => questions,
            Err(error) => {
                self.finish_voice_routing(&attempt, Err(error), cx);
                return;
            }
        };
        let asked = candidates.clone();
        let work = cx.background_executor().spawn(async move {
            jcode_desktop_harness::network::run(async move {
                voice_intent::classify_with_report(&text, &asked).await
            })
            .await
            .map_err(anyhow::Error::msg)?
        });
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |panel, cx| {
                panel.finish_voice_report(&attempt, result, cx);
            });
        });
        self.voice.routing_started(candidates, questions, audio, task);
        cx.notify();
    }

    fn finish_voice_report(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: anyhow::Result<voice_intent::VoiceClassification>,
        cx: &mut Context<Self>,
    ) {
        // A canceled or superseded request cannot overwrite the visible evidence.
        if !self.voice.is_live(attempt, &[Phase::Routing]) {
            return;
        }
        let intent = self.voice.record_report(result);
        self.finish_voice_routing(attempt, intent, cx);
    }

    fn finish_voice_routing(
        &mut self,
        attempt: &Arc<AtomicBool>,
        result: anyhow::Result<VoiceIntent>,
        cx: &mut Context<Self>,
    ) {
        if !self.voice.is_live(attempt, &[Phase::Routing]) {
            return;
        }
        match self.voice.resolve_route(result) {
            // Send only this utterance, never text or attachments already in the composer.
            // Send ASAP like Enter: an active turn is steered, not queued behind.
            Route::CodingAgent(text) => self.submit_or_queue(tag::wrap(&text), Vec::new(), false, cx),
            Route::QuickAction(action, text) => cx.emit(VoiceActionRequested(action, text)),
            Route::Session(session, text) => cx.emit(VoiceSessionRequested(session, text)),
            Route::Draft(text) => self.input.update(cx, |input, cx| {
                input.append_dictation(&tag::wrap(&text), cx)
            }),
        }
        cx.notify();
    }

    /// While Jev routes a finished utterance, show it above the composer as a
    /// prompt that is on its way. Coding-agent routes then land as a normal
    /// prompt card, so the wait is visible from release to send.
    pub(super) fn render_voice_pending(&self) -> Option<gpui::AnyElement> {
        if self.voice.phase() != Phase::Routing || self.voice.transcript().trim().is_empty() {
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
                        .child(self.voice.transcript().to_owned()),
                )
                .into_any_element(),
        )
    }

    /// The shortcut is in the tooltip, never in place of the microphone.
    pub(super) fn render_voice_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let phase = self.voice.phase();
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
            .tooltip(move |_, cx| cx.new(|_| crate::components::Tooltip::header_text(tooltip_status.clone()).debug_selector("voice-shortcut-tooltip")).into())
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
#[path = "panel_voice_tests.rs"]
mod tests;
