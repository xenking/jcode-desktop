//! Hidden models and the two-step model -> effort picker, through the real keymap.
use super::*;
use gpui::{TestAppContext, WindowOptions};

fn route(model: &str, provider: &str, method: &str) -> jcode_sdk::ModelRouteInfo {
    jcode_sdk::ModelRouteInfo {
        model: model.into(),
        provider: provider.into(),
        api_method: method.into(),
        available: true,
        detail: String::new(),
        usage: None,
    }
}

fn catalog() -> Vec<jcode_sdk::ModelRouteInfo> {
    vec![
        route("gpt-5.6-sol", "OpenAI", "openai-oauth"),
        route("gpt-5.5", "OpenAI", "openai-oauth"),
        route("llama3", "Ollama", "ollama"),
    ]
}

fn window(
    cx: &mut TestAppContext,
) -> (
    gpui::WindowHandle<PromptInput>,
    std::rc::Rc<RefCell<Vec<String>>>,
) {
    cx.update(|cx| bind_keys(cx));
    let submitted = std::rc::Rc::new(RefCell::new(Vec::new()));
    let sink = submitted.clone();
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| {
                PromptInput::new(cx, "test", move |text, _, _, _| {
                    sink.borrow_mut().push(text)
                })
            })
        })
        .unwrap()
    });
    window
        .update(cx, |input, window, cx| {
            window.focus(&input.focus_handle, cx);
            input.set_model_routes(Vec::new(), &catalog(), Some("gpt-5.5".into()), cx);
        })
        .unwrap();
    (window, submitted)
}

fn values(input: &PromptInput) -> Vec<String> {
    input
        .command_suggestions()
        .iter()
        .map(|row| row.value.clone())
        .collect()
}

#[gpui::test]
fn hidden_models_leave_the_picker_but_the_current_route_stays(cx: &mut TestAppContext) {
    crate::config::set_hidden_models(&[]).unwrap();
    let (window, _) = window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            assert!(values(input).contains(&"/model openai-oauth:gpt-5.6-sol".to_string()));
            crate::config::set_hidden_models(&[
                "openai-oauth:gpt-5.6-sol".into(),
                "openai-oauth:gpt-5.5".into(),
            ])
            .unwrap();
            input.suggestions_revision += 1;
            let rows = values(input);
            assert!(!rows.contains(&"/model openai-oauth:gpt-5.6-sol".to_string()));
            assert!(
                rows.contains(&"/model openai-oauth:gpt-5.5".to_string()),
                "hiding the route in use must not make the session unreachable"
            );
            assert_eq!(rows.last().map(String::as_str), Some("Manage models"));
            assert_eq!(input.command_suggestions().last().unwrap().help, "1 hidden");
            // Search honours the same list.
            input.set_content("/model 5.6".into(), cx);
            assert!(!values(input).contains(&"/model openai-oauth:gpt-5.6-sol".to_string()));
        })
        .unwrap();
    crate::config::set_hidden_models(&[]).unwrap();
}

#[gpui::test]
fn manage_view_lists_every_route_and_enter_toggles_without_leaving(cx: &mut TestAppContext) {
    crate::config::set_hidden_models(&[]).unwrap();
    let (window, submitted) = window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            let manage = values(input).len() - 1;
            input.command_selection = manage;
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, cx| {
            assert_eq!(input.model_stage, model_stage::ModelStage::Manage);
            assert!(
                input
                    .model_stage_rows("")
                    .iter()
                    .all(|row| row.help == "Shown")
            );
            assert_eq!(input.model_picker_rows().len(), 3);
            let target = values(input)
                .iter()
                .position(|row| row == "/model openai-oauth:gpt-5.6-sol")
                .unwrap();
            input.command_selection = target;
            cx.notify();
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, _| {
            assert_eq!(crate::config::hidden_models(), ["openai-oauth:gpt-5.6-sol"]);
            assert_eq!(input.model_stage, model_stage::ModelStage::Manage);
            assert_eq!(
                input.model_picker_rows().len(),
                3,
                "hidden routes stay listed here"
            );
            assert_eq!(
                input.command_suggestions()[input.command_selection].help,
                "Hidden"
            );
        })
        .unwrap();
    assert!(
        submitted.borrow().is_empty(),
        "toggling never switches model"
    );
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.model_stage, model_stage::ModelStage::Models);
            assert_eq!(input.content.as_ref(), "/model ");
            assert!(!values(input).contains(&"/model openai-oauth:gpt-5.6-sol".to_string()));
        })
        .unwrap();
    crate::config::set_hidden_models(&[]).unwrap();
}

#[gpui::test]
fn enter_on_a_model_opens_its_effort_list_and_esc_goes_back(cx: &mut TestAppContext) {
    crate::config::set_hidden_models(&[]).unwrap();
    let (window, submitted) = window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            input.command_selection = values(input)
                .iter()
                .position(|row| row == "/model openai-oauth:gpt-5.6-sol")
                .unwrap();
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, _| {
            assert!(matches!(
                input.model_stage,
                model_stage::ModelStage::Efforts { .. }
            ));
            assert_eq!(input.content.as_ref(), "/model ");
            let rows = input.command_suggestions();
            let levels: Vec<_> = rows.iter().map(|row| row.value.as_str()).collect();
            // Exactly the model's ladder, no model rows and no swarm sentinel.
            assert_eq!(
                levels,
                [
                    "/effort none",
                    "/effort minimal",
                    "/effort low",
                    "/effort medium",
                    "/effort high",
                    "/effort xhigh",
                    "/effort max"
                ]
            );
            assert!(
                rows[0]
                    .header
                    .as_deref()
                    .unwrap()
                    .starts_with("Reasoning effort")
            );
            assert_eq!(rows[input.command_selection].value, "/effort medium");
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.model_stage, model_stage::ModelStage::Models);
            assert_eq!(
                input.command_suggestions()[input.command_selection].value,
                "/model openai-oauth:gpt-5.6-sol",
                "back lands on the model that was open"
            );
        })
        .unwrap();
    assert!(submitted.borrow().is_empty());
    // Second Esc, with no step left, closes the menu.
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |input, _, _| assert!(input.content.is_empty()))
        .unwrap();
}

#[gpui::test]
fn effort_step_submits_model_and_level_together(cx: &mut TestAppContext) {
    crate::config::set_hidden_models(&[]).unwrap();
    let (window, submitted) = window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            input.command_selection = values(input)
                .iter()
                .position(|row| row == "/model openai-oauth:gpt-5.6-sol")
                .unwrap();
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter down enter");
    assert_eq!(
        submitted.borrow().as_slice(),
        ["/model openai-oauth:gpt-5.6-sol --effort high"]
    );
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.model_stage, model_stage::ModelStage::Models);
            assert!(input.content.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn model_without_effort_control_switches_immediately(cx: &mut TestAppContext) {
    crate::config::set_hidden_models(&[]).unwrap();
    let (window, submitted) = window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            input.command_selection = values(input)
                .iter()
                .position(|row| row == "/model llama3")
                .unwrap_or_else(|| panic!("{:?}", values(input)));
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    assert_eq!(submitted.borrow().as_slice(), ["/model llama3"]);
}
