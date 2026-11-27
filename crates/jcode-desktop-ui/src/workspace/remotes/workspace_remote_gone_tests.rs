//! A paired machine on screen: named by its label, loud only when the link drops,
//! and honest about a session the machine no longer has.
use super::super::remote_tests::session;
use super::*;

const ID: &str = "0123456789abcdef0123456789abcdef";
const HOST: &str = "paired:0123456789abcdef0123456789abcdef";
const SESSION: &str = "paired://0123456789abcdef0123456789abcdef/session_puppy_1";

fn publish(label: Option<&str>) {
    let machines: Vec<_> = label
        .map(|label| paired::Destination {
            id: ID.into(),
            endpoint: "https://work.example".into(),
            label: label.into(),
            working_dir: "/work/default".into(),
        })
        .into_iter()
        .collect();
    machines::publish_paired_labels(&machines);
}

#[test]
fn a_paired_machine_is_named_by_its_label_and_never_by_its_id() {
    publish(Some("WORK"));
    assert_eq!(machines::machine_label(HOST), "WORK");
    // A machine unpaired since this session was saved has no label left. It
    // must still never read as the 32-hex id.
    publish(None);
    let unknown = machines::machine_label(HOST);
    assert!(!unknown.contains(ID) && !unknown.contains("paired:"), "{unknown}");
}

#[gpui::test]
fn the_remote_chip_and_lost_notice_use_the_label(cx: &mut gpui::TestAppContext) {
    publish(Some("WORK"));
    let panel = cx.new(|cx| Panel::new(SESSION.into(), None, None, harness::spawn_inert(), cx));
    panel.update(cx, |panel, cx| {
        assert_eq!(panel.remote_machine_label().as_deref(), Some("WORK"));
        assert!(panel.remote_lost_notice().is_none(), "connected is not an alarm");
        panel.connection_lost("The gateway did not answer in time", cx);
        let notice = panel.remote_lost_notice().unwrap();
        assert!(notice.starts_with("Lost connection to WORK"), "{notice}");
        assert!(!notice.contains(ID), "{notice}");
    });
}

#[gpui::test]
fn a_remote_session_shows_a_banner_only_while_its_connection_is_lost(cx: &mut gpui::TestAppContext) {
    publish(Some("WORK"));
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.active = w.open_session(session(SESSION), cx);
        w
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-remote-lost").is_none(), "a remote session is a normal state");
    workspace.update(vcx, |w, cx| {
        w.slots[w.active].panel.update_chat(cx, |panel, cx| panel.connection_lost("down", cx));
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-remote-lost").is_some());
    workspace.update(vcx, |w, cx| {
        w.apply(Update::SessionConnected { session_id: SESSION.into() }, cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("panel-remote-lost").is_none(), "the banner leaves with the outage");
}

#[gpui::test]
fn a_session_its_machine_lost_leaves_the_list_with_one_clear_message(cx: &mut gpui::TestAppContext) {
    publish(Some("WORK"));
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.sessions.push(session(SESSION));
        w.active = w.open_session(session(SESSION), cx);
        w
    });
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        w.apply(Update::SessionGone { session_id: SESSION.into() }, cx);
        assert!(w.sessions.iter().all(|s| s.session_id != SESSION), "still listed");
        assert!(w.status.contains("WORK") && w.status.contains("Remote work"), "{}", w.status);
        assert!(!w.status.contains(ID), "{}", w.status);
        // A repeat (a second panel worker, a late retry) adds nothing.
        w.apply(Update::SessionGone { session_id: SESSION.into() }, cx);
        assert_eq!(w.gone_sessions, [SESSION]);
    });
    vcx.run_until_parked();
    vcx.executor().advance_clock(Duration::from_secs(2));
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert!(
            w.slots.iter().all(|slot| slot.closing || slot.panel.session_id(cx) != SESSION),
            "an empty panel must not keep posing as the session"
        );
        assert!(w.gone_sessions.is_empty());
    });
}

/// An empty model picker must always say why, for every cause, naming the
/// machine, and never offer a local sign-in for a remote session.
#[gpui::test]
fn an_empty_model_picker_always_explains_itself(cx: &mut gpui::TestAppContext) {
    use crate::panel::EmptyModels;
    publish(Some("WORK"));
    let (bridge, _commands) = harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new(SESSION.into(), None, None, bridge, cx));
    let say = |vcx: &mut gpui::VisualTestContext, panel: &Entity<Panel>| {
        panel.read_with(vcx, |panel, _| (panel.empty_models(), panel.empty_models_message()))
    };
    // Loading: nothing has answered yet.
    let (cause, message) = say(vcx, &panel);
    assert_eq!(cause, EmptyModels::Loading);
    assert!(message.contains("Loading") && message.contains("WORK"), "{message}");
    // The catalog request failed: the error text, not an empty list.
    panel.update(vcx, |panel, cx| panel.set_models_error("request timed out".into(), cx));
    let (cause, message) = say(vcx, &panel);
    assert_eq!(cause, EmptyModels::Failed("request timed out".into()));
    assert!(message.contains("request timed out") && message.contains("WORK"), "{message}");
    // The runtime answered with no accounts: say where to log in.
    panel.update(vcx, |panel, cx| {
        panel.apply_event(&jcode_sdk::ApiEvent::RuntimeInfo {
            speed_mode: Default::default(),
            speed_options: Vec::new(),
            session_id: SESSION.into(),
            provider: None,
            model: None,
            routes: Vec::new(),
            reasoning_effort: None,
            auth_method: None,
            account_label: None,
        }, cx);
    });
    let (cause, message) = say(vcx, &panel);
    assert_eq!(cause, EmptyModels::NoAccounts);
    assert!(message.contains("No accounts on WORK") && message.contains("jcode login"), "{message}");
    assert!(!message.contains(ID), "{message}");
    // The lost session: the connection, not the accounts, is the cause.
    panel.update(vcx, |panel, cx| panel.connection_lost("The gateway did not answer in time", cx));
    let (cause, message) = say(vcx, &panel);
    assert!(matches!(cause, EmptyModels::Lost(_)));
    assert!(message.contains("Not connected to WORK"), "{message}");
    // Opened, the picker shows that message and no local sign-in for a remote machine.
    panel.update(vcx, |panel, cx| panel.open_recovery_models(cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("recovery-model-message").is_some());
    assert!(vcx.debug_bounds("recovery-connect-account").is_none());
}
