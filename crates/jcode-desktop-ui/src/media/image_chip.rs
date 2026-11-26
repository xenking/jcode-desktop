//! One visual treatment for inline image attachments in every text surface.
use crate::theme::Theme;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, Image,
    InspectorElementId, IntoElement, LayoutId, Pixels, Position, Style, TextLayout, Window, div,
    img, prelude::*, px, relative, size,
};
use std::{borrow::Cow, ops::Range, rc::Rc, sync::Arc};

/// Where the image markers (`[#image 3]`, `[#img_<hash>]`) sit in `content`.
pub(crate) fn image_marker_ranges(content: &str) -> Vec<Range<usize>> {
    content
        .match_indices("[#")
        .filter_map(|(start, _)| {
            let rest = &content[start..];
            let end = rest.find(']')?;
            let marker = &rest[2..end];
            let valid = marker.strip_prefix("image ").is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            }) || marker.strip_prefix("img_").is_some_and(|hash| {
                hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit())
            });
            valid.then_some(start..start + end + 1)
        })
        .collect()
}

/// Replace marker glyphs while preserving byte offsets used by selection and IME.
pub(crate) fn mask_markers(text: &str, ranges: impl IntoIterator<Item = Range<usize>>) -> String {
    let mut display = text.to_owned();
    for range in ranges {
        display.replace_range(range.clone(), &"n".repeat(range.len()));
    }
    display
}

pub(crate) const UNKNOWN_LABEL: &str = "[Image]";
const LABEL_SIZE: f32 = 9.5;

/// Reserve a single unbreakable word in the surrounding text run.
/// The label, thumbnail and padding determine the width, never the marker ID.
pub(crate) fn transcript_spacer(
    label: &str,
    font: gpui::Font,
    font_size: Pixels,
    window: &Window,
) -> String {
    let shape = |s: &str, font, size| {
        window
            .text_system()
            .shape_line(
                s.to_owned().into(),
                size,
                &[gpui::TextRun {
                    len: s.len(),
                    font,
                    ..Default::default()
                }],
                None,
            )
            .width
    };
    let target = shape(label, gpui::font(Theme::global().FONT_UI), px(LABEL_SIZE)) + px(22.);
    let wide = f32::from(shape("n", font.clone(), font_size)).max(1.);
    let narrow = f32::from(shape("i", font, font_size)).max(1.);
    let target = f32::from(target);
    let baseline = (target / wide).floor() as usize;
    let (n, i) = (0..=baseline + 1)
        .map(|n| {
            let i = ((target - n as f32 * wide) / narrow).ceil().max(0.) as usize;
            (n, i)
        })
        .min_by(|(an, ai), (bn, bi)| {
            (wide * *an as f32 + narrow * *ai as f32)
                .total_cmp(&(wide * *bn as f32 + narrow * *bi as f32))
        })
        .unwrap_or_default();
    format!("{}{}", "n".repeat(n), "i".repeat(i))
}

pub(crate) fn session_image_key(marker: &str) -> Option<&str> {
    marker
        .strip_prefix("[#")?
        .strip_suffix(']')
        .filter(|id| id.starts_with("img_"))
}

