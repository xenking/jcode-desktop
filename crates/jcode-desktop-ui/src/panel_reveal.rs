//! Scroll an opened transcript to the message a search hit named.
//!
//! Global search returns message text, not a row address: the daemon's history
//! rows carry no stable id, and a session can be reloaded into a different row
//! layout (reasoning coalesced, todos pinned, tool cards expanded). So the way
//! back is by text. The hit's snippet fragment is verbatim message text, which
//! makes it a reliable needle even though whitespace differs.

use super::*;

impl Panel {
    /// Returns false if the matching row exists but has not been laid out yet.
    pub(crate) fn reveal_transcript_match(
        &mut self,
        needle: Option<&str>,
        exact_hit: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let needle = needle.and_then(normalize_needle);
        let find = |matches: &dyn Fn(&str) -> bool| {
            self.items.iter().position(|item| match item {
                Item::User(text) | Item::Assistant(text) => matches(text),
                _ => false,
            })
        };
        let snippet = || {
            let needle = needle.as_deref()?;
            find(&|text| normalize_needle(text).is_some_and(|text| text.contains(needle)))
        };
        let marked = || {
            find(&|text| !crate::global_search::term_ranges(text, &self.search_terms).is_empty())
        };
        let item = if exact_hit {
            snippet().or_else(marked)
        } else {
            marked().or_else(snippet)
        };
        let Some(item_index) = item else {
            return true;
        };
        let derived = self.transcript_derived(cx);
        let Some(position) = derived.rows.iter().position(|row| {
            matches!(&row.source, TranscriptRowSource::Settled(index) if *index == item_index)
        }) else {
            return true;
        };
        if position >= self.transcript_list.item_count() {
            return false;
        }
        // Landing on a message while the transcript is pinned to the bottom
        // would snap straight back to the newest row on the next stream event.
        self.stick_to_bottom = false;
        self.cancel_transcript_momentum();
        let clearance = (self.transcript_list.viewport_bounds().size.height / 3.).min(px(160.));
        // Keep the start of the match below the sticky prompt, including for tall rows.
        self.transcript_list.scroll_to(gpui::ListOffset {
            item_ix: position,
            offset_in_item: -clearance,
        });
        cx.notify();
        true
    }

    pub(crate) fn scroll_search_preview(&mut self, key: &str, cx: &mut Context<Self>) {
        let page = f32::from(self.transcript_list.viewport_bounds().size.height) * 0.9;
        let delta = match key {
            "up" => 40.0,
            "down" => -40.0,
            "pageup" => page,
            "pagedown" => -page,
            "home" => f32::from(self.transcript_list.max_offset_for_scrollbar().y),
            "end" => -f32::from(self.transcript_list.max_offset_for_scrollbar().y),
            _ => return,
        };
        self.scroll_transcript_direct(delta, cx);
    }
}

/// Lowercase, fold `ё` to `е` (the search index does, the transcript does not),
/// and collapse whitespace so a snippet that joins lines still matches.
fn normalize_needle(text: &str) -> Option<String> {
    let folded: String = text
        .chars()
        .map(|character| match character {
            'ё' => 'е',
            'Ё' => 'Е',
            other => other,
        })
        .collect();
    let normalized = folded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (!normalized.trim().is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::normalize_needle;

    #[test]
    fn needles_fold_yo_and_collapse_whitespace() {
        assert_eq!(
            normalize_needle("  Ещё\tраз\nпроверь ").as_deref(),
            Some("еще раз проверь")
        );
        assert_eq!(normalize_needle("   ").as_deref(), None);
    }
}
