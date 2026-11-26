//! Inline image chips in completed ask answers.
use super::*;
use gpui::{HighlightStyle, InteractiveText, StyledText};

impl Panel {
    pub(super) fn render_ask_answer_part(
        &self,
        call_id: &str,
        shown: String,
        image_offset: usize,
        show_extras: bool,
        selection: Option<(Entity<TextSelection>, String)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let images: Vec<_> = self
            .ask_result_images(call_id)
            .into_iter()
            .map(|(_, image)| image.clone())
            .collect();
        let ranges = crate::image_chip::image_marker_ranges(&shown);
        let selected: Vec<Option<usize>> = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| {
                let marker = &shown[range.clone()];
                if let Some(id) = marker
                    .strip_prefix("[#")
                    .and_then(|s| s.strip_suffix(']'))
                    .filter(|s| s.starts_with("img_"))
                {
                    images.iter().position(|image| image.has_image_id(id))
                } else {
                    (image_offset + index < images.len()).then_some(image_offset + index)
                }
            })
            .collect();
        let display = crate::image_chip::mask_markers(&shown, ranges.iter().cloned());
        let mut highlights: Vec<_> = ranges
            .iter()
            .cloned()
            .map(|range| {
                (
                    range,
                    HighlightStyle {
                        color: Some(gpui::transparent_black()),
                        ..Default::default()
                    },
                )
            })
            .collect();
        if let Some((model, key)) = &selection
            && let Some(highlight) = model.read(cx).highlight(key, shown.len())
        {
            highlights.push(highlight);
        }
        let styled = StyledText::new(display).with_highlights(highlights);
        let layout = styled.layout().clone();
        let chips = ranges
            .iter()
            .enumerate()
            .map(|(index, range)| {
                (
                    range.clone(),
                    selected[index].and_then(|i| images.get(i))
                        .and_then(TranscriptImage::image_ordinal)
                        .or_else(|| self.gallery_image_ordinal(&shown[range.clone()]))
                        .map_or_else(|| shown[range.clone()].to_owned(), |number| format!("[#image {number}]")),
                    selected[index]
                        .and_then(|i| images.get(i))
                        .and_then(|image| image.preview.clone()),
                )
            })
            .collect();
        let weak = cx.weak_entity();
        let click_images: Vec<_> = selected
            .iter()
            .map(|index| index.and_then(|i| images.get(i).cloned()))
            .collect();
        let text = InteractiveText::new(format!("ask-answer-{call_id}"), styled).on_click(
            ranges.clone(),
            move |index, window, cx| {
                if let Some(Some(image)) = click_images.get(index) {
                    let _ = weak.update(cx, |panel, cx| {
                        panel.open_image_preview(image.clone(), window, cx)
                    });
                }
            },
        );
        let text = if let Some((model, key)) = selection {
            text_selection::selectable(model, key, shown.clone(), layout.clone(), text, cx)
        } else {
            text.into_any_element()
        };
        let extras = images
            .into_iter()
            .enumerate()
            .skip(image_offset)
            .filter(|(index, _)| !selected.contains(&Some(*index)))
            .filter(|_| show_extras)
            .map(|(index, image)| {
                let label = image.image_ordinal().map(|number| format!("[#image {number}]")).or_else(|| image
                    .data
                    .strip_prefix("jcode-lazy:")
                    .filter(|id| id.starts_with("img_"))
                    .map(|id| format!("[#{id}]"))
                    .or_else(|| {
                        image.label.as_ref().and_then(|text| {
                            crate::image_chip::image_marker_ranges(text)
                                .into_iter()
                                .map(|range| text[range].to_owned())
                                .find(|marker| marker.starts_with("[#img_"))
                        })
                    }))
                    .unwrap_or_else(|| format!("[#image {}]", index + 1));
                crate::image_chip::chip(
                    image.preview.clone(),
                    &label,
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
