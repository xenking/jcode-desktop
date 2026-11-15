use super::*;
use gpui::EntityInputHandler;

#[gpui::test]
fn model_search_never_replaces_the_composer_draft(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("draft-search".into(), None, None, bridge, cx));
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        panel.update(cx, |p, cx| {
            p.available_models = vec!["openai:test".into(), "anthropic:test".into()];
            p.input.update(cx, |input, cx| {
                input.set_command_models(p.available_models.clone(), cx)
            });
            p.focus_input(window, cx);
        });
    });
    vcx.simulate_input("a valuable draft");
    let button = vcx.debug_bounds("panel-model").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "a valuable draft"
    );
    vcx.simulate_input("anthropic");
    assert_eq!(
        panel.read_with(vcx, |p, cx| p
            .picker_search
            .as_ref()
            .unwrap()
            .1
            .read(cx)
            .content
            .to_string()),
        "anthropic"
    );
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "a valuable draft"
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "a valuable draft"
    );
    let button = vcx.debug_bounds("panel-model").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.simulate_input("anthropic");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "a valuable draft"
    );
}

#[gpui::test]
fn model_button_preserves_a_partially_typed_model_command(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("draft-command".into(), None, None, bridge, cx));
    vcx.update(|window, cx| {
        Panel::connect_input(&panel, cx);
        panel.update(cx, |p, cx| {
            p.available_models = vec!["openai:test".into()];
            p.input.update(cx, |input, cx| {
                input.set_command_models(p.available_models.clone(), cx)
            });
            p.focus_input(window, cx);
        });
    });
    vcx.simulate_input("/model unfinished");
    vcx.run_until_parked();
    let button = vcx.debug_bounds("panel-model").unwrap();
    vcx.simulate_click(button.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        panel.read_with(vcx, |p, cx| p.input.read(cx).content.to_string()),
        "/model unfinished"
    );
}

