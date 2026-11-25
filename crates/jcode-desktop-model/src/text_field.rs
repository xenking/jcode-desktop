//! Caret, selection and IME composition for plain-string text fields.
//!
//! The composer and the masked login field keep their text as a `String` and
//! address it in UTF-8 byte offsets, while the platform speaks UTF-16. This
//! module owns that arithmetic once: the selection with its marked text,
//! the boundaries the arrow keys and double click use, and the UTF-16
//! conversions. The code editor keeps a rope with undo in [`crate::text_buffer`]
//! and addresses it in `char` offsets, so the two stay separate models.
//!
//! Every function takes the text it works on instead of storing it. The
//! composer's text is a `SharedString` that the rest of the UI reads, and the
//! login field must be able to wipe its `String`.

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Selection and marked (IME preedit) range of one text field, in byte offsets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldSelection {
    /// Ordered, so `start <= end`.
    pub range: Range<usize>,
    /// The caret is at `range.start` and the anchor at `range.end`.
    pub reversed: bool,
    /// Text the IME is still composing.
    pub marked: Option<Range<usize>>,
}

impl FieldSelection {
    /// The moving end of the selection.
    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.range.start
        } else {
            self.range.end
        }
    }

    /// Drop the selection and put the caret at `offset`.
    pub fn collapse_to(&mut self, offset: usize) {
        self.range = offset..offset;
        self.reversed = false;
    }

    /// Move the caret to `offset` and keep the anchor.
    pub fn extend_to(&mut self, offset: usize) {
        let anchor = if self.reversed {
            self.range.end
        } else {
            self.range.start
        };
        self.range = anchor.min(offset)..anchor.max(offset);
        self.reversed = offset < anchor;
    }

    /// Select `0..len`.
    pub fn select_all(&mut self, len: usize) {
        self.range = 0..len;
        self.reversed = false;
    }

    /// Byte range an edit from the platform replaces: the UTF-16 range it
    /// names, else the marked text, else the selection. Marked and selected
    /// ranges go stale when text changes outside the IME, so the result is
    /// clamped to `text` rather than trusted.
    pub fn replacement_range(&self, text: &str, range_utf16: Option<&Range<usize>>) -> Range<usize> {
        let range = range_utf16
            .map(|range| range_from_utf16(text, range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.range.clone());
        clamp_range(text, range)
    }

    /// `inserted` bytes now sit at `start`. Put the caret after them and end
    /// any composition.
    pub fn commit(&mut self, start: usize, inserted: usize) {
        self.collapse_to(start + inserted);
        self.marked = None;
    }

    /// `new_text` now sits at `start` as the composition. `selected_utf16` is
    /// the IME's selection, relative to `new_text`.
    pub fn mark(&mut self, start: usize, new_text: &str, selected_utf16: Option<&Range<usize>>) {
        self.marked = (!new_text.is_empty()).then(|| start..start + new_text.len());
        self.reversed = false;
        self.range = match selected_utf16 {
            Some(selected) => {
                let from = utf16_to_byte(new_text, selected.start);
                let to = utf16_to_byte(new_text, selected.end).max(from);
                start + from..start + to
            }
            None => start + new_text.len()..start + new_text.len(),
        };
    }
}

/// Byte offset of a UTF-16 offset within `text`, clamped to `text.len()`. An
/// offset inside a surrogate pair rounds down to the character start.
pub fn utf16_to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return index;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// UTF-16 offset of a byte offset. An offset inside a character counts as the
/// character's start.
pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    let mut units = 0;
    for (index, ch) in text.char_indices() {
        if index + ch.len_utf8() > byte {
            break;
        }
        units += ch.len_utf16();
    }
    units
}

/// Ordered byte range for a platform UTF-16 range.
pub fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = utf16_to_byte(text, range.start);
    start..utf16_to_byte(text, range.end).max(start)
}

pub fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    byte_to_utf16(text, range.start)..byte_to_utf16(text, range.end)
}

