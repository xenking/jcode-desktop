use super::*;
use crate::harness::Command as Bridge;
use jcode_sdk::ApiEvent;

fn event(json: serde_json::Value) -> ApiEvent {
    serde_json::from_value(json).expect("routing event parses")
}

fn gate_event(gate_id: &str) -> ApiEvent {
    event(serde_json::json!({
        "ev": "routing_gate", "session_id": "session-a", "gate_id": gate_id,
        "reason": "claude:team reached its 5h limit",
        "options": [
            {"kind": "switch", "account": "claude:spare", "detail": "40% used"},
            {"kind": "wait_until", "resets_at": 1_900_000_000},
            {"kind": "ack", "account": "claude:team"},
        ],
    }))
}

fn decision(account: &str, warnings: &[&str], pending: Option<&str>) -> ApiEvent {
    event(serde_json::json!({
        "ev": "routing_decision", "session_id": "session-a", "account": account,
        "reason": "priority 1", "warnings": warnings, "pending": pending,
    }))
}

type Harness<'a> = (
    gpui::Entity<Panel>,
    &'a mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Bridge>,
);

fn panel_with_bridge(cx: &mut gpui::TestAppContext) -> Harness<'_> {
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("session-a".into(), None, None, bridge, cx));
    vcx.run_until_parked();
    (panel, vcx, commands)
}

fn focus_panel(panel: &gpui::Entity<Panel>, vcx: &mut gpui::VisualTestContext) {
    vcx.update(|window, cx| {
        let focus = panel.read(cx).focus_handle.clone();
        window.focus(&focus, cx);
    });
    vcx.run_until_parked();
}

