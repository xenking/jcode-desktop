//! Fenced code: syntax highlighting, highlight flattening and the code block
//! element.
//!
//! This sits below the markdown renderer so the diff view, the code editor,
//! HTML previews and applets draw code the same way without depending on it.
use gpui::{HighlightStyle, SharedString, StyledText, div, prelude::*, px, relative};

use crate::prompt_images::{self, project_prompt_images};
use crate::text_selection::{self, TextSelection};
use crate::theme::{Theme, to_hsla};

mod syntax;
pub(crate) use syntax::{LineHighlighter, MAX_HIGHLIGHT_LINE, token_color};

pub(crate) fn highlight_code(
    body: &str,
    lang: &str,
) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
    syntax::highlight(body, lang)
}

pub(crate) fn code_block(lang: &str, body: &str, window: &gpui::Window) -> gpui::AnyElement {
    code_block_with_selection(lang, body, window, None)
}

pub(crate) fn code_block_with_selection(
    lang: &str,
    body: &str,
    window: &gpui::Window,
    selection: Option<(&gpui::Entity<TextSelection>, SharedString, &gpui::App)>,
) -> gpui::AnyElement {
    let (plain, highlights) = highlight_code(body, lang);
    let line_count = plain.lines().count().max(1);
    let gutter = (1..=line_count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut style = window.text_style();
    style.font_family = Theme::global().mono_family();
    style.font_size = px(12.5).into();
    style.color = to_hsla(Theme::global().CODE_TEXT);
    let mut highlights = highlights;
    let images = prompt_images::current();
    let markers = images.as_ref().map_or_else(Vec::new, |images| {
        crate::image_chip::image_marker_ranges(&plain)
            .into_iter()
            .filter_map(|range| Some((range.clone(), images.attachments.get(&plain[range])?)))
            .collect()
    });
    if let Some((model, key, cx)) = &selection
        && let Some(highlight) = model.read(cx).highlight(key, plain.len())
    {
        highlights.push(highlight);
    }
    let (display, offsets, positioned) = project_prompt_images(
        &plain,
        "",
        &markers,
        gpui::font(Theme::global().FONT_MONO),
        px(12.5),
        window,
    );
    let mut highlights: Vec<_> = highlights
        .into_iter()
        .map(|(range, style)| (offsets.layout_range(range), style))
        .collect();
    highlights.extend(
        positioned
            .iter()
            .map(|(range, _, _)| (range.clone(), crate::image_chip::hidden_marker())),
    );
    let styled =
        StyledText::new(display).with_default_highlights(&style, flatten_highlights(&highlights));
    let layout = styled.layout().clone();
    let has_markers = !markers.is_empty();
    let overlay = images.as_ref().filter(|_| has_markers).map(|images| {
        let keys = markers
            .iter()
            .map(|(range, _)| plain[range.clone()].to_owned())
            .collect();
        let leaf = selection
            .as_ref()
            .map(|(_, key, _)| key.clone())
            .unwrap_or_else(|| format!("code-image-{:x}", hash(&plain)).into());
        crate::image_chip::PositionedChips::new(
            layout.clone(),
            positioned,
            leaf,
            keys,
            images.on_click.clone(),
        )
    });
    let code = if let Some((model, key, cx)) = selection {
        text_selection::selectable_with_offsets(
            model.clone(),
            key,
            plain,
            layout.clone(),
            styled,
            offsets,
            cx,
        )
    } else {
        styled.into_any_element()
    };
    let code = if let Some(overlay) = overlay {
        div()
            .relative()
            .child(code)
            .child(overlay)
            .into_any_element()
    } else {
        code
    };
    // Multi-line blocks always get the header so copy is reachable even when
    // the fence carried no language.
    let show_header = !lang.is_empty() || line_count > 1;
    let copy_body = body.to_string();
    let copy_id: SharedString = format!("md-copy-{:x}", hash(body)).into();

    div()
        .flex()
        .flex_col()
        .my_1()
        .w_full()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(Theme::global().CODE_BORDER)
        .bg(Theme::global().CODE_BG)
        .when(show_header, |el| {
            el.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_2p5()
                    .py_1()
                    .bg(Theme::global().CODE_HEADER_BG)
                    .border_b_1()
                    .border_color(Theme::global().CODE_BORDER)
                    .text_size(px(10.5))
                    .text_color(Theme::global().TEXT_DIM)
                    .font_family(Theme::global().FONT_MONO)
                    .child(if lang.is_empty() {
                        "code".to_string()
                    } else {
                        lang.to_ascii_lowercase()
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(format!(
                                "{line_count} line{}",
                                if line_count == 1 { "" } else { "s" }
                            ))
                            .child(
                                div()
                                    .id(copy_id)
                                    .debug_selector(|| "code-copy".into())
                                    .cursor_pointer()
                                    .text_color(Theme::global().TEXT_FAINT)
                                    .hover(|el| el.text_color(Theme::global().TEXT))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        move |_event, _window, cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                copy_body.clone(),
                                            ));
                                        },
                                    )
                                    .child("copy"),
                            ),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .px_2()
                .py_1p5()
                .gap_2p5()
                .when(line_count > 1, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .font_family(Theme::global().FONT_MONO)
                            .text_size(px(12.5))
                            .line_height(relative(1.5))
                            .text_color(Theme::global().CODE_GUTTER)
                            .text_right()
                            .child(gutter),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .when(has_markers, |el| {
                            el.debug_selector(|| "prompt-image-code".into())
                        })
                        .line_height(relative(1.5))
                        .child(code),
                ),
        )
        .into_any_element()
}

