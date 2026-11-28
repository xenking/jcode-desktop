//! Side-panel routing: swarm status namespacing and daemon order.

use super::*;

#[test]
fn swarm_status_namespaces_members_and_preserves_daemon_order() {
    let address = remote::SessionAddress::parse("ssh://example/lead").unwrap();
    let event = namespace_event(
        ApiEvent::SwarmStatus {
            session_id: "lead".into(),
            updated_at_unix_ms: 1_065,
            members: vec![jcode_sdk::SwarmMemberInfo {
                model: None,
                provider: None,
                effort: None,
                session_id: "worker".into(),
                status: "running".into(),
                parent_session_id: Some("lead".into()),
                agent_label: Some("task".into()),
            }],
        },
        &address,
    );
    assert_eq!(event_session_id(&event), Some("ssh://example/lead"));
    let ApiEvent::SwarmStatus {
        members,
        updated_at_unix_ms,
        ..
    } = event
    else {
        panic!("wrong event")
    };
    assert_eq!(updated_at_unix_ms, 1_065);
    assert_eq!(members[0].session_id, "ssh://example/worker");
    assert_eq!(
        members[0].parent_session_id.as_deref(),
        Some("ssh://example/lead")
    );
    assert_eq!(members[0].status, "running");
}

#[test]
fn swarm_dm_keeps_both_sessions_in_the_remote_namespace() {
    let address = remote::SessionAddress::parse("ssh://example/recipient").unwrap();
    let event = namespace_event(
        ApiEvent::AgentMessage {
            session_id: "recipient".into(),
            from_session: "sender".into(),
            from_name: Some("fox".into()),
            message: "DM from fox: check the output".into(),
        },
        &address,
    );
    assert_eq!(event_session_id(&event), Some("ssh://example/recipient"));
    assert!(
        matches!(event, ApiEvent::AgentMessage { from_session, from_name, message, .. }
        if from_session == "ssh://example/sender"
            && from_name.as_deref() == Some("fox")
            && message == "DM from fox: check the output")
    );
}

#[test]
fn side_panel_events_keep_remote_session_namespace_and_content() {
    let address = remote::SessionAddress::parse("ssh://example/session_one").unwrap();
    let snapshot = jcode_sdk::SidePanelSnapshot {
        focus_revision: 7,
        focused_page_id: Some("notes".into()),
        pages: vec![jcode_sdk::SidePanelPage {
            id: "notes".into(),
            content: "# Remote PDF".into(),
            format: jcode_sdk::SidePanelPageFormat::Pdf,
            pdf_data: Some("JVBERi0xLjcK".into()),
            file_path: "/remote-only/report.pdf".into(),
            ..Default::default()
        }],
    };
    let event = namespace_event(
        ApiEvent::SidePanelState {
            session_id: "session_one".into(),
            snapshot: snapshot.clone(),
        },
        &address,
    );
    assert_eq!(event_session_id(&event), Some("ssh://example/session_one"));
    match event {
        ApiEvent::SidePanelState {
            snapshot: actual, ..
        } => assert_eq!(actual, snapshot),
        _ => panic!("wrong event"),
    }
}
