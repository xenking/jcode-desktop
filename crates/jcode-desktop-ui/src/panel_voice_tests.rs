//! Panel dictation behavior, driven through the panel with no network.
use super::*;
use std::sync::atomic::Ordering;

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
        panel.voice.set_phase(Phase::Recording);
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
        panel.voice.set_phase(Phase::Recording);
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
        panel.voice.set_phase(Phase::Routing);
        panel.voice.set_transcript(text);
        panel.voice.set_trace(Some(VoiceTrace {
            transcript: text.into(),
            candidates: Vec::new(),
            questions: voice_intent::describe_questions(text, &[]).unwrap(),
            answers: Vec::new(),
            audio: None,
            usage: None,
        }));
        let attempt = panel.voice.attempt();
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
        let trace = panel.voice.trace_card().unwrap();
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
            panel.voice.trace_card().unwrap().answers[0].probability,
            0.91
        );
        panel.cancel_voice(true, "cancel button", cx);
        assert!(panel.voice.trace_card().is_none());
        panel.finish_voice_report(
            &attempt,
            Ok(voice_intent::VoiceClassification {
                intent: VoiceIntent::CodingAgent,
                answers: Vec::new(),
                usage: None,
            }),
            cx,
        );
        assert!(panel.voice.trace_card().is_none());
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
        panel.voice.set_phase(Phase::Routing);
        panel.voice.set_transcript("keep this");
        panel.voice.set_trace(Some(VoiceTrace {
            transcript: "keep this".into(),
            candidates: Vec::new(),
            questions: voice_intent::describe_questions("keep this", &[]).unwrap(),
            answers: Vec::new(),
            audio: None,
            usage: None,
        }));
        let attempt = panel.voice.attempt();
        panel.finish_voice_report(&attempt, Err(anyhow::anyhow!("provider unavailable")), cx);
        assert_eq!(panel.voice.trace_card().unwrap().questions.len(), 6);
        assert!(panel.voice.trace_card().unwrap().answers.is_empty());
        assert!(
            panel
                .voice
                .error()
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
        let attempt = panel.voice.attempt();
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
        panel.voice.set_phase(Phase::Routing);
        panel.voice.set_transcript("fix the flaky test");
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
        let attempt = panel.voice.attempt();
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
        panel.voice.set_sessions(Some(Vec::new()));
        // Seed the same state as production after its offline-preview guard.
        panel.voice.begin(true, false);
        let attempt = panel.voice.attempt();
        panel.begin_voice_hold(cx);
        panel.begin_voice_hold(cx);
        assert!(panel.voice.owns(&attempt));
        assert!(panel.voice.phase() == Phase::Checking);
        assert!(panel.voice.error().is_none());
        panel.end_voice_hold(cx);
        panel.end_voice_hold(cx);
        assert!(attempt.load(Ordering::SeqCst));
        assert!(panel.voice.phase() == Phase::Idle);
        assert!(!panel.voice.is_hold());
        assert!(panel.voice.has_sessions());
        assert_eq!(panel.input.read(cx).content.as_ref(), "keep");

        // Late startup completion cannot resurrect a canceled capture or
        // change a newer attempt. No recording or microphone is constructed.
        panel.finish_voice_startup(&attempt, Err(VoiceError::Cancelled), cx);
        assert!(panel.voice.phase() == Phase::Idle);
        assert!(panel.voice.error().is_none());
        panel.voice.begin(true, false);
        let current = panel.voice.attempt();
        panel.finish_voice_startup(&attempt, Err(VoiceError::Network), cx);
        assert!(panel.voice.owns(&current));
        assert!(panel.voice.phase() == Phase::Checking);
        assert!(panel.voice.error().is_none());
        current.store(true, Ordering::SeqCst);
        panel.finish_voice_startup(&current, Err(VoiceError::Network), cx);
        assert!(panel.voice.phase() == Phase::Checking);
        assert!(panel.voice.error().is_none());
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
            panel.voice.set_phase(phase);
            panel.voice.set_hold(false);
            panel.voice.set_transcript("existing click request");
            let attempt = panel.voice.attempt();
            panel.begin_voice_hold(cx);
            panel.end_voice_hold(cx);
            assert!(panel.voice.phase() == phase);
            assert!(panel.voice.owns(&attempt));
            assert!(!attempt.load(Ordering::SeqCst));
            assert_eq!(panel.voice.transcript(), "existing click request");
        }
    });
}

