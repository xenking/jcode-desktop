use gpui::{BoxShadow, Div, FocusHandle, InteractiveElement, Stateful, StyleRefinement, Styled, px};

use crate::theme::Theme;

fn ring(style: StyleRefinement) -> StyleRefinement {
    style.shadow(vec![BoxShadow::new(px(0.), px(0.), Theme::global().TEXT.into())
        .spread_radius(px(2.))
        .inset()])
}

/// GPUI's stateful div supplies a stable handle and turns Enter and Space
/// into the same `on_click` event as a pointer.
pub(crate) fn button(element: Stateful<Div>) -> Stateful<Div> {
    element.tab_index(0).focus_visible(ring)
}

pub(crate) fn tracked_button(element: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    button(element.track_focus(handle))
}

pub(crate) fn popover_trigger(element: Stateful<Div>, handle: &FocusHandle) -> Stateful<Div> {
    tracked_button(element, handle).on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
        // Wait for the click handler. Moving focus on the down event would
        // close the popover before its trigger could toggle it on release.
        window.prevent_default();
    })
}

pub(crate) fn tab_direction(event: &gpui::KeyDownEvent) -> Option<bool> {
    let key = &event.keystroke;
    let mods = key.modifiers;
    (key.key == "tab" && !(mods.control || mods.alt || mods.platform || mods.function))
        .then_some(mods.shift)
}
