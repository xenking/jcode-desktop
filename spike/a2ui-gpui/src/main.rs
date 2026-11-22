use gpui::{App, Context, IntoElement, Render, Window, WindowOptions, div, prelude::*, rgb};
#[cfg(test)]
use gpui::{Modifiers, TestAppContext};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

const FIXTURE: &str = include_str!("../fixture.json");
const CATALOG: &str = "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json";

#[derive(Clone)]
struct ButtonModel {
    surface: String,
    id: String,
    label: String,
    action: String,
    context: Value,
}

fn parse(input: &str) -> Result<ButtonModel, String> {
    let doc: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let v = match &doc {
        Value::Array(messages) if messages.len() == 1 => &messages[0],
        Value::Array(_) => return Err("expected exactly one message".into()),
        _ => &doc,
    };
    let text = |v: &Value| {
        v.as_str()
            .map(str::to_owned)
            .ok_or("expected string".to_owned())
    };
    if v["version"] != "1.0" || v["createSurface"]["catalogId"] != CATALOG {
        return Err("unsupported version or catalog".into());
    }
    let surface = &v["createSurface"];
    let components = surface["components"]
        .as_array()
        .ok_or("missing components")?;
    let root = components
        .iter()
        .find(|c| c["id"] == "root" && c["component"] == "Button")
        .ok_or("expected root Button")?;
    let child = components
        .iter()
        .find(|c| c["id"] == root["child"] && c["component"] == "Text")
        .ok_or("expected Text child")?;
    let event = &root["action"]["event"];
    let context = event.get("context").cloned().unwrap_or(json!({}));
    if !context.is_object() {
        return Err("context must be an object".into());
    }
    Ok(ButtonModel {
        surface: text(&surface["surfaceId"])?,
        id: text(&root["id"])?,
        label: text(&child["text"])?,
        action: text(&event["name"])?,
        context,
    })
}

struct Demo {
    model: ButtonModel,
    events: Rc<RefCell<Vec<Value>>>,
}
impl Render for Demo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.clone();
        let events = self.events.clone();
        div().p_8().child(div().id("a2ui_button").debug_selector(|| "a2ui_button".into()).px_4().py_2().rounded_full().bg(rgb(0x3856c8)).text_color(rgb(0xffffff)).cursor_pointer().child(model.label.clone()).on_click(move |_, _, _| {
            let event = json!({"version":"1.0", "action": {"name":model.action, "surfaceId":model.surface, "sourceComponentId":model.id, "timestamp":chrono::Utc::now().to_rfc3339(), "context":model.context}});
            println!("{event}");
            events.borrow_mut().push(event);
        }))
    }
}

fn main() {
    let input = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(path).expect("cannot read A2UI file"),
        None => FIXTURE.to_owned(),
    };
    let model = parse(&input).expect("invalid fixture");
    gpui_platform::application().run(move |cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Demo {
                model,
                events: Default::default(),
            })
        })
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn a2ui_button_click_emits_action(cx: &mut TestAppContext) {
        let input = match std::env::var("A2UI_FIXTURE") {
            Ok(path) => std::fs::read_to_string(path).expect("cannot read A2UI_FIXTURE"),
            Err(_) => FIXTURE.to_owned(),
        };
        let model = parse(&input).unwrap();
        let expected = model.clone();
        let events = Rc::new(RefCell::new(Vec::new()));
        let (view, vcx) = cx.add_window_view(|_, _| Demo {
            model,
            events: events.clone(),
        });
        vcx.run_until_parked();
        view.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        let bounds = vcx.debug_bounds("a2ui_button").expect("button rendered");
        vcx.simulate_click(bounds.center(), Modifiers::default());
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["action"]["name"], expected.action);
        assert_eq!(events[0]["action"]["surfaceId"], expected.surface);
        assert_eq!(events[0]["action"]["sourceComponentId"], expected.id);
        assert_eq!(events[0]["action"]["context"], expected.context);
        assert!(
            chrono::DateTime::parse_from_rfc3339(
                events[0]["action"]["timestamp"].as_str().unwrap()
            )
            .is_ok()
        );
    }
}
