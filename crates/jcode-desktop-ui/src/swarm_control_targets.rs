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
        || selector.starts_with("sidebar-worker:")
        || selector.starts_with("status-worker:")
}

pub(crate) fn click(selector: &str, session: &str, window: &mut Window, cx: &mut App) -> bool {
    let Some(position) = locate(selector, session, window, cx) else {
        return false;
    };
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
