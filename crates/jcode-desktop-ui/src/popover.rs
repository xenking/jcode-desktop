use gpui::{App, Bounds, DispatchPhase, MouseUpEvent, Pixels, Window};

/// Close after dispatch, so a trigger's click can toggle the existing popover
/// and a control outside can claim focus before the popover is removed.
pub(crate) fn dismiss_outside<const N: usize>(
    window: &mut Window,
    regions: [Bounds<Pixels>; N],
    dismiss: impl Fn(&mut Window, &mut App) + Clone + 'static,
) {
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture
            && !window.has_active_prompt()
            && !regions.iter().any(|bounds| bounds.contains(&event.position))
        {
            let dismiss = dismiss.clone();
            window.defer(cx, move |window, cx| dismiss(window, cx));
        }
    });
}
