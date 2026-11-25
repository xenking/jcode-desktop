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
            expandable::Regions::flip(&mut panel.expanded_regions, call);
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

#[gpui::test]
fn display_config_is_read_once_per_frame_not_per_row(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let reads_for_one_frame = |cx: &mut gpui::TestAppContext, rows: usize| {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            let mut panel = Panel::new(
                "display-reads".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            );
            *panel.items = (0..rows).map(|row| tool(&format!("call-{row}"))).collect();
            panel.stick_to_bottom = false;
            panel
        });
        vcx.run_until_parked();
        panel.update(vcx, |_, cx| cx.notify());
        let before = crate::config::display_reads();
        vcx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        crate::config::display_reads() - before
    };
    let few = reads_for_one_frame(cx, 3);
    let many = reads_for_one_frame(cx, 40);
    assert!(few > 0, "the frame must read the display config");
    assert_eq!(many, few, "reads per frame must not grow with visible rows");
}