/// GPUI's `StyledText::with_default_highlights` requires highlight ranges to
/// be sorted and non-overlapping; nested spans (code inside bold, a link
/// inside italics) naturally produce overlapping ranges, and feeding those in
/// directly makes GPUI compute text runs that overrun the string and abort
/// the process. Flatten overlaps into disjoint, sorted segments, merging the
/// styles of every range covering each segment (inner spans first, so an
/// outer wrapper refines rather than replaces them).
pub(crate) fn flatten_highlights(
    highlights: &[(std::ops::Range<usize>, HighlightStyle)],
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    // Most inline emphasis and syntax tokens are already ordered and disjoint.
    // Avoid sorting endpoints and rescanning every range for every segment.
    if highlights.iter().all(|(range, _)| range.start <= range.end)
        && highlights
            .windows(2)
            .all(|pair| pair[0].0.end <= pair[1].0.start)
    {
        let mut flattened: Vec<(std::ops::Range<usize>, HighlightStyle)> =
            Vec::with_capacity(highlights.len());
        for (range, style) in highlights {
            if range.is_empty() {
                continue;
            }
            if let Some((last_range, last_style)) = flattened.last_mut()
                && last_range.end == range.start
                && last_style == style
            {
                last_range.end = range.end;
            } else {
                flattened.push((range.clone(), *style));
            }
        }
        return flattened;
    }
    flatten_overlapping_highlights(highlights)
}

pub(crate) fn flatten_overlapping_highlights(
    highlights: &[(std::ops::Range<usize>, HighlightStyle)],
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let mut bounds: Vec<usize> = highlights
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    let mut flattened: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for pair in bounds.windows(2) {
        let (segment_start, segment_end) = (pair[0], pair[1]);
        let mut merged: Option<HighlightStyle> = None;
        for (range, style) in highlights {
            if range.start <= segment_start && segment_end <= range.end {
                merged = Some(match merged {
                    Some(base) => base.highlight(*style),
                    None => *style,
                });
            }
        }
        let Some(style) = merged else { continue };
        if let Some((last_range, last_style)) = flattened.last_mut()
            && last_range.end == segment_start
            && *last_style == style
        {
            last_range.end = segment_end;
            continue;
        }
        flattened.push((segment_start..segment_end, style));
    }
    flattened
}

pub(crate) fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
