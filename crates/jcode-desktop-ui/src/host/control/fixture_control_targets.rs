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

/// An offline ask card fixture (`ask`, `ask-single`, ...).
pub(crate) fn ask_fixture() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT")
            .is_ok_and(|name| name.starts_with("ask"))
}

pub(crate) fn files_panel() -> bool {
    enabled() && std::env::var("JCODE_DESKTOP_CONTROL_FILES_PANEL").as_deref() == Ok("1")
}

pub(crate) fn context_overlay_fixture() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_CONTEXT_OVERLAY_PROOF").as_deref() == Ok("1")
}

fn gallery_preload() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("gallery-preload")
}

pub(crate) fn gallery_navigation() -> bool {
    enabled() && std::env::var("JCODE_DESKTOP_SCREENSHOT_GALLERY_NAVIGATION").as_deref() == Ok("1")
}

pub(crate) fn draft_session() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("draft-session")
}

pub(crate) fn side_questions() -> bool {
    draft_session()
        || (enabled()
            && std::env::var("JCODE_DESKTOP_CONTROL_SIDE_QUESTIONS").as_deref() == Ok("1"))
}

pub(crate) fn background_tasks() -> bool {
    enabled()
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("background-tasks")
}

pub(crate) fn resume_preview() -> bool {
    enabled() && std::env::var_os("JCODE_DESKTOP_SCREENSHOT_RESUME_FIXTURE").is_some()
}

pub(crate) fn voice_recording() -> bool {
    enabled() && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("voice-rt")
}

pub(crate) fn model_speed_picker() -> bool {
    enabled() && std::env::var("JCODE_DESKTOP_SCREENSHOT_COMPOSER_PICKER").as_deref() == Ok("model")
}

pub(crate) fn voice_soniox() -> bool {
    cfg!(debug_assertions)
        && enabled()
        && std::env::var("JCODE_TEST_SESSION").as_deref() == Ok("1")
        && std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").as_deref() == Ok("soniox-rt")
}

