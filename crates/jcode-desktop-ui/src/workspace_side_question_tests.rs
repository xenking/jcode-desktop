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
        let Ok(Command::ListSideQuestions {
            parent_session_id,
            request_id,
        }) = commands.try_recv()
        else {
            panic!("bare /btw must only request its parent's side questions");
        };
        assert_eq!(parent_session_id, "source");
        workspace.update(vcx, |workspace, cx| {
            workspace.apply(
                Update::SideQuestionHistory {
                    parent_session_id,
                    request_id,
                    result: Ok(Vec::new()),
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("side-question-history").is_some());
        assert!(commands.try_recv().is_err());
        vcx.simulate_input("/btw what is the retry policy?");
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

#[gpui::test]
fn side_question_history_reopens_a_topic_and_continues_its_child(cx: &mut gpui::TestAppContext) {
    use harness::{SideQuestionThread, SideQuestionTopic, SideQuestionTurn};
    let saved_question = "Why did we remove the retry loop? Explain what happens to failed writes, how the handler responds after a timeout, and which component decides whether another attempt is safe.";
    let saved_answer = format!(
        "> **Saved answer A**\n\n{}",
        "Continue reading.\n\n".repeat(60)
    );
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("parent", cx);
        workspace
    });
    let window = vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.focus_active(window, cx));
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("Do not overwrite".into()));
        window.window_handle()
    });
    vcx.simulate_window_resize(window, gpui::size(px(390.), px(730.)));
    vcx.run_until_parked();
    while commands.try_recv().is_ok() {}

    vcx.simulate_input("btw");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let Ok(Command::ListSideQuestions {
        parent_session_id,
        request_id,
    }) = commands.try_recv()
    else {
        panic!("standalone btw must only read the side history");
    };
    assert_eq!(parent_session_id, "parent");
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::SideQuestionHistory {
                parent_session_id: "parent".into(),
                request_id,
                result: Ok(vec![
                    SideQuestionTopic {
                        session_id: "btw-b".into(),
                        question: "Topic B".into(),
                    },
                    SideQuestionTopic {
                        session_id: "btw-a".into(),
                        question: "Topic A".into(),
                    },
                ]),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    vcx.simulate_input("Keep my main draft");
    let topic_a = vcx
        .debug_bounds("side-question-topic-1")
        .expect("history row");
    vcx.simulate_click(topic_a.center(), Default::default());
    vcx.run_until_parked();
    let Ok(Command::OpenSideQuestion {
        parent_session_id,
        session_id,
        request_id,
    }) = commands.try_recv()
    else {
        panic!("opening a topic must only read its saved answer");
    };
    assert_eq!(
        (parent_session_id.as_str(), session_id.as_str()),
        ("parent", "btw-a")
    );
    assert!(commands.try_recv().is_err());
    workspace.update(vcx, |workspace, cx| {
        workspace.apply(
            Update::SideQuestionOpened {
                parent_session_id,
                request_id,
                result: Ok(SideQuestionThread {
                    session: super::super::tests::session_info("btw-a", None),
                    turns: vec![
                        SideQuestionTurn {
                            question: "Original question".into(),
                            answer: "Do not copy this earlier answer.".into(),
                        },
                        SideQuestionTurn {
                            question: saved_question.into(),
                            answer: saved_answer.clone(),
                        },
                    ],
                }),
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let turns = vcx.debug_bounds("side-question-turns").unwrap();
    let question = vcx
        .debug_bounds("side-question-current-prompt")
        .expect("current question must be rendered when reopened");
    assert!(
        turns.contains(&question.center()),
        "open at the question, not the end of a long answer: {turns:?} vs {question:?}"
    );
    assert!(
        question.size.height > px(40.),
        "the question wraps at narrow widths"
    );
    assert!(
        vcx.debug_bounds("md-quote").is_some(),
        "render the saved Markdown quote"
    );
    let clipboard = |vcx: &mut gpui::VisualTestContext| {
        vcx.update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
    };
    assert_eq!(clipboard(vcx).as_deref(), Some("Do not overwrite"));
    let copy = vcx.debug_bounds("side-question-copy").unwrap();
    assert!(
        copy.bottom() <= turns.top(),
        "actions belong above the answer"
    );
    vcx.simulate_click(copy.center(), Default::default());
    assert_eq!(clipboard(vcx).as_deref(), Some(saved_answer.as_str()));
    vcx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("Again".into())));
    vcx.simulate_keystrokes("space");
    assert_eq!(clipboard(vcx).as_deref(), Some(saved_answer.as_str()));
    assert!(commands.try_recv().is_err());
    workspace.read_with(vcx, |workspace, cx| {
        let panel = workspace.slots[0].panel.read(cx);
        assert_eq!(panel.side_question_card_opt().unwrap().answer, saved_answer);
        assert_eq!(
            panel.input.read(cx).snapshot().content,
            "Keep my main draft"
        );
    });
    let field = vcx
        .debug_bounds("side-question-follow-up")
        .expect("follow-up field");
    vcx.simulate_click(field.center(), Default::default());
    vcx.simulate_input("btw");
    vcx.simulate_keystrokes("shift-enter");
    vcx.simulate_input("a literal follow-up");
    assert!(
        vcx.debug_bounds("side-question-follow-up")
            .unwrap()
            .size
            .height
            > field.size.height
    );
    let turns_before = vcx.debug_bounds("side-question-turns").unwrap();
    let question_before = vcx.debug_bounds("side-question-current-prompt").unwrap();
    let start = vcx.debug_bounds("side-question-resize").unwrap().center();
    let end = gpui::point(start.x, start.y - px(80.));
    vcx.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    vcx.simulate_mouse_move(end, gpui::MouseButton::Left, Default::default());
    vcx.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
    vcx.run_until_parked();
    let turns_after = vcx.debug_bounds("side-question-turns").unwrap();
    let question_after = vcx.debug_bounds("side-question-current-prompt").unwrap();
    assert!(turns_after.size.height > turns_before.size.height);
    assert_eq!(
        question_before.top() - turns_before.top(),
        question_after.top() - turns_after.top()
    );
    vcx.simulate_input("!");
    vcx.simulate_keystrokes("enter enter");
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::ContinueSideQuestion { session_id, question })
        if session_id == "btw-a" && question == "btw\na literal follow-up!")
    );
    assert!(commands.try_recv().is_err());
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 1);
        let panel = workspace.slots[0].panel.read(cx);
        assert!(panel.items.is_empty());
        assert_eq!(
            panel.input.read(cx).snapshot().content,
            "Keep my main draft"
        );
    });
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
fn side_question_wheel_stays_in_the_card_even_at_both_edges(cx: &mut gpui::TestAppContext) {
    let (bridge, _) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("source", cx);
        workspace.push_test_panel("other", cx);
        workspace.active = 0;
        workspace.slots[0].panel.update(cx, |panel, cx| {
            panel.items.clear();
            panel.items.extend((0..80).map(|i| {
                crate::panel::Item::Assistant(format!("Parent transcript paragraph {i}"))
            }));
            cx.notify();
        });
        workspace
    });
    vcx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
    let apply = |vcx: &mut gpui::VisualTestContext, update| {
        workspace.update(vcx, |workspace, cx| workspace.apply(update, cx));
        vcx.run_until_parked();
    };
    apply(
        vcx,
        Update::SideQuestionStarted {
            parent_session_id: "source".into(),
            session: super::super::tests::session_info("btw-child", None),
            question: "Where does the retry take place?".into(),
        },
    );
    let stream = |vcx: &mut gpui::VisualTestContext, paragraphs: usize| {
        apply(
            vcx,
            Update::Event {
                session_id: "btw-child".into(),
                event: text_delta("btw-child", &"**Answer paragraph**.\n\n".repeat(paragraphs)),
            },
        );
    };
    stream(vcx, 60);
    let turns = vcx.debug_bounds("side-question-turns").unwrap();
    let card = vcx.debug_bounds("side-question-card").unwrap();
    let prompt_offset = |vcx: &mut gpui::VisualTestContext| {
        vcx.debug_bounds("side-question-current-prompt")
            .unwrap()
            .top()
            - turns.top()
    };
    let top = prompt_offset(vcx);
    assert!(top >= px(0.));
    let outer = |vcx: &mut gpui::VisualTestContext| {
        workspace.read_with(vcx, |w, cx| {
            (
                w.active_row,
                w.active,
                w.slots[0].panel.read(cx).test_scroll_offset_y(),
            )
        })
    };
    let outer_before = outer(vcx);
    let at = turns.center();
    let wheel = |vcx: &mut gpui::VisualTestContext, position, delta, touch_phase| {
        vcx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta,
            modifiers: Default::default(),
            touch_phase,
        });
        vcx.background_executor
            .advance_clock(Duration::from_millis(160));
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    };
    use gpui::{ScrollDelta, TouchPhase};
    let touch = |vcx: &mut gpui::VisualTestContext, dx, dy, phase| {
        wheel(
            vcx,
            at,
            ScrollDelta::Pixels(gpui::point(px(dx), px(dy))),
            phase,
        );
    };
    touch(vcx, 0., 400., TouchPhase::Started);
    touch(vcx, 450., 400., TouchPhase::Moved);
    touch(vcx, 0., 0., TouchPhase::Ended);
    assert_eq!(prompt_offset(vcx), top);
    assert_eq!(
        outer(vcx),
        outer_before,
        "top edge must not move the parent"
    );
    wheel(
        vcx,
        at,
        ScrollDelta::Lines(gpui::point(0., -3.)),
        TouchPhase::Moved,
    );
    let reading_offset = prompt_offset(vcx);
    assert!(reading_offset < top, "the wheel moves the side answer");
    stream(vcx, 10);
    assert_eq!(
        prompt_offset(vcx),
        reading_offset,
        "streaming leaves the reader in place"
    );
    touch(vcx, 0., -9000., TouchPhase::Started);
    let bottom = prompt_offset(vcx);
    assert!(bottom < reading_offset);
    for phase in [TouchPhase::Moved, TouchPhase::Ended] {
        touch(vcx, -500., -600., phase);
        assert_eq!(prompt_offset(vcx), bottom);
        assert_eq!(
            outer(vcx),
            outer_before,
            "bottom edge must not move the parent"
        );
        assert_eq!(vcx.debug_bounds("side-question-card").unwrap(), card);
    }
    let parent = vcx.debug_bounds("composer-dock").unwrap();
    let outside = gpui::point(card.center().x, parent.top() - px(8.));
    wheel(
        vcx,
        outside,
        ScrollDelta::Lines(gpui::point(0., 10.)),
        TouchPhase::Moved,
    );
    assert_ne!(
        outer(vcx),
        outer_before,
        "the underlying transcript can scroll outside the card"
    );
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
            let action = PromoteSideQuestion { source, session };
            workspace.promote_side_question(&action, window, cx);
            workspace.promote_side_question(&action, window, cx);
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
        matches!(commands.try_recv(), Ok(Command::PromoteSideQuestion { session_id }) if session_id == "btw-1"),
        "a promoted side question rejoins the session list"
    );
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, Command::PromoteSideQuestion { .. }))
    );
}
