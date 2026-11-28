//! Keyboard editing: motion, deletion, undo, history and dictation.
use super::*;

impl PromptInput {
    /// Nothing but whitespace typed.
    pub(crate) fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }

    /// Tab on a visible slash popup accepts its highlighted row.
    pub(super) fn complete_slash_command(&mut self, cx: &mut Context<Self>) -> bool {
        if self.picker_prefix.is_some() {
            return false;
        }
        if !self.command_completion || !self.attachments.is_empty() {
            return false;
        }
        let suggestions = self.command_suggestions();
        let Some(row) = suggestions.get(self.command_selection) else {
            return false;
        };
        if row.toggle.is_some()
            || (row.action.is_some()
                && !(self.model_stage == model_stage::ModelStage::Models
                    && row.value.starts_with("/model ")))
        {
            return false;
        }
        let value = row.value.clone();
        let embedded = self.embedded_command_token();
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        if let Some((range, _)) = embedded {
            let caret = range.start + value.len();
            let mut draft = self.content.to_string();
            draft.replace_range(range, &value);
            self.set_content(draft, cx);
            self.sel.range = caret..caret;
            self.close_command_menu(CommandMenuClose::Literal);
        } else {
            self.set_content(value, cx);
            self.close_command_menu(CommandMenuClose::Completed);
        }
        cx.notify();
        true
    }

    pub(super) fn shift_space(
        &mut self,
        _: &ShiftSpace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.content.is_empty() && self.picker_prefix.is_none() {
            cx.propagate();
        } else {
            self.replace_text_in_range(None, " ", window, cx);
        }
    }

    pub(super) fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.step_picker_effort(1, cx) {
            return;
        }
        if self.sel.cursor() == self.content.len() && self.enter_selected_mcp(cx) {
            return;
        }
        if self.sel.range.is_empty() {
            self.move_to(self.next_boundary(self.sel.range.end), cx);
        } else {
            self.move_to(self.sel.range.end, cx)
        }
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.range.is_empty() {
            let prev = self.previous_boundary(self.sel.cursor());
            if self.sel.cursor() == prev {
                return;
            }
            self.select_to(prev, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.sel.range.is_empty() {
            let next = self.next_boundary(self.sel.cursor());
            if self.sel.cursor() == next {
                return;
            }
            self.select_to(next, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.sel.range.clone()].to_string(),
            ));
        }
    }

    pub(super) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sel.range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.sel.range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    pub(super) fn delete_word_back(
        &mut self,
        _: &DeleteWordBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sel.range.is_empty() {
            self.select_to(previous_word_boundary(&self.content, self.sel.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sel.range.is_empty() {
            self.select_to(next_word_boundary(&self.content, self.sel.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.edit_snapshot());
            self.attachments = previous.attachments;
            self.set_content(previous.content, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.edit_snapshot());
            self.attachments = next.attachments;
            self.set_content(next.content, cx);
        }
    }

    pub(super) fn show_history_entry(&mut self, index: usize, cx: &mut Context<Self>) {
        self.history_index = Some(index);
        self.sel.marked = None;
        self.set_content(self.history[index].clone(), cx);
        self.close_command_menu(CommandMenuClose::Literal);
    }

    /// One dim line above the partials saying where the words will land.
    pub(crate) fn set_voice_preview_hint(&mut self, hint: Option<&str>, cx: &mut Context<Self>) {
        let hint = hint.filter(|hint| !hint.is_empty()).map(SharedString::from);
        if self.voice_preview_hint != hint {
            self.voice_preview_hint = hint;
            cx.notify();
        }
    }

    /// Insert dictated text at the caret as one undoable edit. This is what
    /// `[dictation] mode = "insert"` means: the words arrive where the user was
    /// typing (replacing a selection, like typing over it), the caret lands
    /// after them, and nothing is submitted. Words are separated from adjacent
    /// text so two dictated phrases do not fuse into one token.
    pub(crate) fn insert_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let range = self.expanded_chip_range(clamp_range(&self.content, self.sel.range.clone()));
        let mut insert = String::with_capacity(text.len() + 2);
        if self.content[..range.start]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace())
        {
            insert.push(' ');
        }
        insert.push_str(text);
        if self.content[range.end..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
        {
            insert.push(' ');
        }
        let caret = range.start + insert.len();
        let edited = self.content[..range.start].to_owned() + &insert + &self.content[range.end..];
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.remove_chips_in_range(&range);
        self.control_revision = self.control_revision.wrapping_add(1);
        self.rebase_live_dictation_after_edit(&range, insert.len());
        self.content = edited.into();
        self.sel.marked = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        self.sel.range = caret..caret;
        self.sel.reversed = false;
        self.reset_model_groups_after_exit();
        self.command_selection = 0;
        self.reveal_command(0);
        self.dismiss_ghost();
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
        cx.notify();
    }

    /// Append asynchronous dictation on a new line as one undoable edit,
    /// without replacing newer edits or attachments.
    pub(crate) fn append_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        let mut draft = self.content.to_string();
        if !draft.is_empty() && !draft.ends_with('\n') {
            draft.push('\n');
        }
        draft.push_str(text);
        self.sel.marked = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        self.set_content(draft, cx);
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
    }

    /// Replace the whole draft with dictated text as one undoable edit. This is
    /// what `[dictation] mode = "replace"` means: the utterance stands where the
    /// draft was, the discarded draft stays one undo away, and redo is cleared
    /// because the new content is not a step back toward the old future.
    pub(crate) fn replace_dictation(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.undo.push(self.edit_snapshot());
        self.redo.clear();
        self.sel.marked = None;
        if self.live_draft.is_none() {
            self.history_index = None;
        }
        let mut content = text.to_owned();
        ensure_visible_chips(&mut content, &mut self.attachments);
        self.set_content(content, cx);
        // Dictation completes inside a parent Panel update. Run its callback
        // after that lease ends, just as a later native edit would.
        let input = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = input.update(cx, |input, cx| {
                if let Some(on_change) = &input.on_change {
                    on_change(&input.content, cx);
                }
            });
        });
    }

    pub(super) fn vertical_caret(&self) -> Option<&VerticalCaret> {
        self.vertical_caret.as_ref().filter(|caret| {
            caret.revision == self.control_revision
                && self.sel.range.is_empty()
                && caret.offset == self.sel.cursor()
        })
    }
}
