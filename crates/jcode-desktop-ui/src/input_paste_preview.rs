//! Full-size preview for a composer image chip.
use super::*;
use std::{cell::Cell, rc::Rc};

pub(super) type MeasuredBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

pub(super) struct Preview {
    pub index: usize,
}

impl Preview {
    pub fn new(index: usize) -> Self {
        Self { index }
    }
}

pub(super) fn bounds_marker(bounds: MeasuredBounds) -> gpui::AnyElement {
    gpui::canvas(|_, _, _| (), move |rect, _, _, _| bounds.set(Some(rect)))
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
}

impl PromptInput {
    pub(crate) fn paste_preview_panel_marker(&self) -> gpui::AnyElement {
        bounds_marker(self.preview_panel_bounds.clone())
    }

    pub(super) fn render_paste_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let preview = self.attachment_preview.as_ref()?;
        let image = self.attachments.get(preview.index)?.preview.clone();
        let viewport = window.viewport_size();
        let overlay = div()
            .id("paste-image-preview")
            .debug_selector(|| "paste-image-preview".into())
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(Theme::global().PANEL_BG)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(Theme::global().TEXT_DIM)
                    .child("Image preview")
                    .child(
                        div()
                            .id("paste-image-preview-close")
                            .debug_selector(|| "paste-image-preview-close".into())
                            .rounded_full()
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .hover(|style| style.bg(Theme::global().QUOTE_BG))
                            .child("Close ×")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.attachment_preview = None;
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                img(crate::image_cache::source(image))
                    .debug_selector(|| "paste-image-preview-content".into())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .object_fit(gpui::ObjectFit::Contain),
            );
        Some(
            gpui::deferred(
                gpui::anchored()
                    .position(point(px(0.), px(0.)))
                    .child(overlay),
            )
            .with_priority(90)
            .into_any_element(),
        )
    }
}
