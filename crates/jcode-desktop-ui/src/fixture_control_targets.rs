use gpui::{
    App, Bounds, Canvas, Global, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, PlatformInput,
    Point, Styled, Window, canvas, point, px,
};
use std::cell::RefCell;

pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("JCODE_DESKTOP_SCREENSHOT").as_deref() == Ok("1")
            && std::env::var("JCODE_DESKTOP_CONTROL_TEST").as_deref() == Ok("1")
    })
}

pub(crate) fn transcript_scroll_fixture() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref()
            == Ok("transcript-scroll")
}

pub(crate) fn draft_session() -> bool {
    enabled() && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("draft-session")
}

pub(crate) fn background_tasks() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("background-tasks")
}

struct Capture {
    selector: String,
    session: String,
    matches: Vec<Bounds<Pixels>>,
}

#[derive(Default)]
struct Targets(RefCell<Option<Capture>>);

impl Global for Targets {}

pub(crate) fn marker(
    selector: impl FnOnce() -> String,
    session: impl FnOnce() -> Option<String>,
) -> Option<Canvas<()>> {
    enabled().then(|| {
        let selector = selector();
        let session = session();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                let Some(targets) = cx.try_global::<Targets>() else {
                    return;
                };
                let mut active = targets.0.borrow_mut();
                let Some(capture) = active.as_mut() else {
                    return;
                };
                if capture.selector == selector
                    && session
                        .as_ref()
                        .is_none_or(|owner| owner == &capture.session)
                {
                    let viewport = Bounds {
                        origin: point(px(0.), px(0.)),
                        size: window.viewport_size(),
                    };
                    let bounds = bounds
                        .intersect(&window.content_mask().bounds)
                        .intersect(&viewport);
                    if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                        capture.matches.push(bounds);
                    }
                }
            },
        )
        .absolute()
        .inset_0()
    })
}

pub(crate) fn is_selector(selector: &str) -> bool {
    selector == "swarm-status"
        || selector == "hover:panel-cache"
        || selector.starts_with("sidebar-worker:")
        || selector.starts_with("sidebar-todos:")
        || selector.starts_with("status-worker:")
        || selector.starts_with("background-job:")
        || (draft_session() && (selector.starts_with("draft-model-row:")
            || matches!(selector, "composer-location" | "folder-picker-entry-0" | "folder-picker-open" | "panel-model" | "panel-model-effort" | "draft-effort-high")))
        || selector.starts_with("proof-pointer:")
        || (background_tasks() && selector.starts_with("task:"))
        || matches!(selector, "compaction-divider" | "history:scroll-up")
        || (transcript_scroll_fixture()
            && matches!(
                selector,
                "prompt:toggle" | "scroll:tool" | "scroll:momentum" | "scroll:transcript"
            ))
}

pub(crate) fn dispatch(selector: &str, session: &str, window: &mut Window, cx: &mut App) -> bool {
    if let Some(coordinates) = selector.strip_prefix("proof-pointer:") {
        let Some((x, y)) = coordinates.split_once(',') else {
            return false;
        };
        let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) else {
            return false;
        };
        let viewport = window.viewport_size();
        if !(0.0..f32::from(viewport.width)).contains(&x)
            || !(0.0..f32::from(viewport.height)).contains(&y)
        {
            return false;
        }
        window.refresh();
        window.draw(cx).clear(cx);
        window.dispatch_event(
            PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position: point(px(x), px(y)),
                ..Default::default()
            }),
            cx,
        );
        return true;
    }
    let marker = if selector == "scroll:momentum" {
        "scroll:tool"
    } else {
        selector
    };
    let Some(position) = locate(marker, session, window, cx) else {
        return false;
    };
    if selector == "hover:panel-cache" {
        window.dispatch_event(
            PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                ..Default::default()
            }),
            cx,
        );
        window.refresh();
        window.draw(cx).clear(cx);
        return true;
    }
    let wheel = match selector {
        "history:scroll-up" => Some((12_000., gpui::TouchPhase::Moved)),
        "scroll:tool" => Some((-12_000., gpui::TouchPhase::Started)),
        "scroll:momentum" => Some((-120., gpui::TouchPhase::Moved)),
        "scroll:transcript" => Some((-120., gpui::TouchPhase::Started)),
        _ => None,
    };
    if let Some((dy, touch_phase)) = wheel {
        window.dispatch_event(
            PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase,
            }),
            cx,
        );
        return true;
    }
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    true
}

fn locate(
    selector: &str,
    session: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<Point<Pixels>> {
    cx.set_global(Targets(RefCell::new(Some(Capture {
        selector: selector.into(),
        session: session.into(),
        matches: Vec::new(),
    }))));
    window.refresh();
    window.draw(cx).clear(cx);
    let capture = cx.global::<Targets>().0.take()?;
    match capture.matches.as_slice() {
        [bounds] => Some(bounds.center()),
        _ => None,
    }
}