/// `range` ordered, inside `text`, and on character boundaries, so slicing
/// `text` with it cannot panic.
pub fn clamp_range(text: &str, range: Range<usize>) -> Range<usize> {
    let floor = |mut offset: usize| {
        offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    };
    let start = floor(range.start.min(range.end));
    start..floor(range.start.max(range.end)).max(start)
}

/// Start of the character before `offset`.
pub fn previous_char_boundary(text: &str, offset: usize) -> usize {
    text[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

/// End of the character after `offset`.
pub fn next_char_boundary(text: &str, offset: usize) -> usize {
    text[offset..]
        .chars()
        .next()
        .map_or(offset, |ch| offset + ch.len_utf8())
}

/// Start of the grapheme cluster before `offset`.
pub fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

/// End of the grapheme cluster after `offset`.
pub fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(text.len())
}

/// Start of the whitespace-delimited word before `offset`.
pub fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let prefix = &text[..offset];
    let trimmed = prefix.trim_end_matches(char::is_whitespace);
    trimmed
        .char_indices()
        .rev()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index + ch.len_utf8()))
        .unwrap_or(0)
}

/// Start of the next whitespace-delimited word after `offset`.
pub fn next_word_boundary(text: &str, offset: usize) -> usize {
    let suffix = &text[offset..];
    let word_end = suffix
        .char_indices()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
        .unwrap_or(suffix.len());
    let rest = &suffix[word_end..];
    offset
        + word_end
        + rest
            .char_indices()
            .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
            .unwrap_or(rest.len())
}