fn answers(commands: &std::sync::mpsc::Receiver<Bridge>) -> Vec<RoutingGateChoice> {
    commands
        .try_iter()
        .filter_map(|command| match command {
            Bridge::RoutingGateAnswer {
                session_id,
                gate_id,
                choice,
            } if session_id == "session-a" && gate_id == "g1" => Some(choice),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn a_gate_replaces_the_text_slot_and_a_digit_answers_it(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    assert!(vcx.debug_bounds("composer-text-slot").is_some());
    assert!(vcx.debug_bounds("routing-gate-card").is_none());

    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    focus_panel(&panel, vcx);

    assert!(
        vcx.debug_bounds("routing-gate-card").is_some(),
        "the card paints"
    );
    assert!(
        vcx.debug_bounds("composer-text-slot").is_none()
            && vcx.debug_bounds("composer-tabs").is_some(),
        "the gate replaces only the text slot and leaves the composer bar"
    );
    for row in [
        "routing-gate-row-0",
        "routing-gate-row-1",
        "routing-gate-row-2",
        "routing-gate-row-3",
    ] {
        assert!(
            vcx.debug_bounds(row).is_some(),
            "{row} paints: three options and Cancel"
        );
    }
    assert!(vcx.debug_bounds("routing-gate-row-4").is_none());

    vcx.simulate_keystrokes("3");
    vcx.run_until_parked();
    assert_eq!(
        answers(&commands),
        vec![RoutingGateChoice::Ack { minutes: 30 }],
        "the digit answers its row without moving the focus first"
    );
    panel.read_with(vcx, |panel, _| {
        assert!(panel.routing_gate.as_ref().unwrap().view().submitting);
    });
}

#[gpui::test]
fn enter_answers_the_focused_switch_and_escape_cancels(cx: &mut gpui::TestAppContext) {
    cx.update(crate::bind_workspace_keys);
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    focus_panel(&panel, vcx);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        answers(&commands),
        vec![RoutingGateChoice::Switch {
            account: "claude:spare".into()
        }]
    );

    // A refused answer reopens the card with the reason, and Escape cancels.
    panel.update(vcx, |panel, cx| {
        panel.routing_gate_answer_settled("g1", Err("No routing gate g1 is waiting".into()), cx)
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("routing-gate-error").is_some(),
        "the reason is shown"
    );
    vcx.simulate_keystrokes("down down escape");
    vcx.run_until_parked();
    assert_eq!(answers(&commands), vec![RoutingGateChoice::Cancel]);
}

#[gpui::test]
fn a_click_answers_its_row_and_a_second_click_waits_for_the_daemon(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();

    let wait = vcx.debug_bounds("routing-gate-row-1").unwrap();
    vcx.simulate_click(wait.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(answers(&commands), vec![RoutingGateChoice::Wait]);

    let cancel = vcx.debug_bounds("routing-gate-row-3").unwrap();
    vcx.simulate_click(cancel.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        answers(&commands).is_empty(),
        "no second answer while one is in flight"
    );
}

#[gpui::test]
fn the_card_closes_when_the_daemon_closes_the_gate_and_not_for_another(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &event(serde_json::json!({"ev":"routing_gate_closed","session_id":"session-a","gate_id":"other"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("routing-gate-card").is_some(),
        "another gate's close is not ours"
    );

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &event(serde_json::json!({"ev":"routing_gate_closed","session_id":"session-a","gate_id":"g1"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-gate-card").is_none());
    assert!(
        vcx.debug_bounds("composer-text-slot").is_some(),
        "the composer returns"
    );
}

#[gpui::test]
fn a_finished_turn_cannot_leave_a_gate_open(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &event(serde_json::json!({"ev":"turn_done","session_id":"session-a"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-gate-card").is_none());
}

#[gpui::test]
fn the_chip_names_the_account_with_a_warning_marker_and_a_pending_move(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    assert!(
        vcx.debug_bounds("composer-routing-chip").is_none(),
        "no chip before a decision"
    );

    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:team", &[], None), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-routing-account").is_some());
    assert!(vcx.debug_bounds("composer-routing-warning").is_none());
    assert!(vcx.debug_bounds("composer-routing-pending").is_none());

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &decision(
                "claude:team",
                &["budget hf 90 of 100"],
                Some("claude:spare"),
            ),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-routing-warning").is_some());
    assert!(vcx.debug_bounds("composer-routing-pending").is_some());
    panel.read_with(vcx, |panel, _| {
        let chip = panel.routing_chip.as_ref().unwrap();
        assert_eq!(
            chip.account_text(),
            format!(
                "{} claude:team",
                jcode_session_types::account_icon::account_icon("claude:team", None)
            )
        );
        assert_eq!(chip.warning_marker().as_deref(), Some("⚠"));
        assert_eq!(
            chip.pending_text(),
            Some(format!(
                "{} claude:spare",
                jcode_session_types::account_icon::account_icon("claude:spare", None)
            ))
        );
        assert!(
            !panel
                .items
                .iter()
                .any(|item| matches!(item, Item::SwarmWorkerNotice(_))),
            "a warning or pending move is not an account change"
        );
    });
}

#[gpui::test]
fn moving_to_another_account_is_announced_once(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    let announcements = |panel: &Panel| {
        panel
            .items
            .iter()
            .filter(|item| matches!(item, Item::SwarmWorkerNotice(text) if text.starts_with("Account: ")))
            .count()
    };
    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:team", &[], None), cx)
    });
    panel.read_with(vcx, |panel, _| {
        assert_eq!(announcements(panel), 0, "the first decision is not a move")
    });
    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:spare", &[], None), cx);
        panel.apply(&decision("claude:spare", &[], None), cx);
    });
    panel.read_with(vcx, |panel, _| {
        assert_eq!(announcements(panel), 1);
        assert_eq!(panel.routing_chip.as_ref().unwrap().account, "claude:spare");
        let Some(Item::SwarmWorkerNotice(notice)) = panel.items.last() else {
            panic!("the move is one notice row");
        };
        let icon = |account| jcode_session_types::account_icon::account_icon(account, None);
        assert_eq!(
            notice,
            &format!(
                "Account: {} claude:team → {} claude:spare (priority 1)",
                icon("claude:team"),
                icon("claude:spare")
            )
        );
    });
}

#[gpui::test]
fn acct_sends_the_parsed_intent_and_a_bad_command_explains_itself(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| {
        assert!(panel.handle_acct_command("/acct hold claude:team 2h", cx));
        assert!(panel.handle_acct_command("/acct nonsense", cx));
        assert!(
            !panel.handle_acct_command("/account", cx),
            "other commands are not ours"
        );
    });
    let sent: Vec<_> = commands
        .try_iter()
        .filter_map(|command| match command {
            Bridge::RoutingIntent { session_id, action } => Some((session_id, action)),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent,
        vec![(
            "session-a".to_string(),
            jcode_session_types::routing::RoutingIntentAction::Hold {
                account: Some("claude:team".into()),
                minutes: Some(120),
            }
        )]
    );
    panel.read_with(vcx, |panel, _| {
        let popover = panel
            .routing_popover
            .as_ref()
            .expect("the usage error shows in the popover");
        assert!(
            popover
                .rows(None)
                .iter()
                .any(|row| row.text.contains("Usage: /acct"))
        );
        assert!(
            panel.items.is_empty(),
            "no transcript item for an /acct answer"
        );
    });

    panel.update(vcx, |panel, cx| {
        panel.routing_intent_reply(Ok("Routing auto · holding claude:team".into()), cx)
    });
    panel.read_with(vcx, |panel, _| {
        let rows = panel.routing_popover.as_ref().unwrap().rows(None);
        assert!(
            rows.iter()
                .any(|row| row.text.contains("holding claude:team"))
        );
        assert!(panel.items.is_empty());
    });
}

fn status_requests(commands: &std::sync::mpsc::Receiver<Bridge>) -> usize {
    commands
        .try_iter()
        .filter(|command| {
            matches!(
                command,
                Bridge::RoutingIntent {
                    action: jcode_session_types::routing::RoutingIntentAction::Status,
                    ..
                }
            )
        })
        .count()
}

#[gpui::test]
fn the_chip_click_toggles_a_popover_and_the_status_table_never_enters_the_transcript(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:claude-fox", &[], None), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-popover").is_none());

    let chip = vcx.debug_bounds("composer-routing-chip").expect("chip");
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(
        status_requests(&commands),
        1,
        "one click asks for the table once"
    );
    assert!(
        vcx.debug_bounds("routing-popover").is_some(),
        "the click opens the popover"
    );

    let table = "routing: auto\nthis session: claude:claude-fox\n* \u{1f98a} claude:claude-fox  subscription  priority 2";
    for _ in 0..3 {
        panel.update(vcx, |panel, cx| {
            panel.routing_intent_reply(Ok(table.into()), cx)
        });
    }
    vcx.run_until_parked();
    panel.read_with(vcx, |panel, _| {
        assert!(
            panel.items.is_empty(),
            "the table is never a transcript item: {:?}",
            panel.items.len()
        );
        let rows = panel
            .routing_popover
            .as_ref()
            .unwrap()
            .rows(panel.routing_chip.as_ref());
        assert_eq!(
            rows.iter()
                .filter(|row| row.text == "routing: auto")
                .count(),
            1
        );
    });
    assert!(vcx.debug_bounds("routing-popover-row-0").is_some());

    let chip = vcx.debug_bounds("composer-routing-chip").expect("chip");
    vcx.simulate_click(chip.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("routing-popover").is_none(),
        "a second click closes it"
    );
    assert_eq!(status_requests(&commands), 0);
}

#[gpui::test]
fn acct_is_a_registered_command(_cx: &mut gpui::TestAppContext) {
    assert!(
        crate::commands::registered_command("/acct").is_some(),
        "the composer offers /acct"
    );
}

#[gpui::test]
fn an_opening_gate_takes_the_keyboard_so_keys_answer_without_a_click(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (panel, vcx, commands) = panel_with_bridge(cx);
    vcx.update(|window, cx| window.blur(cx));
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    vcx.run_until_parked();
    assert!(
        panel.read_with(vcx, |panel, _| panel.routing_gate.is_some()),
        "the gate is open"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        answers(&commands),
        vec![RoutingGateChoice::Switch {
            account: "claude:spare".into()
        }],
        "Enter answers the focused switch with no prior click"
    );
}

/// Hover the chip and read back the tooltip rows it paints.
fn hover_rows(vcx: &mut gpui::VisualTestContext, chip: gpui::Bounds<gpui::Pixels>) -> Vec<gpui::Bounds<gpui::Pixels>> {
    vcx.simulate_mouse_move(chip.center(), None, Default::default());
    vcx.run_until_parked();
    vcx.executor().advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
    (0..12)
        .map_while(|index| vcx.debug_bounds(Box::leak(format!("rows-tooltip-row-{index}").into_boxed_str())))
        .collect()
}

#[gpui::test]
fn a_long_why_wraps_inside_the_chips_tooltip_and_never_paints_over_the_next_row(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    let reason = "behind pace: claude:claude-otter would be at 91% of its weekly window by the reset, \
                  so the other subscription takes the next turns until the pace evens out";
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &event(serde_json::json!({
                "ev": "routing_decision", "session_id": "session-a", "account": "claude:claude-fox",
                "reason": reason, "warnings": [], "pending": null,
            })),
            cx,
        )
    });
    vcx.run_until_parked();
    let chip = vcx.debug_bounds("composer-routing-chip").expect("chip");
    let rows = hover_rows(vcx, chip);
    assert!(rows.len() >= 2, "the tooltip lists the decision rows: {rows:?}");
    assert!(
        rows.iter().any(|row| row.size.height > px(16.5)),
        "the Why row wraps to several lines: {rows:?}"
    );
    for pair in rows.windows(2) {
        assert!(
            pair[0].bottom() <= pair[1].top() + px(0.5) && !pair[0].intersects(&pair[1]),
            "tooltip rows overlap: {pair:?}"
        );
    }
    let tooltip = vcx.debug_bounds("rows-tooltip").expect("tooltip");
    assert!(rows.iter().all(|row| row.right() <= tooltip.right()), "a row spills out of the tooltip");
}