pub(super) fn ordinal_fixture_ids() -> [String; 2] {
    std::env::var("JCODE_DESKTOP_SCREENSHOT_ORDINAL_IDS")
        .ok()
        .and_then(|json| serde_json::from_str::<[String; 2]>(&json).ok())
        .filter(|ids| {
            ids[0] != ids[1]
                && ids.iter().all(|id| {
                    id.strip_prefix("img_").is_some_and(|hash| {
                        hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                })
        })
        .unwrap_or_else(|| {
            [
                "img_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                "img_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ]
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn attachment_label_is_numbered_image() {
        assert_eq!(super::label("[#image 1]"), "[Image #1]");
        assert_eq!(super::label("[#image 12]"), "[Image #12]");
    }

    #[test]
    fn numbered_text_keeps_prose_and_numbers_attachments_by_position() {
        let (text, markers) = super::numbered_text(
            "До [#image 12](так)\nПосле [#img_0123456789abcdef0123456789abcdef]",
            3,
        );
        assert_eq!(text, "До [#image 1](так)\nПосле [#image 2] [#image 3]");
        assert_eq!(markers, ["[#image 1]", "[#image 2]", "[#image 3]"]);
        assert_eq!(super::numbered_text("[image]", 1).0, "[#image 1]");
        assert_eq!(super::numbered_text("[image]", 0).0, "[image]");
    }

    #[test]
    fn masked_markers_preserve_offsets_without_visible_marker_glyphs() {
        let source = "café [#image 1] and [#image 22]";
        let ranges = super::image_marker_ranges(source);
        let masked = super::mask_markers(source, ranges.clone());
        assert_eq!(masked.len(), source.len());
        assert!(!masked.contains("[#image"));
        assert!(!masked.contains(']'));
        assert_eq!(&masked[..ranges[0].start], "café ");
        assert_eq!(&masked[ranges[1].end..], "");
    }
}

/// Normalize display text before shaping, so a storage ID takes up no more
/// room than a numbered marker. The original prompt and wire IDs are untouched.
pub(crate) fn numbered_text(text: &str, image_count: usize) -> (Cow<'_, str>, Vec<String>) {
    let text = if image_count != 0 && text.trim() == "[image]" {
        ""
    } else {
        text
    };
    let ranges = image_marker_ranges(text);
    let markers: Vec<_> = (1..=ranges.len().max(image_count))
        .map(|number| format!("[#image {number}]"))
        .collect();
    if markers.is_empty() {
        return (Cow::Borrowed(text), markers);
    }
    let mut shown = String::with_capacity(text.len());
    let mut offset = 0;
    for (range, marker) in ranges.iter().zip(&markers) {
        shown.push_str(&text[offset..range.start]);
        shown.push_str(marker);
        offset = range.end;
    }
    shown.push_str(&text[offset..]);
    for marker in &markers[ranges.len()..] {
        if !shown.is_empty() && !shown.ends_with(char::is_whitespace) {
            shown.push(' ');
        }
        shown.push_str(marker);
    }
    (Cow::Owned(shown), markers)
}

pub(crate) fn numbered_label(number: usize) -> String {
    format!("[Image #{number}]")
}

pub(crate) fn hidden_marker() -> gpui::HighlightStyle {
    gpui::HighlightStyle {
        fade_out: Some(1.),
        ..Default::default()
    }
}

pub(crate) fn label(marker: &str) -> String {
    if let Some(number) = marker_number(marker) {
        return numbered_label(number);
    }
    marker
        .strip_prefix("[#")
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(marker)
        .to_owned()
}

pub(crate) fn prompt_label(marker: &str, ordinal: Option<usize>) -> String {
    ordinal
        .or_else(|| marker_number(marker))
        .map_or_else(|| UNKNOWN_LABEL.to_owned(), numbered_label)
}

pub(crate) fn marker_number(marker: &str) -> Option<usize> {
    marker
        .strip_prefix("[#image ")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

pub(crate) fn chip(
    image: Option<Arc<Image>>,
    marker: &str,
    chip_selector: &'static str,
    image_selector: &'static str,
    label_selector: &'static str,
) -> gpui::Div {
    chip_with_label(
        image,
        label(marker),
        chip_selector,
        image_selector,
        label_selector,
    )
}

pub(crate) fn chip_with_label(
    image: Option<Arc<Image>>,
    label: String,
    chip_selector: &'static str,
    image_selector: &'static str,
    label_selector: &'static str,
) -> gpui::Div {
    let missing = image.is_none();
    div()
        .debug_selector(move || chip_selector.into())
        .rounded_full()
        .bg(Theme::global().USER_BG)
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(2.))
        .overflow_hidden()
        .children(image.map(|image| {
            img(crate::image_cache::source(image))
                .debug_selector(move || image_selector.into())
                .flex_none()
                .size(px(14.))
                .rounded_sm()
                .object_fit(gpui::ObjectFit::Cover)
        }))
        .when(missing, |chip| chip.child(
            div().debug_selector(|| "image-chip-placeholder".into())
                .flex_none().size(px(14.)).text_size(px(12.))
                .text_color(Theme::global().TEXT_DIM).child("▧"),
        ))
        .child(
            div()
                .debug_selector(move || label_selector.into())
                .flex_none()
                .whitespace_nowrap()
                .font_family(Theme::global().FONT_UI)
                .text_size(px(LABEL_SIZE))
                .child(label),
        )
}

#[cfg(test)]
thread_local! {
    static PAINTED_THUMBNAILS: std::cell::RefCell<Vec<Bounds<Pixels>>> = const { std::cell::RefCell::new(Vec::new()) };
    static TRANSCRIPT_CHIPS: std::cell::RefCell<Vec<(String, Bounds<Pixels>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn take_painted_thumbnails() -> Vec<Bounds<Pixels>> {
    PAINTED_THUMBNAILS.with(|painted| std::mem::take(&mut *painted.borrow_mut()))
}

#[cfg(test)]
pub(crate) fn take_transcript_chips() -> Vec<(String, Bounds<Pixels>)> {
    TRANSCRIPT_CHIPS.with(|chips| std::mem::take(&mut *chips.borrow_mut()))
}

pub(crate) type PromptImageClick = Rc<dyn Fn(&str, &mut Window, &mut App)>;

struct ChipActions {
    leaf: gpui::SharedString,
    keys: Vec<String>,
    on_click: PromptImageClick,
}

/// Paint real GPUI chips over the corresponding transparent text runs, using
/// the already-shaped text layout so links and selection keep their offsets.
pub(crate) struct PositionedChips {
    layout: TextLayout,
    chips: Vec<(Range<usize>, String, Option<Arc<Image>>)>,
    children: Vec<AnyElement>,
    selectors: (&'static str, &'static str, &'static str),
    actions: Option<ChipActions>,
    #[cfg(test)]
    thumbnails: Vec<(Arc<Image>, Bounds<Pixels>)>,
}
impl PositionedChips {
    pub(crate) fn new(
        layout: TextLayout,
        chips: Vec<(Range<usize>, String, Option<Arc<Image>>)>,
        leaf: gpui::SharedString,
        keys: Vec<String>,
        on_click: PromptImageClick,
    ) -> Self {
        let mut positioned = Self::from_labels(
            layout,
            chips,
            (
                "transcript-image-chip",
                "transcript-image-thumbnail",
                "transcript-image-label",
            ),
        );
        positioned.actions = Some(ChipActions { leaf, keys, on_click });
        positioned
    }

    pub(crate) fn with_selectors(
        layout: TextLayout,
        chips: Vec<(Range<usize>, String, Option<Arc<Image>>)>,
        selectors: (&'static str, &'static str, &'static str),
    ) -> Self {
        Self::from_labels(
            layout,
            chips
                .into_iter()
                .map(|(range, marker, image)| (range, label(&marker), image))
                .collect(),
            selectors,
        )
    }

    fn from_labels(
        layout: TextLayout,
        chips: Vec<(Range<usize>, String, Option<Arc<Image>>)>,
        selectors: (&'static str, &'static str, &'static str),
    ) -> Self {
        Self {
            layout,
            chips,
            selectors,
            actions: None,
            children: Vec::new(),
            #[cfg(test)]
            thumbnails: Vec::new(),
        }
    }
}
impl IntoElement for PositionedChips {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PositionedChips {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        self.actions.as_ref().map(|actions| actions.leaf.clone().into())
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
        let style = Style {
            position: Position::Absolute,
            size: size(relative(1.).into(), relative(1.).into()),
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        for (index, (range, marker, image)) in self.chips.iter().enumerate() {
            let (Some(mut start), Some(end)) = (
                self.layout.position_for_index(range.start),
                self.layout.position_for_index(range.end),
            ) else {
                continue;
            };
            if start.y != end.y
                && let (Some(next), Some(after)) = (
                    self.layout.position_for_index(range.start + 1),
                    self.layout.position_for_index(range.start + 2),
                )
                && next.y == end.y
                && after.y == next.y
            {
                start = gpui::point(2. * next.x - after.x, next.y);
            }
            if start.y != end.y
                && let (Some(next), Some(after)) = (
                    self.layout.position_for_index(range.start + 2),
                    self.layout.position_for_index(range.start + 3),
                )
                && next.y == end.y && after.y == end.y
            {
                start = gpui::point(next.x - (after.x - next.x), next.y);
            }
            if start.y != end.y {
                continue;
            }
            // Leave a little breathing room even when the marker touches text
            // or another marker in the stored message.
            let width = (end.x - start.x - px(2.)).max(px(1.));
            start.x += px(1.);
            #[cfg(test)]
            if self.selectors.0 == "transcript-image-chip" {
                let bounds = Bounds::new(start, size(width, window.line_height()));
                TRANSCRIPT_CHIPS.with(|chips| chips.borrow_mut().push((marker.clone(), bounds)));
            }
            let chip = chip_with_label(
                image.clone(),
                marker.clone(),
                self.selectors.0,
                self.selectors.1,
                self.selectors.2,
            )
            .w(width)
            .h(window.line_height());
            let mut element = if let Some(actions) = &self.actions {
                let key = actions.keys[index].clone();
                let on_click = actions.on_click.clone();
                chip.id(index)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| on_click(&key, window, cx))
                    .into_any_element()
            } else {
                chip.into_any_element()
            };
            element.prepaint_as_root(
                start,
                size(
                    AvailableSpace::Definite(width),
                    AvailableSpace::Definite(window.line_height()),
                ),
                window,
                cx,
            );
            #[cfg(test)]
            if let Some(image) = image {
                let origin = start + gpui::point(px(2.), (window.line_height() - px(14.)) / 2.);
                self.thumbnails
                    .push((image.clone(), Bounds::new(origin, size(px(14.), px(14.)))));
            }
            self.children.push(element);
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        for child in &mut self.children {
            child.paint(window, cx);
        }
        #[cfg(test)]
        for (image, bounds) in &self.thumbnails {
            if let gpui::ImageSource::Custom(load) = crate::image_cache::source(image.clone())
                && matches!(load(window, cx), Some(Ok(_)))
            {
                PAINTED_THUMBNAILS.with(|painted| painted.borrow_mut().push(*bounds));
            }
        }
    }
}