#[gpui::test]
fn voice_hold_send_mode_routes_final_transcription_without_touching_typed_draft(
    cx: &mut gpui::TestAppContext,
) {
    let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(cx, |panel, cx| {
        panel.voice.set_sessions(Some(Vec::new()));
        panel.voice.begin(true, false);
        panel.voice.set_phase(Phase::Recording);
        panel
            .input
            .update(cx, |input, cx| input.set_content("typed draft".into(), cx));
        panel.apply_voice_event(NariEvent::Transcript("fix the bug".into()), cx);
        assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
        assert!(panel.voice.phase() == Phase::Recording);
        panel.finish_voice_with_mode(
            Ok("fix the bug".into()),
            jcode_base::protocol::TranscriptMode::Send,
            cx,
        );
        // Jev routing is off: an explicit `send` still goes straight to the agent.
        assert!(panel.voice.phase() == Phase::Idle);
        assert!(!panel.voice.is_hold());
        assert_eq!(panel.input.read(cx).content.as_ref(), "typed draft");
        assert!(
            panel.voice.decision().is_none(),
            "the transcript shows the send"
        );
        assert!(
            panel.items.iter().any(
                |item| matches!(item, Item::User(text) if text == &tag::wrap("fix the bug"))
            )
        );
        assert!(panel.voice.transcript().is_empty());
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
        assert!(panel.voice.error().is_none());
        panel.session_id = "preview://chat".into();
        panel.begin_voice_hold(cx);
        assert!(!panel.voice_active());
        assert!(!panel.voice.is_hold());
        assert!(
            panel
                .voice
                .error()
                .unwrap()
                .contains("offline previews")
        );
        assert!(!panel.voice.has_pending_work());
        assert!(!panel.voice.has_pending_work());
    });
}

#[gpui::test]
fn voice_recording_indicator_clears_when_capture_stops(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(vcx, |panel, cx| { panel.voice.set_phase(Phase::Recording); cx.notify(); });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("voice-recording-indicator").is_some());
    assert!(vcx.debug_bounds("voice-stop-icon").is_some());
    assert!(vcx.debug_bounds("voice-microphone-icon").is_none());
    panel.update(vcx, |panel, cx| { panel.voice.set_phase(Phase::Transcribing); cx.notify(); });
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
            panel.voice.set_sessions(Some(Vec::new()));
            let before = panel.items.len();
            panel.resolve_voice_for_test("spoken words", result, cx);
            assert_eq!(
                panel.input.read(cx).content.as_ref(),
                format!("typed\n{}", tag::wrap("spoken words"))
            );
            assert_eq!(panel.items.len(), before);
            assert!(!panel.voice_active());
            assert!(panel.voice.transcript().is_empty());
        });
    }
}

#[gpui::test]
fn canceled_and_superseded_jev_results_cannot_edit_the_draft(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(cx, |panel, cx| {
        panel.voice.set_phase(Phase::Routing);
        panel.voice.set_transcript("discard");
        let attempt = panel.voice.attempt();
        panel.cancel_voice(true, "cancel button", cx);
        panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
        assert!(panel.input.read(cx).content.is_empty());
        assert!(panel.items.is_empty());
        panel.voice.set_phase(Phase::Routing);
        panel.voice.set_transcript("new recording");
        panel.finish_voice_routing(&attempt, Ok(VoiceIntent::CodingAgent), cx);
        assert!(panel.input.read(cx).content.is_empty());
        assert!(panel.items.is_empty());
        assert_eq!(panel.voice.transcript(), "new recording");
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
        panel.voice.set_sessions(Some(vec![session]));
        panel.resolve_voice_for_test(
            "open PDF renderer",
            Ok(VoiceIntent::OpenSession("target".into())),
            cx,
        );
        assert_eq!(
            panel.input.read(cx).content.as_ref(),
            "keep my code request"
        );
        assert!(panel.voice.error().is_none());
    });
}

#[gpui::test]
fn voice_streaming_insert_revises_only_the_spoken_span(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(vcx, |panel, cx| {
        panel.voice.set_phase(Phase::Recording);
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
        panel.voice.set_phase(Phase::Recording);
        panel.voice.set_provider("Soniox (stt-rt-v6)");
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
        let note = panel.voice.error().map(str::to_owned).expect("the failure is reported");
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
        panel.voice.set_phase(Phase::Recording);
        panel
            .input
            .update(cx, |input, cx| input.set_content("typed draft".into(), cx));
        panel.apply_voice_event(NariEvent::Transcript("hello wor".into()), cx);
        panel.apply_voice_event(NariEvent::Transcript("Hello world.".into()), cx);
        assert_eq!(panel.voice.transcript(), "Hello world.");
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
        assert!(panel.voice.transcript().is_empty());
        assert!(!panel.voice_active());
    });
}

