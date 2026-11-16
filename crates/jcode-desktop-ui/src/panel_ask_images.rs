//! Inline image chips in completed ask answers.
use super::*;
use gpui::{HighlightStyle, InteractiveText, StyledText};

impl Panel {
    pub(super) fn render_ask_answer(
        &self,
        call_id: &str,
        shown: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_ask_answer_part(call_id, shown, 0, true, cx)
    }

    pub(super) fn render_ask_answer_part(
        &self,
        call_id: &str,
        shown: String,
        image_offset: usize,
        show_extras: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let images: Vec<_> = self
            .ask_result_images(call_id)
            .into_iter()
            .skip(image_offset)
            .map(|(_, image)| image.clone())
            .collect();
        let ranges = crate::input::image_marker_ranges(&shown);
        let display = crate::image_chip::mask_markers(&shown, ranges.iter().cloned());
        let styled =
            StyledText::new(display).with_highlights(ranges.iter().cloned().map(|range| {
                (
                    range,
                    HighlightStyle {
                        color: Some(gpui::transparent_black()),
                        ..Default::default()
                    },
                )
            }));
        let layout = styled.layout().clone();
        let chips = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| {
                (
                    range.clone(),
                    shown[range.clone()].to_owned(),
                    images.get(index).and_then(|image| image.preview.clone()),
                )
            })
            .collect();
        let weak = cx.weak_entity();
        let click_images = images.clone();
        let text = InteractiveText::new(format!("ask-answer-{call_id}"), styled).on_click(
            ranges.clone(),
            move |index, window, cx| {
                if let Some(image) = click_images.get(index) {
                    let _ = weak.update(cx, |panel, cx| {
                        panel.open_image_preview(image.clone(), window, cx)
                    });
                }
            },
        );
        let extras = images
            .into_iter()
            .enumerate()
            .skip(ranges.len())
            .filter(|_| show_extras)
            .map(|(index, image)| {
                crate::image_chip::chip(
                    image.preview.clone(),
                    &format!("[#image {}]", index + 1),
                    "ask-result-image",
                    "ask-result-image-thumbnail",
                    "ask-result-image-label",
                )
                .id(("ask-result-image", index))
                .cursor_pointer()
                .on_click(cx.listener(move |panel, _, window, cx| {
                    panel.open_image_preview(image.clone(), window, cx);
                    cx.stop_propagation();
                }))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .min_w_0()
                    .relative()
                    .text_size(px(13.))
                    .child(text)
                    .child(crate::image_chip::PositionedChips::with_selectors(
                        layout,
                        chips,
                        (
                            "ask-result-inline-image",
                            "ask-result-inline-thumbnail",
                            "ask-result-inline-label",
                        ),
                    )),
            )
            .child(div().flex().flex_wrap().gap_1().children(extras))
            .into_any_element()
    }
}
