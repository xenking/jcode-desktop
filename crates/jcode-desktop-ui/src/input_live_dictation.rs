use super::*;
use std::borrow::Cow;

impl PromptInput {
    pub(crate) fn dictation_proof_state(&self) -> serde_json::Value {
        serde_json::json!({
            "text": self.content.as_ref(),
            "caret_byte": self.cursor_offset(),
            "provisional": self.live_dictation.is_some(),
        })
    }

    pub(crate) fn begin_live_dictation(&mut self, cx: &mut Context<Self>) {
        let caret = self
            .clamp_byte_range(self.cursor_offset()..self.cursor_offset())
            .start;
        let anchor = self.expanded_chip_range(caret..caret).end;
        self.live_dictation = Some(anchor..anchor);
        self.dismiss_ghost();
        cx.notify();
    }

    pub(crate) fn revise_live_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(span) = self.live_dictation.clone() else {
            return;
        };
        let text = text.trim();
        let mut replacement = String::new();
        if !text.is_empty() {
            if self.content[..span.start]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            {
                replacement.push(' ');
            }
            replacement.push_str(text);
            if self.content[span.end..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace())
            {
                replacement.push(' ');
            }
        }
        let previous = &self.content[span.clone()];
        if previous == replacement {
            return;
        }
        // Streaming backends send aggregate revisions. Their common prefix
        // stays in the draft, including all tokens the provider finalized.
        let prefix_len = previous
            .chars()
            .zip(replacement.chars())
            .take_while(|(old, new)| old == new)
            .map(|(old, _)| old.len_utf8())
            .sum::<usize>();
        let changed = (span.start + prefix_len)..span.end;
        let next_end = span.start + replacement.len();
        let follows_voice =
            self.selected_range == (span.end..span.end) && self.marked_range.is_none();
        self.selected_range = rebase_range(&self.selected_range, &changed, next_end);
        if follows_voice {
            self.selected_range = next_end..next_end;
        }
        self.marked_range = self
            .marked_range
            .as_ref()
            .map(|marked| rebase_range(marked, &changed, next_end));
        self.control_revision = self.control_revision.wrapping_add(1);
        let mut content = self.content.to_string();
        content.replace_range(changed, &replacement[prefix_len..]);
        self.content = content.into();
        self.live_dictation = Some(span.start..next_end);
        self.dismiss_ghost();
        cx.notify();
    }

    pub(crate) fn end_live_dictation(&mut self, final_text: Option<&str>, cx: &mut Context<Self>) {
        if self.live_dictation.is_none() {
            return;
        }
        self.revise_live_dictation(final_text.unwrap_or_default(), cx);
        if self
            .live_dictation
            .as_ref()
            .is_some_and(|span| !span.is_empty())
        {
            self.undo.push(self.edit_snapshot());
            self.redo.clear();
            self.history_index = None;
            let input = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = input.update(cx, |input, cx| {
                    if let Some(on_change) = &input.on_change {
                        on_change(&input.content, cx);
                    }
                });
            });
        }
        self.live_dictation = None;
        cx.notify();
    }

    pub(super) fn draft_without_live_dictation(&self) -> (Cow<'_, str>, Range<usize>) {
        match &self.live_dictation {
            Some(span) if !span.is_empty() => {
                let mut draft = self.content.to_string();
                draft.replace_range(span.clone(), "");
                (
                    Cow::Owned(draft),
                    rebase_range(&self.selected_range, span, span.start),
                )
            }
            _ => (Cow::Borrowed(&self.content), self.selected_range.clone()),
        }
    }

    /// A manual edit at either side moves the anchor. Editing the dictated
    /// range itself takes ownership of those words from the recognizer.
    pub(super) fn rebase_live_dictation_after_edit(&mut self, range: &Range<usize>, len: usize) {
        let Some(span) = &mut self.live_dictation else {
            return;
        };
        if range.end <= span.start {
            let start = span.start - range.len() + len;
            *span = start..start + span.len();
        } else if range.start < span.end {
            self.live_dictation = None;
        }
    }
}

fn rebase_range(selection: &Range<usize>, replaced: &Range<usize>, new_end: usize) -> Range<usize> {
    let boundary = |offset: usize| {
        if offset <= replaced.start {
            offset
        } else if offset >= replaced.end {
            offset - replaced.end + new_end
        } else {
            // Interior byte offsets may no longer be UTF-8 boundaries.
            replaced.start
        }
    };
    boundary(selection.start)..boundary(selection.end)
}
