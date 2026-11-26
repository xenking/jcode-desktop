use gpui::{
    AnyElement, App, Bounds, Canvas, DispatchPhase, Div, Element, ElementId, Global,
    GlobalElementId, Hitbox, HitboxBehavior, HitboxId, InspectorElementId, IntoElement, LayoutId,
    Pixels, RenderOnce, ScrollHandle, ScrollWheelEvent, Stateful, Styled, Window, canvas,
    prelude::StatefulInteractiveElement, px,
};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct WheelCapture(Option<bool>);

impl Global for WheelCapture {}

/// The transcript smooths wheel input in capture, before GPUI's List scrolls
/// in bubble. Bracket its children in paint order so their live hitboxes can
/// claim the event first, without keeping a registry of virtualized rows.
pub(crate) fn capture_root() -> (Canvas<HitboxId>, Rc<Cell<Option<HitboxId>>>) {
    let hitbox = Rc::new(Cell::new(None));
    let root = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal).id,
        {
            let hitbox = hitbox.clone();
            move |_, id, window, _| {
                hitbox.set(Some(id));
                window.on_mouse_event(|_: &ScrollWheelEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        cx.default_global::<WheelCapture>().0 = Some(false);
                    }
                });
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    (root, hitbox)
}

pub(crate) fn end_capture(cx: &mut App) -> bool {
    cx.default_global::<WheelCapture>().0.take() == Some(true)
}

pub(crate) fn vertical(div: Stateful<Div>) -> ScrollRegion {
    ScrollRegion {
        div,
        handle: None,
        horizontal: false,
    }
}

pub(crate) fn tracked_vertical(div: Stateful<Div>, handle: &ScrollHandle) -> ScrollRegion {
    ScrollRegion {
        div,
        handle: Some(handle.clone()),
        horizontal: false,
    }
}

pub(crate) fn horizontal(div: Stateful<Div>) -> ScrollRegion {
    ScrollRegion {
        div,
        handle: None,
        horizontal: true,
    }
}

#[derive(IntoElement)]
pub(crate) struct ScrollRegion {
    div: Stateful<Div>,
    handle: Option<ScrollHandle>,
    horizontal: bool,
}

impl RenderOnce for ScrollRegion {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let handle = self.handle.unwrap_or_else(|| {
            let id = Element::id(&self.div).expect("scroll region with element ID");
            window.with_global_id(id, |id, window| {
                window.with_element_state(id, |handle: Option<ScrollHandle>, _| {
                    let handle = handle.unwrap_or_default();
                    (handle.clone(), handle)
                })
            })
        });
        let div = if self.horizontal {
            self.div.overflow_x_scroll()
        } else {
            self.div.overflow_y_scroll()
        };
        WheelRegion {
            child: div.track_scroll(&handle).into_any_element(),
            handle,
            horizontal: self.horizontal,
        }
    }
}

struct WheelRegion {
    child: AnyElement,
    handle: ScrollHandle,
    horizontal: bool,
}

impl IntoElement for WheelRegion {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for WheelRegion {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.child.prepaint(window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let handle = self.handle.clone();
        let hitbox = hitbox.id;
        let horizontal = self.horizontal;
        // Register before painting the div. In bubble, GPUI applies its wheel
        // delta first, then we stop ancestors even at the edge. A nested region
        // deeper in the tree gets both its scroll and its stop before this one.
        window.on_mouse_event(move |_: &ScrollWheelEvent, phase, window, cx| {
            if !hitbox.should_handle_scroll(window) {
                return;
            }
            let max = handle.max_offset();
            if (if horizontal { max.x } else { max.y }) <= px(0.) {
                return;
            }
            match phase {
                DispatchPhase::Capture => {
                    if let Some(claimed) = &mut cx.default_global::<WheelCapture>().0 {
                        *claimed = true;
                    }
                }
                DispatchPhase::Bubble => cx.stop_propagation(),
            }
        });
        self.child.paint(window, cx);
    }
}
