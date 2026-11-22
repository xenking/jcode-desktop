use super::*;
use gpui::{DispatchPhase, ScrollHandle, canvas};
use std::cell::Cell;
use std::ops::RangeInclusive;
use std::rc::Rc;

#[derive(Clone, Default)]
pub(crate) struct VerticalResize(Rc<Cell<ResizeState>>);

#[derive(Clone, Copy, Default)]
struct ResizeState {
    height: Option<Pixels>,
    drag: Option<ResizeDrag>,
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    y: Pixels,
    height: Pixels,
}

impl VerticalResize {
    pub(crate) fn height(&self, bounds: RangeInclusive<Pixels>) -> Pixels {
        self.0
            .get()
            .height
            .unwrap_or(*bounds.start())
            .clamp(*bounds.start(), *bounds.end())
    }

    fn set_height(&self, height: Pixels, bounds: RangeInclusive<Pixels>) {
        let height = height.clamp(*bounds.start(), *bounds.end());
        let mut state = self.0.get();
        state.height = (height > *bounds.start()).then_some(height);
        self.0.set(state);
    }

    pub(crate) fn handle<T: 'static>(
        &self,
        id: &'static str,
        control_id: &'static str,
        scroll: &ScrollHandle,
        bounds: RangeInclusive<Pixels>,
        cx: &Context<T>,
    ) -> impl IntoElement {
        let owner = cx.entity().entity_id();
        let resize = self.clone();
        let pressed = self.clone();
        let scroll = scroll.clone();
        div()
            .relative()
            .flex()
            .justify_center()
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        let moving = resize.clone();
                        let bounds = bounds.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let mut state = moving.0.get();
                            if let Some(drag) = state.drag {
                                if event.dragging() {
                                    moving.set_height(
                                        drag.height + drag.y - event.position.y,
                                        bounds.clone(),
                                    );
                                    cx.notify(owner);
                                } else {
                                    state.drag = None;
                                    moving.0.set(state);
                                }
                                cx.stop_propagation();
                            }
                        });
                        let released = resize.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture || event.button != MouseButton::Left
                            {
                                return;
                            }
                            let mut state = released.0.get();
                            if state.drag.take().is_some() {
                                released.0.set(state);
                                cx.stop_propagation();
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id(id)
                    .debug_selector(move || id.into())
                    .w(px(60.))
                    .h(px(9.))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || control_id.into(),
                        || None,
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::ResizeUpDown)
                    .child(
                        div()
                            .w(px(32.))
                            .h(px(3.))
                            .rounded_full()
                            .bg(Theme::global().TEXT_FAINT),
                    )
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        let mut state = pressed.0.get();
                        if event.click_count == 2 {
                            state = ResizeState::default();
                        } else {
                            state.drag = Some(ResizeDrag {
                                y: event.position.y,
                                height: scroll.bounds().size.height,
                            });
                        }
                        pressed.0.set(state);
                        cx.notify(owner);
                    }),
            )
    }
}

impl PromptInput {
    fn automatic_editor_height(&self, window: &Window) -> Pixels {
        let viewport = f32::from(window.viewport_size().height);
        px(if self.queue_editor {
            (viewport * 0.4).clamp(28., 280.)
        } else {
            (viewport * 0.25).clamp(28., 160.)
        })
    }

    pub(super) fn editor_height(&self, window: &Window) -> Pixels {
        self.editor_resize.height(self.editor_height_bounds(window))
    }

    fn editor_height_bounds(&self, window: &Window) -> RangeInclusive<Pixels> {
        let automatic = self.automatic_editor_height(window);
        automatic..=(window.viewport_size().height * 0.7).max(automatic)
    }

    pub(super) fn can_resize_editor(&self, window: &Window) -> bool {
        !self.queue_editor
            && !self.compact_padding
            && self.picker_prefix.is_none()
            && !self.content.is_empty()
            && self.last_line_height.is_some_and(|line| {
                line * self.visual_line_count as f32 > self.automatic_editor_height(window)
            })
    }

    fn set_editor_height(&mut self, height: Pixels, window: &Window, cx: &mut Context<Self>) {
        self.editor_resize
            .set_height(height, self.editor_height_bounds(window));
        cx.notify();
    }

    pub(super) fn grow_editor(
        &mut self,
        _: &GrowEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.can_resize_editor(window) {
            self.set_editor_height(self.editor_height(window) + px(32.), window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn shrink_editor(
        &mut self,
        _: &ShrinkEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.can_resize_editor(window) {
            self.set_editor_height(self.editor_height(window) - px(32.), window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn render_editor_resize(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .child(self.editor_resize.handle(
                "prompt-resize-handle",
                "composer:resize",
                &self.editor_scroll,
                self.editor_height_bounds(window),
                cx,
            ))
    }
}
