//! Scroll an opened transcript to the message a search hit named.
//!
//! Global search returns message text, not a row address: the daemon's history
//! rows carry no stable id, and a session can be reloaded into a different row
//! layout (reasoning coalesced, todos pinned, tool cards expanded). So the way
//! back is by text. The hit's snippet fragment is verbatim message text, which
//! makes it a reliable needle even though whitespace differs.

use super::*;

impl Panel {
    /// Reveal the transcript row whose text contains `needle`.
    ///
    /// Returns false when the transcript does not contain it *yet*: a session
    /// opened from search loads its history asynchronously, so the caller
    /// retries for a short window rather than giving up on the first frame.
    pub(crate) fn reveal_transcript_text(
        &mut self,
        needle: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(needle) = normalize_needle(needle) else {
            return false;
        };
        let Some(item_index) = self.items.iter().position(|item| match item {
            Item::User(text) | Item::Assistant(text) => {
                normalize_needle(text).is_some_and(|text| text.contains(&needle))
            }
            _ => false,
        }) else {
            return false;
        };
        let derived = self.transcript_derived(cx);
        let Some(position) = derived.rows.iter().position(|row| {
            matches!(&row.source, TranscriptRowSource::Settled(index) if *index == item_index)
        }) else {
            return false;
        };
        // Landing on a message while the transcript is pinned to the bottom
        // would snap straight back to the newest row on the next stream event.
        self.stick_to_bottom = false;
        self.cancel_transcript_momentum();
        self.transcript_list.scroll_to_reveal_item(position);
        cx.notify();
        true
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