#[gpui::test]
fn footer_model_click_preserves_draft_and_selects_without_sending(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, commands) = crate::harness::spawn_recording();
    let (panel, vcx) =
        cx.add_window_view(|_, cx| Panel::new("footer-test".into(), None, None, bridge, cx));
    vcx.update(|_, cx| Panel::connect_input(&panel, cx));
    panel.update(vcx, |panel, cx| {
        panel.model = Some("openai:test".into());
        panel.provider = Some("openai".into());
        panel.auth_method = Some("oauth".into());
        panel.available_models = vec!["openai:test".into(), "anthropic:test".into()];
        panel.input.update(cx, |input, cx| {
            input.set_command_models(panel.available_models.clone(), cx);
            input.set_current_model(panel.model.clone(), cx);
            input.set_content("keep my draft".into(), cx)
        });
        cx.notify();
    });
    vcx.run_until_parked();
    let model = vcx.debug_bounds("panel-model").unwrap();
    let context = vcx.debug_bounds("panel-context-meter").unwrap();
    assert!(
        model.right() <= context.left(),
        "the context ring follows model"
    );
    vcx.simulate_click(model.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").is_some());
    assert!(commands.try_recv().is_err());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());
    let model = vcx.debug_bounds("panel-model").unwrap();
    vcx.simulate_click(model.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let choice = vcx.debug_bounds("slash-command-row-1").unwrap();
    vcx.simulate_click(choice.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(
        matches!(commands.try_recv(), Ok(Command::SetModel { model, .. }) if model == "anthropic:test")
    );
    assert!(commands.try_recv().is_err());
    assert!(vcx.debug_bounds("slash-command-overlay").is_none());
    panel.read_with(vcx, |panel, cx| {
        assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft")
    });
    panel.update(vcx, |panel, cx| {
        panel.available_models.clear();
        cx.notify();
    });
    vcx.run_until_parked();
    let model = vcx.debug_bounds("panel-model").unwrap();
    vcx.simulate_click(model.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("recovery-connect-account").is_some());
    assert!(vcx.debug_bounds("recovery-refresh-models").is_some());
}

#[gpui::test]
fn narrow_short_panels_keep_composer_and_footer_controls_visible(cx: &mut gpui::TestAppContext) {
    cx.update(crate::input::bind_keys);
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (panel, vcx) = cx.add_window_view(|_, cx| {
        Panel::new(
            "responsive-test".into(),
            None,
            Some("/a/very/long/project/path".into()),
            bridge,
            cx,
        )
    });
    let handle = vcx.update(|window, _| window.window_handle());
    for (width, height) in [(240., 240.), (320., 300.), (480., 360.), (800., 600.)] {
        vcx.simulate_window_resize(handle, gpui::size(px(width), px(height)));
        for fresh in [true, false] {
            panel.update(vcx, |panel, cx| {
                panel.startup_layout = None;
                panel.model = (!fresh).then(|| "a-very-long-model-name".repeat(4));
                panel.provider = (!fresh).then(|| "openai".into());
                panel.auth_method = (!fresh).then(|| "oauth".into());
                panel.reasoning_effort = (!fresh).then(|| "high".into());
                *panel.items = if fresh {
                    vec![]
                } else {
                    vec![Item::User("Hello".into())]
                };
                panel
                    .input
                    .update(cx, |input, cx| input.set_content("".into(), cx));
                cx.notify();
            });
            vcx.run_until_parked();
            let input = vcx.debug_bounds("prompt-input").unwrap();
            let footer = vcx.debug_bounds("composer-tabs").unwrap();
            assert!(
                input.top() >= px(0.) && input.bottom() <= px(height),
                "{width}x{height} fresh={fresh}: {input:?}"
            );
            assert!(input.left() >= px(0.) && input.right() <= px(width));
            assert!(footer.bottom() <= px(height));
            // A fresh panel has no effort yet, so the model pill is the one
            // identity control every width must keep.
            for selector in ["panel-model"] {
                let bounds = vcx.debug_bounds(selector).unwrap();
                assert!(
                    bounds.left() >= footer.left() && bounds.right() <= footer.right(),
                    "{selector} at {width}: {bounds:?} outside {footer:?}"
                );
                assert!(bounds.size.width > px(20.), "{selector} is squeezed away");
            }
            let identity = vcx.debug_bounds("panel-identity").unwrap();
            let model = vcx.debug_bounds("panel-model").unwrap();
            assert!(
                model.left() >= identity.left() && model.right() <= identity.right(),
                "{width}x{height} fresh={fresh}: {model:?} {identity:?}"
            );
            if let Some(effort) = vcx.debug_bounds("panel-model-effort") {
                assert!(
                    model.right() <= effort.left(),
                    "{width}x{height} fresh={fresh}: {model:?} {effort:?}"
                );
            }
            let location = vcx
                .debug_bounds("composer-location")
                .expect("the directory label stays in the footer");
            assert!(
                location.top() >= input.bottom(),
                "the directory label shares the pill row: {location:?} {input:?}"
            );
            assert!(
                vcx.debug_bounds("panel-build").is_none(),
                "the footer never shows a version or build label"
            );
            panel.update(vcx, |panel, cx| {
                panel.input.update(cx, |input, cx| {
                    input.set_content("long draft ".repeat(300), cx)
                });
            });
            vcx.run_until_parked();
            let input = vcx.debug_bounds("prompt-input").unwrap();
            assert!(
                input.top() >= px(0.) && input.bottom() <= px(height),
                "long draft at {width}x{height}: {input:?}"
            );
            assert!(input.left() >= px(0.) && input.right() <= px(width));
            let editor = vcx.debug_bounds("prompt-editor").unwrap();
            let before = vcx
                .update(|window, cx| {
                    let input = panel.read(cx).input.clone();
                    input.update(cx, |input, cx| {
                        input.character_index_for_point(editor.center(), window, cx)
                    })
                })
                .unwrap();
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: editor.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(40.))),
                modifiers: gpui::Modifiers::default(),
                touch_phase: gpui::TouchPhase::Started,
            });
            vcx.run_until_parked();
            let after = vcx
                .update(|window, cx| {
                    let input = panel.read(cx).input.clone();
                    input.update(cx, |input, cx| {
                        input.character_index_for_point(editor.center(), window, cx)
                    })
                })
                .unwrap();
            assert!(
                after < before,
                "embedded editor should scroll, fresh={fresh}"
            );
        }
    }
}
