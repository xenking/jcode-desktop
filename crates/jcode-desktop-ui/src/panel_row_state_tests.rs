use super::*;

fn tool(call_id: &str) -> Item {
    Item::Tool {
        call_id: call_id.into(),
        name: "bash".into(),
        input: r#"{"command":"echo test","intent":"Run a command"}"#.into(),
        output: "test".into(),
        done: true,
        error: None,
    }
}

fn keys<V>(map: &std::cell::RefCell<HashMap<String, V>>) -> Vec<String> {
    let mut keys: Vec<_> = map.borrow().keys().cloned().collect();
    keys.sort();
    keys
}

#[gpui::test]
fn row_handles_follow_the_tool_calls_still_in_the_transcript(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        let mut panel = Panel::new(
            "row-state".into(),
            None,
            None,
            crate::harness::spawn_inert(),
            cx,
        );
        *panel.items = vec![tool("first"), tool("second"), tool("third")];
        panel.stick_to_bottom = false;
        panel
    });
    vcx.run_until_parked();
    for call in ["first", "second", "third"] {
        panel.update(vcx, |panel, cx| {
            expandable::flip(&mut panel.expanded_regions, call);
            cx.notify();
        });
        vcx.run_until_parked();
    }
    let both = |panel: &Entity<Panel>, vcx: &mut gpui::VisualTestContext| {
        panel.read_with(&*vcx, |panel, _| {
            (keys(&panel.swarm_row_focus), keys(&panel.swarm_scrolls))
        })
    };
    let (focus, scrolls) = both(&panel, vcx);
    assert_eq!(focus, ["first", "second", "third"]);
    assert_eq!(scrolls, ["first", "second", "third"]);

    panel.update(vcx, |panel, cx| {
        *panel.items = vec![tool("third"), tool("fourth")];
        cx.notify();
    });
    vcx.run_until_parked();
    let (focus, scrolls) = both(&panel, vcx);
    assert_eq!(focus, ["fourth", "third"]);
    assert_eq!(scrolls, ["third"]);
}
