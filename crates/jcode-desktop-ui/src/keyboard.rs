use gpui::{
    BoxShadow, Div, FocusHandle, InteractiveElement, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, px,
};

use crate::theme::Theme;

fn ring(style: StyleRefinement) -> StyleRefinement {
    style.shadow(vec![
        BoxShadow::new(px(0.), px(0.), Theme::global().TEXT.into())
            .spread_radius(px(2.))
            .inset(),
    ])
}

/// GPUI's stateful div supplies a stable handle and turns Enter and Space
/// into the same `on_click` event as a pointer.
pub(crate) fn button(element: Stateful<Div>) -> Stateful<Div> {
    element.tab_index(0).focus_visible(ring)
}

pub(crate) fn tracked_button(element: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    element
        .track_focus(&handle.clone().tab_index(0).tab_stop(true))
        .focus_visible(ring)
}

pub(crate) fn popover_trigger(element: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    tracked_button(element, handle).on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
        // Wait for the click handler. Moving focus on the down event would
        // close the popover before its trigger could toggle it on release.
        window.prevent_default();
    })
}

/// Preserve mouse-down behavior while giving the same action a keyboard click.
pub(crate) fn press_button(
    element: Stateful<Div>,
    activate: impl Fn(&mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> Stateful<Div> {
    let keyboard_activate = activate.clone();
    button(element)
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            activate(window, cx)
        })
        .on_click(move |event, window, cx| {
            if matches!(event, gpui::ClickEvent::Keyboard(_)) {
                keyboard_activate(window, cx);
            }
        })
}

pub(crate) fn tab_navigation(root: Div) -> Div {
    root.tab_group()
        .on_key_down(|event: &gpui::KeyDownEvent, window, cx| {
            if let Some(reverse) = tab_direction(event) {
                if reverse {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
                cx.stop_propagation();
            }
        })
}

pub(crate) fn arrow_peer(
    event: &gpui::KeyDownEvent,
    rows: &[FocusHandle],
    window: &gpui::Window,
    cx: &gpui::App,
) -> Option<FocusHandle> {
    if rows.is_empty() || event.keystroke.modifiers != gpui::Modifiers::default() {
        return None;
    }
    let current = rows.iter().position(|row| row.contains_focused(window, cx));
    let next = match event.keystroke.key.as_str() {
        "down" => current.map_or(0, |index| (index + 1) % rows.len()),
        "up" => current.map_or(rows.len() - 1, |index| {
            (index + rows.len() - 1) % rows.len()
        }),
        _ => return None,
    };
    Some(rows[next].clone())
}

fn tab_direction(event: &gpui::KeyDownEvent) -> Option<bool> {
    let key = &event.keystroke;
    let mods = key.modifiers;
    (key.key == "tab" && !(mods.control || mods.alt || mods.platform || mods.function))
        .then_some(mods.shift)
}