#[gpui::test]
fn voice_cancel_discards_partial_without_changing_draft(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(cx, |panel, cx| {
        panel.voice.set_phase(Phase::Recording);
        panel
            .input
            .update(cx, |input, cx| input.set_content("keep".into(), cx));
        panel.begin_inline_voice(cx);
        panel.apply_voice_event(NariEvent::Transcript("discard me".into()), cx);
        assert_eq!(panel.input.read(cx).content.as_ref(), "keep discard me");
        let token = panel.voice.attempt();
        panel.cancel_voice(true, "cancel button", cx);
        assert!(token.load(Ordering::SeqCst));
        assert!(panel.voice.transcript().is_empty());
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
            assert!(panel.voice.error().is_none());
        }
    });
}

/// Partials are visible while recording as dim composer text, and the
/// draft itself stays empty: nothing is inserted until the recording ends.
#[gpui::test]
fn voice_meter_and_dim_composer_partials_show_while_recording(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(vcx, |panel, cx| {
        panel.voice.set_phase(Phase::Recording);
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
                .error()
                .unwrap()
                .contains("offline previews")
        );
        assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft");
        assert!(!panel.voice.has_pending_work());
    });
    for phase in [Phase::Checking, Phase::Transcribing] {
        panel.update(vcx, |panel, cx| {
            panel.voice.set_error(None);
            panel.voice.set_phase(phase);
            cx.notify();
        });
        vcx.simulate_keystrokes("ctrl-shift-v");
        panel.read_with(vcx, |panel, cx| {
            assert!(panel.voice.phase() == Phase::Idle);
            assert!(panel.voice.error().is_none(), "toggle must fire only once");
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
                panel.voice.set_phase(phase);
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
                .error()
                .unwrap()
                .contains("offline previews")
        );
        assert!(!panel.voice.has_pending_work());
    });
    let dismiss = vcx.debug_bounds("voice-cancel").expect("dismiss error");
    vcx.simulate_click(dismiss.center(), gpui::Modifiers::default());
    for phase in [Phase::Checking, Phase::Recording, Phase::Transcribing] {
        panel.update(vcx, |panel, cx| {
            panel
                .items
                .push(Item::Assistant("Existing conversation".into()));
            panel.voice.set_phase(phase);
            panel.voice.set_started();
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
        panel.read_with(vcx, |panel, _| assert!(panel.voice.phase() == Phase::Idle));
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
        assert!(panel.voice.error().is_none());
        assert_eq!(panel.voice.phase(), Phase::Idle);
        assert!(panel.voice.transcript().is_empty());
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
        panel.voice.set_phase(Phase::Recording);
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
        panel.voice.begin(true, false);
        panel.voice.set_global(true);
        panel.voice.set_phase(Phase::Transcribing);
        let attempt = panel.voice.attempt();
        panel.finish_global_voice_text(&attempt, "denied".into(), None, false, jcode_base::protocol::TranscriptMode::Insert, cx);
        assert!(panel.ask_voice_input().unwrap().read(cx).snapshot().content.is_empty());
        panel.voice.begin(true, false);
        panel.voice.set_global(true);
        panel.voice.set_phase(Phase::Transcribing);
        let attempt = panel.voice.attempt();
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
        assert!(panel.voice.error().is_some());
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
        let token = panel.voice.attempt();
        panel.voice.set_phase(Phase::Checking);
        panel
            .input
            .update(cx, |input, cx| input.set_content("keep me".into(), cx));
        panel.cancel_voice(true, "cancel button", cx);
        assert!(token.load(Ordering::SeqCst));
        assert!(!panel.voice.owns(&token));
        assert_eq!(panel.input.read(cx).snapshot().content, "keep me");
        assert!(panel.voice.phase() == Phase::Idle);
    });
}

#[gpui::test]
fn voice_preview_never_opens_microphone(cx: &mut gpui::TestAppContext) {
    let panel = cx.new(|cx| Panel::new_preview(PreviewState::Empty, cx));
    panel.update(cx, |panel, cx| {
        panel.start_voice_capture(false, false, cx);
        assert!(panel.voice.phase() == Phase::Idle);
        assert!(!panel.voice.has_pending_work());
        assert!(!panel.voice.has_pending_work());
        assert!(
            panel
                .voice
                .error()
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
