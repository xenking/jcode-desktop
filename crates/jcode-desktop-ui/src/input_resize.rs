use super::*;
use gpui::DispatchPhase;

#[derive(Default)]
pub(super) struct EditorResize {
    height: Option<Pixels>,
    drag: Option<ResizeDrag>,
}

#[derive(Clone, Copy)]
struct ResizeDrag {
    y: Pixels,
    height: Pixels,
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
        let automatic = self.automatic_editor_height(window);
        self.editor_resize.height.map_or(automatic, |height| {
            height.clamp(automatic, self.editor_height_limit(window))
        })
    }

    fn editor_height_limit(&self, window: &Window) -> Pixels {
        (window.viewport_size().height * 0.7).max(self.automatic_editor_height(window))
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
        let automatic = self.automatic_editor_height(window);
        let height = height.clamp(automatic, self.editor_height_limit(window));
        self.editor_resize.height = (height > automatic).then_some(height);
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

    pub(super) fn render_editor_resize(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input = cx.entity().downgrade();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |_, (), window, _| {
                        let moving = input.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let Some(entity) = moving.upgrade() else {
                                return;
                            };
                            if let Some(drag) = entity.read(cx).editor_resize.drag {
                                entity.update(cx, |input, cx| {
                                    if event.dragging() {
                                        input.set_editor_height(
                                            drag.height + drag.y - event.position.y,
                                            window,
                                            cx,
                                        );
                                    } else {
                                        input.editor_resize.drag = None;
                                    }
                                });
                                cx.stop_propagation();
                            }
                        });
                        let released = input.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture || event.button != MouseButton::Left
                            {
                                return;
                            }
                            if let Some(entity) = released.upgrade()
                                && entity.read(cx).editor_resize.drag.is_some()
                            {
                                entity.update(cx, |input, _| input.editor_resize.drag = None);
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
                    .id("prompt-resize-handle")
                    .debug_selector(|| "prompt-resize-handle".into())
                    .w(px(60.))
                    .h(px(9.))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || "composer:resize".into(),
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
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            if event.click_count == 2 {
                                this.editor_resize = EditorResize::default();
                            } else {
                                this.editor_resize.drag = Some(ResizeDrag {
                                    y: event.position.y,
                                    height: this.editor_scroll.bounds().size.height,
                                });
                            }
                            cx.notify();
                        }),
                    ),
            )
    }
}
