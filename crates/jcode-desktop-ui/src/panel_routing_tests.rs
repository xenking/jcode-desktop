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

fn panel_with_bridge(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<Panel>,
    &mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Bridge>,
) {
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

    assert!(vcx.debug_bounds("routing-gate-card").is_some(), "the card paints");
    assert!(
        vcx.debug_bounds("composer-text-slot").is_none() && vcx.debug_bounds("composer-tabs").is_some(),
        "the gate replaces only the text slot and leaves the composer bar"
    );
    for row in [
        "routing-gate-row-0",
        "routing-gate-row-1",
        "routing-gate-row-2",
        "routing-gate-row-3",
    ] {
        assert!(vcx.debug_bounds(row).is_some(), "{row} paints: three options and Cancel");
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
        assert!(panel.routing_gate.as_ref().unwrap().submitting());
    });
}

#[gpui::test]
fn enter_answers_the_focused_switch_and_escape_cancels(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    focus_panel(&panel, vcx);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        answers(&commands),
        vec![RoutingGateChoice::Switch { account: "claude:spare".into() }]
    );

    // A refused answer reopens the card with the reason, and Escape cancels.
    panel.update(vcx, |panel, cx| {
        panel.routing_gate_answer_settled("g1", Err("No routing gate g1 is waiting".into()), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-gate-error").is_some(), "the reason is shown");
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
    assert!(answers(&commands).is_empty(), "no second answer while one is in flight");
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
    assert!(vcx.debug_bounds("routing-gate-card").is_some(), "another gate's close is not ours");

    panel.update(vcx, |panel, cx| {
        panel.apply(
            &event(serde_json::json!({"ev":"routing_gate_closed","session_id":"session-a","gate_id":"g1"})),
            cx,
        )
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-gate-card").is_none());
    assert!(vcx.debug_bounds("composer-text-slot").is_some(), "the composer returns");
}

#[gpui::test]
fn a_finished_turn_cannot_leave_a_gate_open(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| panel.apply(&gate_event("g1"), cx));
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.apply(&event(serde_json::json!({"ev":"turn_done","session_id":"session-a"})), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("routing-gate-card").is_none());
}

#[gpui::test]
fn the_chip_names_the_account_with_a_warning_marker_and_a_pending_move(
    cx: &mut gpui::TestAppContext,
) {
    let (panel, vcx, _commands) = panel_with_bridge(cx);
    assert!(vcx.debug_bounds("composer-routing-chip").is_none(), "no chip before a decision");

    panel.update(vcx, |panel, cx| panel.apply(&decision("claude:team", &[], None), cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-routing-account").is_some());
    assert!(vcx.debug_bounds("composer-routing-warning").is_none());
    assert!(vcx.debug_bounds("composer-routing-pending").is_none());

    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:team", &["budget hf 90 of 100"], Some("claude:spare")), cx)
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("composer-routing-warning").is_some());
    assert!(vcx.debug_bounds("composer-routing-pending").is_some());
    panel.read_with(vcx, |panel, _| {
        assert_eq!(
            panel.routing_chip.as_ref().unwrap().label(),
            "claude:team ⚠ → claude:spare"
        );
        assert!(
            !panel.items.iter().any(|item| matches!(item, Item::Assistant(text) if text.starts_with("Routing: now on"))),
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
            .filter(|item| matches!(item, Item::Assistant(text) if text.starts_with("Routing: now on")))
            .count()
    };
    panel.update(vcx, |panel, cx| panel.apply(&decision("claude:team", &[], None), cx));
    panel.read_with(vcx, |panel, _| assert_eq!(announcements(panel), 0, "the first decision is not a move"));
    panel.update(vcx, |panel, cx| {
        panel.apply(&decision("claude:spare", &[], None), cx);
        panel.apply(&decision("claude:spare", &[], None), cx);
    });
    panel.read_with(vcx, |panel, _| {
        assert_eq!(announcements(panel), 1);
        assert_eq!(panel.routing_chip.as_ref().unwrap().account, "claude:spare");
    });
}

#[gpui::test]
fn acct_sends_the_parsed_intent_and_a_bad_command_explains_itself(cx: &mut gpui::TestAppContext) {
    let (panel, vcx, commands) = panel_with_bridge(cx);
    panel.update(vcx, |panel, cx| {
        assert!(panel.handle_acct_command("/acct hold claude:team 2h", cx));
        assert!(panel.handle_acct_command("/acct nonsense", cx));
        assert!(!panel.handle_acct_command("/account", cx), "other commands are not ours");
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
        assert!(matches!(panel.items.last(), Some(Item::Error(text)) if text.contains("Usage: /acct")));
    });

    panel.update(vcx, |panel, cx| {
        panel.routing_intent_reply(Ok("Routing auto · holding claude:team".into()), cx)
    });
    panel.read_with(vcx, |panel, _| {
        assert!(matches!(panel.items.last(), Some(Item::Assistant(text)) if text.contains("holding claude:team")));
    });
}

#[gpui::test]
fn acct_is_a_registered_command(_cx: &mut gpui::TestAppContext) {
    assert!(
        crate::commands::registered_command("/acct").is_some(),
        "the composer offers /acct"
    );
}
