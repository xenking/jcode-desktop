use super::*;

#[gpui::test]
fn interleaved_notifications_do_not_split_or_merge_assistant_markdown(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "interleaved-text".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    let session_id = "interleaved-text".to_string();
    let delta = |id: &str, text: &str| ApiEvent::TextDelta {
        session_id: session_id.clone(),
        message_id: Some(id.into()),
        text: text.into(),
    };
    let done = |id: &str| ApiEvent::TextDone {
        session_id: session_id.clone(),
        message_id: Some(id.into()),
    };
    let replace = |id: &str, text: &str| ApiEvent::TextReplace {
        session_id: session_id.clone(),
        message_id: Some(id.into()),
        text: text.into(),
    };
    let first = "**Список вкладок** в окне.";
    let second = "Переключатель готов.";
    panel.update(vcx, |panel, cx| {
        panel.items.clear();
        panel.clean_chat_override = Some(false);
        for event in [
            delta("answer-1", "**Список вклад"),
            ApiEvent::AgentMessage {
                session_id: session_id.clone(),
                from_session: "worker".into(),
                from_name: Some("Worker".into()),
                message: "Incoming DM".into(),
            },
            ApiEvent::MemoryInjected {
                session_id: session_id.clone(),
                count: 1,
                content: "One recalled memory".into(),
            },
        ] {
            panel.apply(&event, cx);
        }
        let checkpoint = serde_json::to_vec(&panel.transcript_snapshot()).unwrap();
        panel.restore_transcript(serde_json::from_slice(&checkpoint).unwrap());
        for event in [
            delta("answer-1", "ок** в окне."),
            done("answer-1"),
            delta("answer-2", second),
            replace("answer-1", first),
            done("answer-2"),
        ] {
            panel.apply(&event, cx);
        }
    });
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        let timeline: Vec<_> = panel
            .transcript_render_rows()
            .iter()
            .filter_map(|row| {
                let item = match &row.source {
                    TranscriptRowSource::Settled(index) => &panel.items[*index],
                    TranscriptRowSource::Owned(item) => item,
                };
                match item {
                    Item::AgentMessage { message, .. } => Some(message.clone()),
                    Item::Memory(_) => Some("One recalled memory".into()),
                    _ => item.assistant_text().map(str::to_owned),
                }
            })
            .collect();
        assert_eq!(
            timeline,
            [
                first,
                "Incoming DM",
                "One recalled memory",
                second,
            ],
        );
        assert_eq!(
            panel.workspace_command_page(false).content,
            format!("## Assistant\n\n{first}\n\n## Assistant\n\n{second}\n\n"),
        );
    });
    let first_row = vcx.debug_bounds("selectable-text-0-0").unwrap();
    let second_row = vcx.debug_bounds("selectable-text-3-0").unwrap();
    assert!(first_row.bottom() < second_row.top());

    panel.update(vcx, |panel, cx| {
        panel.clean_chat_override = Some(true);
        let rows = panel.transcript_render_rows();
        let mut doc = text_document::TranscriptTextDocument::default();
        let segments = doc.sync(&panel.items, &rows, &HashSet::new(), &HashSet::new()).unwrap();
        let first_plain = segments.iter().filter(|(key, _)| key.starts_with("0-")).map(|(_, text)| text.as_ref()).collect::<String>();
        let second_plain = segments.iter().filter(|(key, _)| key.starts_with("3-")).map(|(_, text)| text.as_ref()).collect::<String>();
        assert_eq!(first_plain, "Список вкладок в окне.");
        assert_eq!(second_plain, second);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("selectable-text-0-0").is_some());
    assert!(vcx.debug_bounds("selectable-text-3-0").is_some());
}

