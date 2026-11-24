use super::*;

fn active_panel(cx: &mut Context<Panel>) -> Panel {
    let mut panel = Panel::new(
        "reconnect-test".into(),
        None,
        None,
        crate::harness::spawn_inert(),
        cx,
    );
    panel.history_loaded = true;
    panel.status = "processing".into();
    panel.items.push(Item::User("Continue".into()));
    panel.streaming_text = "Partial answer".into();
    panel
}

fn replay_processing_and_usage(panel: &mut Panel, cx: &mut Context<Panel>) {
    panel.apply(
        &ApiEvent::SessionStatus {
            session_id: "reconnect-test".into(),
            status: "processing".into(),
            pending_soft_interrupts: None,
        },
        cx,
    );
    panel.apply(
        &ApiEvent::TokenUsage {
            session_id: "reconnect-test".into(),
            input: 100,
            output: 20,
            cache_read_input: None,
            cache_creation_input: None,
            effective_budget: None,
        },
        cx,
    );
}

fn notices(panel: &Panel) -> Vec<&StopNotice> {
    panel
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Stopped(notice) => Some(notice),
            _ => None,
        })
        .collect()
}

fn footer_count(panel: &Panel) -> usize {
    panel
        .items
        .iter()
        .filter(|item| matches!(item, Item::ResponseStats(_)))
        .count()
}

#[gpui::test]
fn reload_recovery_shows_activity_until_output_or_completion(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "reload-test".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.load_history(vec![], vec![], cx);
        panel
    });
    let session_id = "reload-test".to_owned();
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::SessionRecovery {
                session_id: session_id.clone(),
                continuation_message: "Must not appear in the transcript".into(),
                reconnect_notice: None,
            },
            cx,
        );
        for status in ["idle", "running"] {
            panel.apply(
                &ApiEvent::SessionStatus {
                    session_id: session_id.clone(),
                    status: status.into(),
                    pending_soft_interrupts: None,
                },
                cx,
            );
            assert!(panel.activity_active());
            assert_eq!(panel.status_line(), "Interrupted by update, continuing…");
        }
        assert!(panel.items.is_empty(), "recovery is not user input");
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("transcript-activity").is_some());

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::ToolStart {
                session_id: session_id.clone(),
                call_id: "tool-1".into(),
                name: "read".into(),
            },
            cx,
        );
        assert_eq!(panel.status_line(), "Running tools");
        panel.apply(
            &ApiEvent::TurnDone {
                session_id,
                pending_soft_interrupts: None,
            },
            cx,
        );
        assert!(!panel.activity_active());
    });
}

#[gpui::test]
fn attach_timeout_reconnect_keeps_transcript_free_of_error_cards(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| active_panel(cx));
    panel.update(vcx, |panel, cx| {
        panel.connection_lost("timeout: no reply to attach_session within 30s; reconnecting", cx);
        assert!(panel.status.starts_with("lost:"));
        assert!(notices(panel).is_empty(), "temporary reconnect must not stop the turn");
        assert!(!panel.items.iter().any(|item| matches!(item, Item::Error(_) | Item::RuntimeError { .. })));
        replay_processing_and_usage(panel, cx);
        assert!(notices(panel).is_empty());
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("response-stop-notice").is_none());
}

fn provisional_stop(panel: &mut Panel, reason: &str) {
    panel.record_stop(StopNotice {
        title: "Provisional stop".into(),
        detail: "Outcome unknown".into(),
        provider_stop_reason: None,
        failure: false,
        provisional: true,
        raw: Some(reason.into()),
        retry: AutoRetry::None,
    });
}

#[gpui::test]
fn reconnect_notice_tests_replayed_processing_and_stats_coalesce(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| active_panel(cx));
    panel.update(vcx, |panel, cx| {
        replay_processing_and_usage(panel, cx);
        provisional_stop(panel, "Socket closed");
        assert_eq!(footer_count(panel), 1);
        // History/accounting replay can put a stats row after the notice too.
        panel
            .items
            .push(Item::ResponseStats(response_stats::fixture()));
        let footers = footer_count(panel);
        for _ in 0..4 {
            replay_processing_and_usage(panel, cx);
            provisional_stop(panel, "Socket closed again");
            assert_eq!(notices(panel).len(), 1);
            assert_eq!(footer_count(panel), footers);
        }
        assert_eq!(
            notices(panel)[0].raw.as_deref(),
            Some("Socket closed again")
        );
    });
}

#[gpui::test]
fn reconnect_notice_tests_confirmed_replaces_across_stats_and_survives_provisional(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = cx.add_window_view(|_, cx| active_panel(cx));
    panel.update(vcx, |panel, cx| {
        provisional_stop(panel, "Socket closed");
        panel
            .items
            .push(Item::ResponseStats(response_stats::fixture()));
        replay_processing_and_usage(panel, cx);
        let confirmed = StopNotice::from_event(
            &TurnStopReason::Failure,
            "Specific runtime failure",
            Some("provider_detail"),
        );
        panel.record_stop(confirmed.clone());
        assert_eq!(notices(panel), vec![&confirmed]);
        let footers = footer_count(panel);
        replay_processing_and_usage(panel, cx);
        provisional_stop(panel, "Another disconnect");
        assert_eq!(notices(panel), vec![&confirmed]);
        assert_eq!(footer_count(panel), footers);
    });
}

#[gpui::test]
fn reconnect_notice_tests_real_output_and_new_user_are_boundaries(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| active_panel(cx));
    panel.update(vcx, |panel, cx| {
        provisional_stop(panel, "Initial disconnect");
        for boundary in [
            Item::Assistant("New answer".into()),
            Item::Reasoning("New reasoning".into()),
            Item::Tool {
                call_id: "new-tool".into(),
                name: "bash".into(),
                input: "{}".into(),
                output: "New result".into(),
                done: true,
                error: None,
            },
            Item::User("New turn".into()),
        ] {
            let count = notices(panel).len();
            let footers = footer_count(panel);
            panel.items.push(boundary);
            replay_processing_and_usage(panel, cx);
            provisional_stop(panel, "New disconnect");
            assert_eq!(notices(panel).len(), count + 1);
            assert_eq!(footer_count(panel), footers + 1);
        }
        // Buffered output is flushed by finish_response and must also break coalescing.
        for reasoning in [false, true] {
            let count = notices(panel).len();
            let footers = footer_count(panel);
            if reasoning {
                panel.streaming_reasoning = "Fresh streamed reasoning".into();
            } else {
                panel.streaming_text = "Fresh streamed answer".into();
            }
            replay_processing_and_usage(panel, cx);
            provisional_stop(panel, "Disconnect after new output");
            assert_eq!(notices(panel).len(), count + 1);
            assert_eq!(footer_count(panel), footers + 1);
        }
    });
}
