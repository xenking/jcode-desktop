//! Scroll an opened transcript to the message a search hit named.
//!
//! Global search returns message text, not a row address: the daemon's history
//! rows carry no stable id, and a session can be reloaded into a different row
//! layout (reasoning coalesced, todos pinned, tool cards expanded). So the way
//! back is by text. The hit's snippet fragment is verbatim message text, which
//! makes it a reliable needle even though whitespace differs.

use super::*;
use jcode_sdk::desktop_control::RevealRow;
use std::ops::RangeInclusive;

pub(super) struct ControlReveal {
    pub range: RangeInclusive<usize>,
    pub text: Option<String>,
    revision: u64,
    expires_at: Instant,
}

/// How the search preview moves when the keyboard drives it.
#[derive(Clone, Copy)]
pub(crate) enum PreviewScroll {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl Panel {
    pub(super) fn control_highlight(&self) -> Option<&ControlReveal> {
        self.control_reveal.as_ref().filter(|reveal| {
            reveal.revision == self.items.revision() && Instant::now() < reveal.expires_at
        })
    }

    pub(crate) fn control_reveal(
        &mut self,
        row: RevealRow,
        cx: &mut Context<Self>,
    ) -> Result<RangeInclusive<usize>, (&'static str, &'static str)> {
        let not_found = ("row_not_found", "no loaded transcript row matches");
        let (range, text) = match row {
            RevealRow::Index { index, end_index } => (index..=end_index.unwrap_or(index), None),
            RevealRow::Text { text } => {
                if text.trim().is_empty() {
                    return Err(("invalid_row", "text must not be blank"));
                }
                let mut matches =
                    self.items
                        .iter()
                        .enumerate()
                        .filter_map(|(index, item)| match item {
                            Item::User(message)
                            | Item::Assistant(message)
                            | Item::AssistantOutput { text: message, .. }
                            | Item::Reasoning(message)
                            | Item::AgentMessage { message, .. }
                                if message.contains(&text) =>
                            {
                                Some(index)
                            }
                            _ => None,
                        });
                let index = matches.next().ok_or(not_found)?;
                if matches.next().is_some() {
                    return Err(("ambiguous_row", "multiple messages match; specify an index"));
                }
                (index..=index, Some(text))
            }
        };
        if range.is_empty() || *range.end() >= self.items.len() {
            return Err((
                "invalid_row",
                "index range must be within loaded_transcript_items",
            ));
        }
        let position = self.transcript_derived(cx).rows.iter().position(|row| range.contains(&row.index)).ok_or(not_found)?;
        if !self.scroll_to_revealed_row(position) {
            return Err((
                "transcript_not_ready",
                "the transcript has not been laid out yet",
            ));
        }
        for index in range.clone() {
            self.open_agent_message(index);
        }
        let duration = Duration::from_secs(5);
        self.control_reveal = Some(ControlReveal {
            range: range.clone(),
            text,
            revision: self.items.revision(),
            expires_at: Instant::now() + duration,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
        Ok(range)
    }

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
                Item::User(text)
                | Item::Assistant(text)
                | Item::AssistantOutput { text, .. }
                | Item::AgentMessage { message: text, .. } => matches(text),
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
        let Some(position) = derived.rows.iter().position(|row| row.index == item_index) else {
            return true;
        };
        if !self.scroll_to_revealed_row(position) {
            return false;
        }
        self.open_agent_message(item_index);
        cx.notify();
        true
    }

    fn scroll_to_revealed_row(&mut self, position: usize) -> bool {
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
        true
    }

    pub(crate) fn scroll_search_preview(&mut self, scroll: PreviewScroll, cx: &mut Context<Self>) {
        let page = f32::from(self.transcript_list.viewport_bounds().size.height) * 0.9;
        let delta = match scroll {
            PreviewScroll::LineUp => 40.0,
            PreviewScroll::LineDown => -40.0,
            PreviewScroll::PageUp => page,
            PreviewScroll::PageDown => -page,
            PreviewScroll::Top => f32::from(self.transcript_list.max_offset_for_scrollbar().y),
            PreviewScroll::Bottom => -f32::from(self.transcript_list.max_offset_for_scrollbar().y),
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