#[gpui::test]
fn reconnect_history_does_not_extend_a_preceding_framed_answer(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "interleaved-text".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        )
    });
    panel.update(vcx, |panel, cx| {
        let session_id = panel.session_id.clone();
        panel.history_loaded = true;
        *panel.items = vec![Item::User("Question".into())];
        for event in [
            ApiEvent::TextDelta {
                session_id: session_id.clone(),
                message_id: Some("first".into()),
                text: "Ready.".into(),
            },
            ApiEvent::TextDone { session_id: session_id.clone(), message_id: Some("first".into()) },
        ] {
            panel.apply(&event, cx);
        }
        let message = |role: &str, content: &str| jcode_sdk::HistoryMessage {
            role: role.into(), content: content.into(), attachments: vec![],
            tool: None, response_stats: None, timestamp_ms: None,
        };
        panel.load_history(vec![
            message("user", "Question"),
            message("assistant", "Ready."),
            message("assistant", "Ready. The switch works."),
        ], vec![], cx);
        panel.apply(&ApiEvent::SessionStatus { session_id, status: "idle".into(), pending_soft_interrupts: None }, cx);
        assert_eq!(panel.items.iter().filter_map(Item::assistant_text).collect::<Vec<_>>(),
            ["Ready.", "Ready. The switch works."]);
    });
}

#[gpui::test]
fn reconnect_turn_completion_reconciles_idle_and_active_history(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("reconnect-turn", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    for active in [false, true] {
        panel.update(vcx, |panel, cx| {
            panel.history_loaded = true;
            *panel.items = vec![Item::User("hello".into())];
            panel.streaming_text = "partial".into();
            panel.connection_phase = "streaming".into();
            panel.status = "lost: reconnecting".into();
            panel.load_history(vec![jcode_sdk::HistoryMessage {
                attachments: Vec::new(),
                response_stats: None, timestamp_ms: None, tool: None, role: "user".into(), content: "hello".into(),
            }, jcode_sdk::HistoryMessage {
                attachments: Vec::new(),
                response_stats: None, timestamp_ms: None, tool: None,
                role: "assistant".into(), content: "partial response".into(),
            }], vec![], cx);
            panel.apply(&ApiEvent::SessionStatus {
                session_id: panel.session_id.clone(),
                status: if active { "running" } else { "idle" }.into(),
                pending_soft_interrupts: None,
            }, cx);
            assert_eq!(panel.activity_active(), active);
            assert_eq!(panel.sidebar_activity().is_some(), active);
            if active {
                assert_ne!(panel.status_line(), "Ready");
            } else {
                assert_eq!(panel.status_line(), "Ready");
            }
            if !active {
                assert!(panel.streaming_text.is_empty());
                assert!(matches!(panel.items.last(), Some(Item::Assistant(text)) if text == "partial response"));
            }
            // Transport events cannot replace the authoritative turn state.
            panel.apply(&ApiEvent::SessionStatus {
                session_id: panel.session_id.clone(), status: "attached".into(),
                pending_soft_interrupts: None,
            }, cx);
            assert_eq!(panel.activity_active(), active);
            panel.apply(&ApiEvent::TurnDone { session_id: panel.session_id.clone(), pending_soft_interrupts: None }, cx);
            assert_eq!(panel.status_line(), "Ready");
            assert!(!panel.activity_active());
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("transcript-activity").is_none());
        assert!(vcx.debug_bounds("panel-activity-spinner").is_none());
    }
}

#[gpui::test]
fn reconnect_history_does_not_replay_previous_answer_over_a_new_turn(
    cx: &mut gpui::TestAppContext,
) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("reconnect-turn", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.history_loaded = true;
        *panel.items = vec![
            Item::Assistant("old answer".into()),
            Item::User("next".into()),
        ];
        panel.streaming_text = "new partial".into();
        panel.load_history(
            vec![
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "assistant".into(),
                    content: "old answer".into(),
                },
                jcode_sdk::HistoryMessage {
                    attachments: Vec::new(),
                    response_stats: None,
                    timestamp_ms: None,
                    tool: None,
                    role: "user".into(),
                    content: "next".into(),
                },
            ],
            vec![],
            cx,
        );
        assert_eq!(panel.streaming_text, "new partial");
        assert_eq!(panel.items.len(), 2);
    });
}

#[gpui::test]
fn stale_lost_status_clears_once_the_stream_resumes(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("lost-then-back", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.connection_lost("harness API stream ended", cx);
        assert!(panel.status_line().starts_with("lost:"));
        // Any session event after the outage proves the stream is back.
        panel.apply(
            &ApiEvent::ConnectionPhase {
                session_id: panel.session_id.clone(),
                phase: "connected".into(),
            },
            cx,
        );
        assert_eq!(panel.status_line(), "Ready", "no stale lost: text");
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-status-badge").is_none());
}
