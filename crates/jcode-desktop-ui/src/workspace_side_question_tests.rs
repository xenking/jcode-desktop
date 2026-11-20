//! `/btw` lifecycle from the harness updates a real run emits.
//!
//! A side question answers in the source chat's card: it must not open a tab,
//! must stream while it runs, and must be promotable to a chat of its own. The
//! source session is never touched, so its prompt cache stays valid.
use super::*;

#[gpui::test]
fn side_question_enter_paints_in_a_chat_started_from_the_welcome_composer(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    for status in ["idle", "running_tools"] {
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("source", cx);
            workspace
        });
        vcx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
        vcx.run_until_parked();
        let panel = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
        panel.update(vcx, |panel, cx| {
            panel.echo_external_prompt("Start the chat".into(), cx);
            panel.status = status.into();
            cx.notify();
        });
        vcx.run_until_parked();
        while commands.try_recv().is_ok() {}
        vcx.simulate_input("/btw");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            let state = panel.input.read(cx);
            assert_eq!(state.snapshot().content, "/btw");
            assert_eq!(
                state.transient_notice(),
                Some("Usage: /btw <side question>")
            );
        });
        assert!(vcx.debug_bounds("composer-notice").is_some());
        assert!(
            commands.try_recv().is_err(),
            "bare command must not reach the bridge"
        );
        vcx.simulate_input(" what is the retry policy?");
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-notice").is_none());
        assert!(
            matches!(commands.try_recv(), Ok(Command::SideQuestion { session_id, question })
            if session_id == "source" && question == "what is the retry policy?"),
            "{status}"
        );
        workspace.update(vcx, |workspace, cx| {
            let session = super::super::tests::session_info("btw-child", None);
            workspace.apply(
                Update::SideQuestionStarted {
                    parent_session_id: "source".into(),
                    session,
                    question: "what is the retry policy?".into(),
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("side-question-card").is_some(),
            "card must paint while {status}"
        );
    }
}

fn text_delta(session_id: &str, text: &str) -> jcode_sdk::ApiEvent {
    jcode_sdk::ApiEvent::TextDelta {
        session_id: session_id.into(),
        text: text.into(),
        message_id: None,
    }
}

fn turn_done(session_id: &str) -> jcode_sdk::ApiEvent {
    jcode_sdk::ApiEvent::TurnDone {
        session_id: session_id.into(),
        pending_soft_interrupts: None,
    }
}

#[gpui::test]
fn workspace_side_question_streams_in_a_card_and_promotes_to_a_chat(cx: &mut gpui::TestAppContext) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    vcx.run_until_parked();
    while commands.try_recv().is_ok() {}
    let question = "why did we drop the retry here?";
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: format!("/btw {question}"),
                },
                window,
                cx,
            );
        })
    });
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::SideQuestion { .. })
    ));

    let child = workspace.read_with(vcx, |_, _| super::super::tests::session_info("btw-1", None));
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::SideQuestionStarted {
                parent_session_id: "source".into(),
                session: child.clone(),
                question: question.into(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(
            workspace.slots.len(),
            1,
            "a side question never opens a tab"
        );
        let panel = workspace.slots[0].panel.read(cx);
        let card = panel.side_question_card_opt().expect("card is open");
        assert_eq!(card.question, question);
        assert!(card.is_working());
        assert!(
            panel.items.is_empty(),
            "the source transcript stays untouched"
        );
    });

    workspace.update(vcx, |workspace, cx| {
        for event in [
            text_delta("btw-1", "Because the provider already retries."),
            turn_done("btw-1"),
        ] {
            workspace.apply(
                Update::Event {
                    session_id: "btw-1".into(),
                    event,
                },
                cx,
            );
        }
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        let panel = workspace.slots[0].panel.read(cx);
        let card = panel.side_question_card_opt().expect("card is open");
        assert_eq!(card.answer.trim(), "Because the provider already retries.");
        assert!(!card.is_working());
        assert!(panel.items.is_empty(), "no answer leaks into the source");
    });

    // Promoting opens the fork as its own chat and clears the card.
    let session = workspace.read_with(vcx, |workspace, cx| {
        workspace.slots[0]
            .panel
            .read(cx)
            .side_question_card_opt()
            .expect("card is open")
            .session
            .clone()
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.promote_side_question(&PromoteSideQuestion { source, session }, window, cx);
        })
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2, "the promoted answer opens a chat");
        let promoted = workspace.slots[1].panel.read(cx);
        assert_eq!(promoted.session_id, "btw-1");
        assert!(
            workspace.slots[0]
                .panel
                .read(cx)
                .side_question_card_opt()
                .is_none(),
            "the card is handed to the promoted chat"
        );
    });
    assert!(
        matches!(commands.try_recv(), Ok(Command::RestoreSideQuestion { session_id }) if session_id == "btw-1"),
        "a promoted side question rejoins the session list"
    );
}
