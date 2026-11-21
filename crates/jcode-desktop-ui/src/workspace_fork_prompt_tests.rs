//! `/btw` and `/fork <prompt>` regressions from real lifecycle updates.
//!
//! The harness sends the first prompt of a fork over the child connection, so
//! the panel never saw a local submit. Without an explicit echo the question
//! was invisible and no work indicator painted, even though the runtime had
//! already accepted it. These tests drive the same `Update`/`ApiEvent`
//! sequence the harness emits for a real fork.
use super::*;
use crate::panel::Item;

fn accepted_event(session_id: &str) -> jcode_sdk::ApiEvent {
    jcode_sdk::ApiEvent::MessageAccepted {
        session_id: session_id.into(),
    }
}

#[gpui::test]
fn workspace_fork_with_prompt_shows_the_question_and_working_indicator(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.bridge = bridge;
        workspace.push_test_panel("source", cx);
        workspace.restore_focus(window, cx);
        workspace
    });
    while commands.try_recv().is_ok() {}
    let question = "why did the cache fall back to a cold request?";
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let source = workspace.slots[0].panel.entity_id();
            workspace.workspace_slash_command(
                &WorkspaceSlashCommand {
                    source,
                    content: format!("/fork {question}"),
                },
                window,
                cx,
            );
        })
    });
    assert!(matches!(
        commands.try_recv(),
        Ok(Command::ForkWithPrompt { .. })
    ));

    let child = workspace.read_with(vcx, |_, _| {
        super::super::tests::session_info("fork-1", None)
    });
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::SessionForked {
                session: child.clone(),
                prompt: Some(question.into()),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2, "the fork opens beside the source");
        let panel = workspace.slots[1].panel.read(cx);
        assert_eq!(panel.session_id, "fork-1");
        assert!(
            matches!(panel.items.first(), Some(Item::User(text)) if text == question),
            "the staged prompt paints as the first user message"
        );
        assert!(
            panel.is_busy(),
            "the forked turn must indicate work immediately"
        );
    });

    // The runtime then acknowledges the send and answers it.
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::Event {
                session_id: "fork-1".into(),
                event: accepted_event("fork-1"),
            },
            cx,
        );
        workspace.apply(
            Update::Event {
                session_id: "fork-1".into(),
                event: jcode_sdk::ApiEvent::TextDelta {
                    session_id: "fork-1".into(),
                    text: "It expired.".into(),
                    message_id: None,
                },
            },
            cx,
        );
        workspace.apply(
            Update::Event {
                session_id: "fork-1".into(),
                event: jcode_sdk::ApiEvent::TurnDone {
                    session_id: "fork-1".into(),
                    pending_soft_interrupts: None,
                },
            },
            cx,
        );
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        let panel = workspace.slots[1].panel.read(cx);
        let users = panel
            .items
            .iter()
            .filter(|item| matches!(item, Item::User(text) if text == question))
            .count();
        assert_eq!(users, 1, "the echoed prompt stays a single message");
        assert!(
            panel
                .items
                .iter()
                .any(|item| matches!(item, Item::Assistant(text) if text.contains("It expired")))
        );
    });

    // Authoritative history for the child must not duplicate the echo.
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::History {
                session_id: "fork-1".into(),
                messages: vec![user_message(question)],
                images: Vec::new(),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        let panel = workspace.slots[1].panel.read(cx);
        let users = panel
            .items
            .iter()
            .filter(|item| matches!(item, Item::User(text) if text == question))
            .count();
        assert_eq!(users, 1, "history reconciles with the local echo");
    });
}

fn user_message(content: &str) -> jcode_sdk::HistoryMessage {
    jcode_sdk::HistoryMessage {
        attachments: Vec::new(),
        timestamp_ms: None,
        response_stats: None,
        role: "user".into(),
        content: content.into(),
        tool: None,
    }
}
