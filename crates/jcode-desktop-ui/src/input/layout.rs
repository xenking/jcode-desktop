//! Preserve every explicit line returned by GPUI shaping, including empty lines.
//! Byte offsets include newline separators, while visual rows include soft wraps.
use crate::theme::{Theme, to_hsla};
use gpui::{
    App, Bounds, PaintQuad, Pixels, Point, Window, WrappedLine, fill, point, px, size,
};
use std::ops::Range;

pub(super) struct PromptLayout {
    lines: Vec<WrappedLine>,
}

impl PromptLayout {
    pub(super) fn new(lines: impl IntoIterator<Item = WrappedLine>) -> Self {
        Self {
            lines: lines.into_iter().collect(),
        }
    }

    pub(super) fn visual_line_count(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.wrap_boundaries().len() + 1)
            .sum()
    }

    pub(super) fn position_for_index(
        &self,
        index: usize,
        line_height: Pixels,
    ) -> Option<Point<Pixels>> {
        self.position_for_caret(index, line_height, false)
    }

    pub(super) fn position_for_caret(
        &self,
        index: usize,
        line_height: Pixels,
        downstream: bool,
    ) -> Option<Point<Pixels>> {
        let mut start = 0;
        let mut top = px(0.);
        for line in &self.lines {
            if index <= start + line.len() {
                if downstream
                    && let Some(row) = line.wrap_boundaries().iter().position(|boundary| {
                        line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index
                            == index - start
                    })
                {
                    return Some(point(px(0.), top + line_height * (row + 1)));
                }
                return line
                    .position_for_index(index - start, line_height)
                    .map(|position| position + point(px(0.), top));
            }
            start += line.len() + 1;
            top += line_height * (line.wrap_boundaries().len() + 1);
        }
        None
    }

    pub(super) fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<usize, usize> {
        if position.y < px(0.) {
            return Err(0);
        }
        let mut start = 0;
        let mut top = px(0.);
        for line in &self.lines {
            let height = line_height * (line.wrap_boundaries().len() + 1);
            if position.y < top + height {
                return line
                    .closest_index_for_position(position - point(px(0.), top), line_height)
                    .map(|index| start + index)
                    .map_err(|index| start + index);
            }
            start += line.len() + 1;
            top += height;
        }
        Err(start.saturating_sub(1))
    }

    pub(super) fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        let mut top = px(0.);
        for line in &self.lines {
            line.paint(
                origin + point(px(0.), top),
                line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )?;
            top += line_height * (line.wrap_boundaries().len() + 1);
        }
        Ok(())
    }
}

pub(super) fn selection_quads(
    line: &PromptLayout,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<PaintQuad> {
    range_quads(line, range, bounds, line_height, Theme::global().SELECTION)
}

pub(super) fn range_quads(
    line: &PromptLayout,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    color: gpui::Rgba,
) -> Vec<PaintQuad> {
    let Some(start) = line.position_for_index(range.start, line_height) else {
        return Vec::new();
    };
    let Some(end) = line.position_for_index(range.end, line_height) else {
        return Vec::new();
    };
    let first_row = (start.y / line_height) as usize;
    let last_row = (end.y / line_height) as usize;
    (first_row..=last_row)
        .map(|row| {
            let left = if row == first_row { start.x } else { px(0.) };
            let right = if row == last_row {
                end.x
            } else {
                bounds.size.width
            };
            fill(
                Bounds::new(
                    point(bounds.left() + left, bounds.top() + line_height * row),
                    size((right - left).max(px(0.)), line_height),
                ),
                to_hsla(color),
            )
        })
        .collect()
}