pub(crate) fn allow_soniox_recording() -> bool {
    if !voice_soniox() {
        return false;
    }
    let config = &jcode_base::config::config().dictation;
    config.backend == jcode_base::config::DictationBackend::Soniox
        && !config.recorder.trim().is_empty()
        && config
            .base_url
            .strip_prefix("ws://")
            .and_then(|url| url.strip_suffix("/transcribe-websocket"))
            .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
            .is_some_and(|address| address.ip().is_loopback())
        && std::env::var("SONIOX_API_KEY").as_deref() == Ok("desktop-loopback-fixture")
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
        || matches!(
            selector,
            "clean-chat-on" | "clean-chat-off" | "hover:clean-chat-on" | "hover:clean-chat-off"
        )
        || selector == "transcript-activity"
        || selector.starts_with("session-status-")
        || selector == "openrig-section-menu"
        || (files_panel()
            && (matches!(selector, "workspace-files-toggle" | "workspace-files-close")
                || selector.starts_with("right-file-tree-")))
        || selector.starts_with("openrig-section:")
        || selector.starts_with("work-row:")
        || selector.starts_with("build-row:")
        || (selector.starts_with("openrig-chat:") && selector.ends_with(":open"))
        || selector == "finished-workers"
        || selector == "hover:panel-cache"
        || selector == "panel-model-pending"
        // The composer's model chip: the only way to open the picker on a live session.
        || selector == "panel-model"
        || selector == "hover:panel-openrig"
        || selector == "hover:panel-context-meter"
        || selector.starts_with("sidebar-worker:")
        || selector.starts_with("sidebar-todos:")
        || matches!(
            selector,
            "sidebar-sort-agent-activity"
                | "sidebar-sort-recent"
                | "sidebar-origin-all"
                | "sidebar-origin-mine"
                | "sidebar-origin-agents"
                | "resume-sort-agent-activity"
                | "resume-sort-recent"
                | "resume-origin-all"
                | "resume-origin-mine"
                | "resume-origin-agents"
        )
        || selector.starts_with("routing-gate-row-")
        || selector == "routing-chip"
        || selector.starts_with("status-worker:")
        || selector.starts_with("background-job:")
        || selector.starts_with("agent-message:")
        || selector == "side-question-promote"
        || (side_questions() && selector.starts_with("side-question-"))
        || matches!(selector, "sidebar-resize-drag" | "sidebar-resize-up")
        || (ask_fixture()
            && matches!(selector, "ask-resize-drag" | "ask-resize-up" | "ask-resize-reset"))
        || ((voice_recording() || voice_soniox()) && selector == "composer.voice")
        || (model_speed_picker()
            && (selector.starts_with("model-speed:")
                || matches!(
                    selector,
                    "panel-model-effort" | "panel-model-speed"
                )))
        || (draft_session()
            && (selector.starts_with("draft-model-row:")
                || matches!(
                    selector,
                    "composer-location"
                        | "tab-new-session"
                        | "folder-picker-entry-0"
                        | "folder-picker-open"
                        | "panel-mode"
                        | "panel-model-effort"
                        | "draft-effort-high"
                )))
        || selector.starts_with("proof-pointer:")
        || matches!(
            selector,
            "panel-context-meter"
                | "context-usage-compact"
                | "context-usage-shake"
                | "context-usage-shake-confirm"
                | "context-usage-shake-cancel"
        )
        || (background_tasks() && selector.starts_with("task:"))
        || (resume_preview()
            && (selector.starts_with("resume-row:")
                || selector.starts_with("resume-action:")
                || selector == "resume-preview:wheel"))
        || matches!(selector, "compaction-divider" | "history:scroll-up")
        || (context_overlay_fixture() && selector == "scroll:transcript")
        || (gallery_preload()
            && (matches!(selector, "composer:images" | "gallery:wheel")
                || selector.starts_with("gallery:thumb:")))
        || (gallery_navigation()
            && matches!(
                selector,
                "gallery-nav:lines" | "gallery-nav:first" | "gallery-nav:last"
                    | "gallery-nav:gap" | "gallery-nav:picture" | "gallery-nav:history"
            ))
        || (transcript_scroll_fixture()
            && matches!(
                selector,
                "prompt:toggle"
                    | "scroll:tool"
                    | "scroll:momentum"
                    | "scroll:transcript"
                    | "scroll:pin-prompt"
                    | "composer:images"
                    | "composer:resize-drag"
                    | "composer:resize-reset"
                    | "composer:wheel-up"
                    | "composer:wheel-down"
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
    } else if selector == "click:panel-openrig" {
        "hover:panel-openrig"
    } else if selector == "hover:panel-context-meter" {
        "panel-context-meter"
    } else if matches!(selector, "hover:clean-chat-on" | "hover:clean-chat-off") {
        selector.strip_prefix("hover:").unwrap()
    } else if matches!(selector, "composer:resize-drag" | "composer:resize-reset") {
        "composer:resize"
    } else if matches!(selector, "sidebar-resize-drag" | "sidebar-resize-up") {
        "sidebar-resize"
    } else if matches!(selector, "side-question-resize-up" | "side-question-resize-down") {
        "side-question-resize"
    } else if matches!(selector, "ask-resize-drag" | "ask-resize-up" | "ask-resize-reset") {
        "ask-resize"
    } else if matches!(
        selector,
        "gallery:wheel" | "gallery-nav:lines" | "gallery-nav:first" | "gallery-nav:last"
    ) {
        "gallery:filmstrip"
    } else {
        selector
    };
    let Some(position) = locate(marker, session, window, cx) else {
        return false;
    };
    if let Some(held) = selector.strip_prefix("sidebar-resize-") {
        // Grab the handle level with the docked composer, beside the usage
        // strip, and drag it right; `up` releases where the handle now is.
        let grab = point(position.x, window.viewport_size().height - px(40.));
        let mouse_down = |window: &mut Window, cx: &mut App| {
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: grab,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            window.refresh();
            window.draw(cx).clear(cx);
        };
        if held == "drag" {
            mouse_down(window, cx);
            window.dispatch_event(
                PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position: grab + point(px(80.), px(0.)),
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                }),
                cx,
            );
        } else {
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: grab,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        }
        return true;
    }
    if selector == "gallery:wheel" {
        window.dispatch_event(
            PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(point(px(-3_900.), px(0.))),
                touch_phase: gpui::TouchPhase::Moved,
                modifiers: Default::default(),
            }),
            cx,
        );
        return true;
    }
    if gallery_navigation() && marker == "gallery:filmstrip" && selector.starts_with("gallery-nav:") {
        let wheel = |delta, touch_phase, window: &mut Window, cx: &mut App| {
            window.dispatch_event(
                PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    delta,
                    modifiers: Default::default(),
                    touch_phase,
                }),
                cx,
            );
        };
        use gpui::{ScrollDelta, TouchPhase};
        if selector == "gallery-nav:lines" {
            wheel(ScrollDelta::Lines(point(0., -4.)), TouchPhase::Moved, window, cx);
        } else {
            let direction = if selector == "gallery-nav:first" { 1. } else { -1. };
            for (phase, pixels) in [
                (TouchPhase::Started, 12_000.),
                (TouchPhase::Moved, 400.),
                (TouchPhase::Ended, 80.),
            ] {
                let delta = point(px(direction * pixels), px(direction * pixels / 2.));
                wheel(ScrollDelta::Pixels(delta), phase, window, cx);
            }
        }
        return true;
    }
    if selector.starts_with("hover:") {
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
        "resume-preview:wheel" => Some((320., gpui::TouchPhase::Moved)),
        "scroll:tool" => Some((-12_000., gpui::TouchPhase::Started)),
        "scroll:momentum" => Some((-120., gpui::TouchPhase::Moved)),
        "scroll:transcript" => Some((-120., gpui::TouchPhase::Started)),
        "scroll:pin-prompt" => Some((-12_000., gpui::TouchPhase::Started)),
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
    let click_count = if matches!(selector, "composer:resize-reset" | "ask-resize-reset") {
        2
    } else {
        1
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count,
            ..Default::default()
        }),
        cx,
    );
    let drag_y = match selector {
        "composer:resize-drag" => Some(position.y - px(50.)),
        "side-question-resize-up" | "ask-resize-up" => Some(px(0.)),
        "ask-resize-drag" => Some(position.y - px(140.)),
        "side-question-resize-down" => Some(window.viewport_size().height - px(1.)),
        _ => None,
    };
    let release_position = if let Some(y) = drag_y {
        let position = point(position.x, y);
        window.dispatch_event(
            PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            }),
            cx,
        );
        position
    } else {
        position
    };
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: release_position,
            click_count,
            ..Default::default()
        }),
        cx,
    );
    true
}

pub(crate) fn locate(
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
