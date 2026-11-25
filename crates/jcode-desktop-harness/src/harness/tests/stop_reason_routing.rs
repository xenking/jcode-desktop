//! Stop reasons settle worker activity.

use super::*;

#[test]
fn legacy_abnormal_statuses_settle_worker_activity() {
    for status in [
        "cancelled",
        "canceled",
        "interrupted",
        "crashed",
        "error",
        "failed",
    ] {
        let mut active = true;
        update_turn_activity(
            &ApiEvent::SessionStatus {
                session_id: "legacy".into(),
                status: status.into(),
                pending_soft_interrupts: None,
            },
            &mut active,
        );
        assert!(!active, "{status}");
    }
}

fn stop(reason: jcode_sdk::TurnStopReason) -> ApiEvent {
    ApiEvent::TurnStopped {
        session_id: "session_one".into(),
        reason,
        message: "Provider stopped the response".into(),
        provider_stop_reason: Some("content_filter".into()),
    }
}

#[test]
fn cache_events_route_to_local_or_remote_session() {
    for ui_id in ["session_one", "ssh://example/session_one"] {
        let address = remote::SessionAddress::parse(ui_id).unwrap();
        let events = [
            ApiEvent::RequestUsage {
                session_id: "session_one".into(),
                request_id: "stable-request-id".into(),
                provider: "anthropic".into(),
                model: "claude-opus".into(),
                input: Some(10),
                output: Some(8),
                cache_read_input: Some(100),
                cache_creation_input: None,
            },
            ApiEvent::KvCacheMiss {
                session_id: "session_one".into(),
                reason: "expired".into(),
                harness_caused: false,
                missed_tokens: 50,
                read_tokens: 100,
                expected_tokens: 150,
                documented_cause: None,
                message: "cache expired".into(),
            },
        ];
        for event in events {
            assert_eq!(event_session_id(&event), Some("session_one"));
            assert_eq!(
                event_session_id(&namespace_event(event, &address)),
                Some(ui_id)
            );
        }
    }
}

#[test]
fn stopped_events_preserve_details_and_route_to_local_or_remote_session() {
    for (address, expected_session) in [
        ("session_one", "session_one"),
        ("ssh://example/session_one", "ssh://example/session_one"),
    ] {
        let address = remote::SessionAddress::parse(address).unwrap();
        for reason in [
            jcode_sdk::TurnStopReason::Interrupted,
            jcode_sdk::TurnStopReason::Failure,
            jcode_sdk::TurnStopReason::Crash,
            jcode_sdk::TurnStopReason::ProviderGuardrail,
            jcode_sdk::TurnStopReason::LimitReached,
            jcode_sdk::TurnStopReason::Unknown,
        ] {
            let original = stop(reason);
            assert_eq!(event_session_id(&original), Some("session_one"));
            let routed = namespace_event(original, &address);
            assert_eq!(event_session_id(&routed), Some(expected_session));
            match routed {
                ApiEvent::TurnStopped {
                    reason: actual_reason,
                    message,
                    provider_stop_reason,
                    ..
                } => {
                    assert_eq!(actual_reason, reason);
                    assert_eq!(message, "Provider stopped the response");
                    assert_eq!(provider_stop_reason.as_deref(), Some("content_filter"));
                }
                _ => panic!("routing changed the stop event type"),
            }
        }
    }
}

#[test]
fn every_stop_reason_settles_worker_activity_without_waiting_for_done_or_idle() {
    for reason in [
        jcode_sdk::TurnStopReason::Interrupted,
        jcode_sdk::TurnStopReason::Failure,
        jcode_sdk::TurnStopReason::Crash,
        jcode_sdk::TurnStopReason::ProviderGuardrail,
        jcode_sdk::TurnStopReason::LimitReached,
        jcode_sdk::TurnStopReason::Unknown,
    ] {
        for initially_active in [false, true] {
            let mut active = initially_active;
            update_turn_activity(&stop(reason), &mut active);
            assert!(!active, "{reason:?}");
            assert!(should_detach_on_drop(active));
            update_turn_activity(&stop(reason), &mut active);
            update_turn_activity(
                &ApiEvent::SessionStatus {
                    session_id: "session_one".into(),
                    status: "attached".into(),
                    pending_soft_interrupts: None,
                },
                &mut active,
            );
            assert!(
                !active,
                "a replay or transport attach cannot revive the turn"
            );
            update_turn_activity(
                &ApiEvent::MessageAccepted {
                    session_id: "session_one".into(),
                },
                &mut active,
            );
            assert!(active, "a new accepted turn must restore steering");
            assert!(!should_detach_on_drop(active));
        }
    }
}
