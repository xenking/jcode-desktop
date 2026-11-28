use gpui::{Bounds, Font, Pixels, Point, SharedString, TextRun, Window, point, px, size};
use std::ops::Range;

/// Typography from the last frame. NSTextInputClient can query new text before
/// GPUI repaints, so the frame supplies style and origin, not string offsets.
#[derive(Clone)]
pub struct ImeGeometry {
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    wrap_width: Option<Pixels>,
}

impl ImeGeometry {
    pub fn new(
        font: Font,
        font_size: Pixels,
        line_height: Pixels,
        wrap_width: Option<Pixels>,
    ) -> Self {
        Self {
            font,
            font_size,
            line_height,
            wrap_width,
        }
    }

    /// `range` uses UTF-8 byte offsets into `display_text`, which must already
    /// be masked for secret inputs. Return the part on the first visual row.
    pub fn bounds_for_range(
        &self,
        display_text: SharedString,
        range: Range<usize>,
        origin: Point<Pixels>,
        window: &Window,
    ) -> Option<Bounds<Pixels>> {
        display_text.get(range.clone())?;
        let run = TextRun {
            len: display_text.len(),
            font: self.font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let lines = window
            .text_system()
            .shape_text(display_text, self.font_size, &[run], self.wrap_width, None)
            .ok()?;
        let mut offset = 0;
        let mut top = px(0.);
        for line in lines {
            if range.start <= offset + line.len() {
                let start_index = range.start - offset;
                let end_of_row = line
                    .wrap_boundaries()
                    .iter()
                    .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
                    .find(|&index| index >= start_index)
                    .unwrap_or(line.len());
                let end_index = (range.end - offset).min(end_of_row);
                let start = line.position_for_index(start_index, self.line_height)?;
                let end = line.position_for_index(end_index, self.line_height)?;
                return Some(Bounds::new(
                    origin + start + point(px(0.), top),
                    size((end.x - start.x).max(px(0.)), self.line_height),
                ));
            }
            offset += line.len() + 1;
            top += self.line_height * (line.wrap_boundaries().len() + 1);
        }
        None
    }
}
