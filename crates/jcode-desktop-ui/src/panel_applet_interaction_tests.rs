use super::*;
use crate::applet_runtime::{AgentOutbound, agent_instance_id};
use serde_json::json;

fn snapshot(revision: u64, note: &str) -> jcode_applet_types::AgentApplets {
    serde_json::from_value(json!({"instances": [{
        "id": "review", "applet": "jcode.agent",
        "placement": {"kind": "inline", "session_id": "server", "anchor": {"kind": "end"}},
        "document": {"revision": revision, "title": "Review", "state": {"note": note},
            "view": {"type": "stack", "children": [
                {"type": "input", "key": "note", "bind": "note", "placeholder": "Note"},
                {"type": "button", "key": "approve", "label": "Approve", "on_press": {"action": "approve"}}
            ]}}
    }]})).unwrap()
}

#[gpui::test]
fn transcript_applet_keeps_draft_and_routes_real_click(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "session-a".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        panel.items.push(Item::User("Review this".into()));
        panel
    });
    let id = agent_instance_id("session-a", "review");
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(1, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    let input = vcx
        .debug_bounds("applet-input")
        .expect("inline input painted");
    vcx.simulate_click(input.center(), gpui::Modifiers::default());
    vcx.simulate_input("note");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("note"))
        )
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(2, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.simulate_input("!");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("note!"))
        )
    });
    let button = vcx.debug_bounds("applet-action").expect("action painted");
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        let outbox = crate::applet_runtime::get(cx).agent_outbox.borrow();
        assert!(matches!(&outbox[..], [AgentOutbound::Action { session_id, instance, action, state, source_key }] if session_id == "session-a" && instance == "review" && action.action == "approve" && state["note"] == "note!" && source_key.as_deref() == Some("approve")), "{outbox:?}");
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(3, "reset")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("reset"))
        )
    });
    let input = vcx.debug_bounds("applet-input").unwrap();
    vcx.simulate_click(input.center(), gpui::Modifiers::default());
    vcx.simulate_input("!");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("reset!"))
        )
    });
    vcx.update(|_, cx| {
        assert!(crate::applet_runtime::get(cx).sync_agent("session-a", &snapshot(4, "")))
    });
    panel.update(vcx, |_, cx| cx.notify());
    vcx.run_until_parked();
    vcx.simulate_input("new");
    vcx.run_until_parked();
    vcx.update(|_, cx| {
        assert_eq!(
            crate::applet_runtime::get(cx)
                .host
                .borrow()
                .state(&id, "note"),
            Some(&json!("new"))
        )
    });
}