/// Word under a double click at byte `offset`, by Unicode word segmentation.
/// At the end of the text it selects the final segment rather than nothing.
pub fn clicked_word_range(text: &str, offset: usize) -> Range<usize> {
    let probe = offset.min(text.len().saturating_sub(1));
    text.split_word_bound_indices()
        .find_map(|(start, word)| {
            let end = start + word.len();
            (start <= probe && probe < end).then_some(start..end)
        })
        .unwrap_or(0..0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(range: Range<usize>, reversed: bool) -> FieldSelection {
        FieldSelection {
            range,
            reversed,
            marked: None,
        }
    }

    #[test]
    fn extending_keeps_the_anchor_and_flips_around_it() {
        let mut sel = FieldSelection::default();
        sel.collapse_to(4);
        sel.extend_to(7);
        assert_eq!(sel, selection(4..7, false));
        sel.extend_to(1);
        assert_eq!(sel, selection(1..4, true));
        assert_eq!(sel.cursor(), 1);
        sel.extend_to(9);
        assert_eq!(sel, selection(4..9, false));
        assert_eq!(sel.cursor(), 9);
        sel.extend_to(4);
        assert_eq!(sel, selection(4..4, false), "back at the anchor is a caret");
        sel.select_all(12);
        assert_eq!(sel, selection(0..12, false));
    }

    #[test]
    fn commit_moves_the_caret_past_the_text_and_ends_composition() {
        let mut sel = selection(2..5, true);
        sel.marked = Some(2..5);
        sel.commit(2, 3);
        assert_eq!(sel, selection(5..5, false));
    }

    #[test]
    fn mark_selects_relative_to_the_new_text_and_clamps() {
        let mut sel = FieldSelection::default();
        sel.mark(9, "你好", Some(&(1..9)));
        assert_eq!(sel.marked, Some(9..15));
        assert_eq!(sel.range, 12..15, "UTF-16 1..9 is bytes 3..6 of the new text");
        sel.mark(9, "ni", None);
        assert_eq!(sel.range, 11..11);
        sel.mark(9, "", None);
        assert_eq!(sel.marked, None, "an empty preedit ends the composition");
        assert_eq!(sel.range, 9..9);
    }

    #[test]
    fn replacement_prefers_the_platform_range_then_marked_then_selection() {
        let text = "你好，ab";
        let mut sel = selection(9..10, false);
        assert_eq!(sel.replacement_range(text, None), 9..10);
        sel.marked = Some(0..3);
        assert_eq!(sel.replacement_range(text, None), 0..3);
        assert_eq!(sel.replacement_range(text, Some(&(1..2))), 3..6);
        // Stale ranges split characters or run past the end.
        sel.marked = Some(1..400);
        assert_eq!(sel.replacement_range(text, None), 0..11);
        #[allow(clippy::reversed_empty_ranges)]
        let inverted = 5..2;
        assert_eq!(sel.replacement_range(text, Some(&inverted)), 11..11);
    }

    #[test]
    fn utf16_conversion_floors_inside_characters() {
        let text = "a😀é";
        assert_eq!(utf16_to_byte(text, 0), 0);
        assert_eq!(utf16_to_byte(text, 1), 1);
        assert_eq!(utf16_to_byte(text, 2), 1, "inside the surrogate pair");
        assert_eq!(utf16_to_byte(text, 3), 5);
        assert_eq!(utf16_to_byte(text, 99), 7);
        assert_eq!(byte_to_utf16(text, 5), 3);
        assert_eq!(byte_to_utf16(text, 3), 1, "inside the emoji");
        assert_eq!(byte_to_utf16(text, 99), 4);
        assert_eq!(range_from_utf16(text, &(1..99)), 1..7);
        assert_eq!(range_to_utf16(text, &(1..7)), 1..4);
    }

    #[test]
    fn char_boundaries_step_one_scalar_and_grapheme_boundaries_one_cluster() {
        // a, e, U+0301 (2 bytes), then a man and woman joined by a ZWJ.
        let text = "ae\u{301}👨\u{200d}👩";
        assert_eq!(previous_char_boundary(text, 4), 2, "steps over the accent alone");
        assert_eq!(previous_char_boundary(text, 2), 1);
        assert_eq!(previous_grapheme_boundary(text, 4), 1, "e + accent");
        assert_eq!(next_char_boundary(text, 1), 2);
        assert_eq!(next_grapheme_boundary(text, 1), 4);
        assert_eq!(next_grapheme_boundary(text, 4), text.len(), "the couple");
        assert_eq!(next_char_boundary(text, text.len()), text.len());
        assert_eq!(previous_char_boundary(text, 0), 0);
        assert_eq!(previous_grapheme_boundary(text, 0), 0);
    }

    #[test]
    fn word_boundaries_break_on_whitespace_only() {
        let text = "one   two three";
        assert_eq!(next_word_boundary(text, 0), 6);
        assert_eq!(next_word_boundary(text, 6), 10);
        assert_eq!(next_word_boundary(text, text.len()), text.len());
        assert_eq!(previous_word_boundary(text, text.len()), 10);
        assert_eq!(previous_word_boundary(text, 10), 6);
        assert_eq!(previous_word_boundary(text, 0), 0);
        let text = "foo.bar(baz)  qux\nnext";
        assert_eq!(next_word_boundary(text, 0), 14, "punctuation does not stop");
        assert_eq!(next_word_boundary(text, 14), 18, "a newline is whitespace");
        assert_eq!(previous_word_boundary(text, 18), 14);
        let text = "你好  world 🚀";
        let second = next_word_boundary(text, 0);
        assert_eq!(&text[second..], "world 🚀");
        assert_eq!(previous_word_boundary(text, text.len()), next_word_boundary(text, second));
    }

    #[test]
    fn double_click_selects_the_unicode_word_without_adjacent_space() {
        for (text, offset, selected) in [
            ("hello world", 1, "hello"),
            ("hello world", 5, " "),
            ("hello world", 11, "world"),
            ("hello, world", 5, ","),
            ("héllo 猫 🚀", 7, "猫"),
            ("héllo 猫 🚀", usize::MAX, "🚀"),
            ("", 0, ""),
        ] {
            assert_eq!(&text[clicked_word_range(text, offset)], selected);
        }
    }
}
