use super::*;
use RoutingGateChoice as C;
use RoutingGateOption as O;

fn card() -> GateCard {
    GateCard::new(
        "g1".into(),
        "claude:team reached its 5h limit".into(),
        vec![
            O::WaitUntil {
                resets_at: 1_900_000_000,
            },
            O::Switch {
                account: "claude:spare".into(),
                detail: "40% used".into(),
            },
            O::Ack {
                account: "claude:team".into(),
            },
        ],
    )
}

#[test]
fn keys_answer_like_the_terminal_and_click_answers_its_row() {
    let mut card = card();
    assert_eq!(card.view().cursor, 1, "focus starts on the first switch");
    assert_eq!(
        card.press(GateKey::Enter),
        Some(C::Switch {
            account: "claude:spare".into()
        })
    );
    assert_eq!(card.press(GateKey::Digit(1)), Some(C::Wait));
    assert_eq!(card.press(GateKey::Esc), Some(C::Cancel));
    assert_eq!(card.press(GateKey::Down), None);
    assert_eq!(card.view().cursor, 2);
    assert_eq!(card.choose_row(3), Some(C::Cancel));
    assert_eq!(card.view().cursor, 3);
    assert_eq!(card.choose_row(4), None, "no such row");
}

#[test]
fn nothing_is_answerable_while_an_answer_is_in_flight_and_a_failure_reopens_it() {
    let mut card = card();
    card.mark_submitting(7);
    assert!(card.view().submitting);
    assert_eq!(card.press(GateKey::Enter), None);
    assert_eq!(card.choose_row(0), None);
    card.answer_failed("No routing gate g1 is waiting for this session".into());
    assert!(!card.view().submitting);
    assert_eq!(
        card.error.as_deref(),
        Some("No routing gate g1 is waiting for this session")
    );
    assert!(card.press(GateKey::Enter).is_some());
    card.mark_submitting(8);
    assert_eq!(card.error, None, "a new attempt clears the old error");
}
