//! Submitting, pasting and recalling prompts.
use super::*;

impl PromptInput {
    pub(super) fn accept_completion(
        &mut self,
        _: &AcceptCompletion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.attachments.is_empty()
            && let Some(name) = self.selected_mode_name()
        {
            self.toggle_mode_row(&name, window, cx);
            return;
        }
        if self.accept_context_row(false, window, cx) {
            return;
        }
        if self.complete_slash_command(cx) {
            return;
        }
        let at_end = self.cursor_at_end() && self.sel.marked.is_none();
        let accepted = self
            .ghost
            .as_mut()
            .and_then(|ghost| ghost.machine.accept(&self.content, at_end));
        match accepted {
            Some(suffix) => {
                self.replace_editor_text(self.replacement_range(None), &suffix, None, cx);
                self.dismiss_ghost();
            }
            None => cx.propagate(),
        }
    }

    pub(super) fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if self.accept_context_row(false, window, cx) {
            return;
        }
        if self.picker_prefix == Some("/mode") {
            if let Some(close) = &self.on_overlay_cancel {
                close(cx);
            }
            return;
        }
        self.submit_prompt(false, window, cx);
    }

    pub(super) fn queue(&mut self, _: &Queue, window: &mut Window, cx: &mut Context<Self>) {
        if self.standalone_context() {
            return;
        }
        self.submit_prompt(true, window, cx);
    }

    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        let private_clipboard = cx
            .try_global::<crate::clipboard_image::ControlPasteImage>()
            .is_some();
        #[cfg(not(target_os = "macos"))]
        let private_clipboard = false;
        if self.paste_mode != PasteMode::Attachments {
            if !private_clipboard {
                self.paste_clipboard_text(cx);
            }
            return;
        }
        match crate::clipboard_image::read_paste(cx) {
            Ok(crate::clipboard_image::ClipboardContent::Files(paths)) => {
                self.attach_pasted_files(&paths, window, cx);
                return;
            }
            Ok(crate::clipboard_image::ClipboardContent::Image(image)) => {
                self.attach_image(image, cx);
                return;
            }
            Ok(crate::clipboard_image::ClipboardContent::Empty) => {}
            Err(error) => {
                self.attachment_notice = Some(format!("image paste unavailable: {error}").into());
            }
        }
        if !private_clipboard
            && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
        {
            self.paste_text_or_image_paths(&text, window, cx);
        }
    }

    pub(super) fn paste_plain_text(
        &mut self,
        _: &PastePlainText,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste_clipboard_text(cx);
    }

    pub(super) fn paste_clipboard_text(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert_pasted_text(&text, cx);
        }
    }

    pub(super) fn paste_text_or_image_paths(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(paths) = image_paths::pasted_image_paths(text) {
            self.drop_files(&paths, window, cx);
        } else {
            self.insert_pasted_text(text, cx);
        }
    }

    pub(super) fn insert_pasted_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = self.replacement_range(None);
        if !self.try_attach_pasted_text(&range, text, cx) {
            self.replace_editor_text(range, text, None, cx);
        }
        if self.paste_mode != PasteMode::InlineText {
            self.finish_paste();
        }
    }

    pub(super) fn history_prev(&mut self, _: &HistoryPrev, _: &mut Window, cx: &mut Context<Self>) {
        let suggestions = self.command_suggestions();
        if !suggestions.is_empty() {
            self.command_selection = self
                .command_selection
                .saturating_sub(1)
                .min(suggestions.len() - 1);
            self.command_selection_explicit =
                Some(suggestions[self.command_selection].value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
            return;
        }
        if self.move_visual_row(-1, cx) || self.history.is_empty() {
            return;
        }
        if self.history_index.is_some() && !self.attachments.is_empty() {
            self.set_transient_notice("Remove or send images before browsing history".into(), cx);
            return;
        }
        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                let (content, selection) = self.draft_without_live_dictation();
                self.live_draft = Some(HistoryDraft {
                    content: content.into_owned(),
                    selection,
                    selection_reversed: self.sel.reversed,
                    attachments: std::mem::take(&mut self.attachments),
                });
                self.attachment_preview = None;
                self.attachment_notice = None;
                self.history.len() - 1
            }
        };
        self.show_history_entry(index, cx);
    }

    pub(super) fn history_next(&mut self, _: &HistoryNext, _: &mut Window, cx: &mut Context<Self>) {
        let suggestions = self.command_suggestions();
        if !suggestions.is_empty() {
            self.command_selection = (self.command_selection + 1).min(suggestions.len() - 1);
            self.command_selection_explicit =
                Some(suggestions[self.command_selection].value.clone());
            self.reveal_command(self.command_selection);
            cx.notify();
            return;
        }
        if self.move_visual_row(1, cx) {
            return;
        }
        let Some(index) = self.history_index else {
            return;
        };
        if !self.attachments.is_empty() {
            self.set_transient_notice("Remove or send images before browsing history".into(), cx);
            return;
        }
        if index + 1 < self.history.len() {
            self.show_history_entry(index + 1, cx);
        } else {
            self.history_index = None;
            if let Some(draft) = self.live_draft.take() {
                self.restore_history_draft(draft, cx);
            }
        }
    }

    pub(crate) fn set_current_modes(
        &mut self,
        modes: Vec<String>,
        catalog: crate::mode_catalog::ModeCatalog,
        cx: &mut Context<Self>,
    ) {
        if self.current_modes == modes && self.mode_catalog == catalog {
            return;
        }
        let selected = self.selected_mode_name();
        self.current_modes = modes;
        self.mode_catalog = catalog;
        self.suggestions_revision += 1;
        if let Some(index) = selected.and_then(|name| {
            self.command_suggestions()
                .iter()
                .position(|row| row.value.strip_prefix("/mode ") == Some(&name))
        }) {
            self.command_selection = index;
            self.reveal_command(index);
        }
        cx.notify();
    }

    pub(super) fn caret_position(
        &self,
        layout: &PromptLayout,
        line_height: Pixels,
    ) -> Option<Point<Pixels>> {
        layout.position_for_caret(
            self.sel.cursor(),
            line_height,
            self.vertical_caret().is_some_and(|caret| caret.downstream),
        )
    }

    /// False means the caret is already at the edge in this direction.
    pub(super) fn move_visual_row(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        if self.content.is_empty() {
            return false;
        }
        let (Some(layout), Some(height)) = (&self.last_layout, self.last_line_height) else {
            return true;
        };
        if self.last_layout_revision != self.control_revision {
            return true;
        }
        let Some(caret) = self.caret_position(layout, height) else {
            return true;
        };
        let Some(end) = layout.position_for_index(self.content.len(), height) else {
            return true;
        };
        let row = |y: Pixels| (y / height).round() as isize;
        let goal_x = self.vertical_caret().map_or(caret.x, |caret| caret.goal_x);
        let mut target = row(caret.y) + step;
        let last = row(end.y);
        let chips = self.chip_ranges();
        while (0..=last).contains(&target) {
            let offset = layout
                .closest_index_for_position(point(goal_x, height * (target as f32 + 0.5)), height)
                .unwrap_or_else(|offset| offset)
                .min(self.content.len());
            let mut grapheme = GraphemeCursor::new(offset, self.content.len(), true);
            let candidates = if let Some((_, chip)) = chips
                .iter()
                .find(|(_, range)| range.start < offset && offset < range.end)
            {
                [chip.start, chip.end]
            } else if grapheme.is_boundary(&self.content, 0).unwrap_or(false) {
                [offset, offset]
            } else {
                let before = grapheme.prev_boundary(&self.content, 0).ok().flatten();
                let mut grapheme = GraphemeCursor::new(offset, self.content.len(), true);
                let after = grapheme.next_boundary(&self.content, 0).ok().flatten();
                [
                    before.unwrap_or_default(),
                    after.unwrap_or(self.content.len()),
                ]
            };
            let nearest = candidates
                .into_iter()
                .flat_map(|offset| [(offset, false), (offset, true)])
                .filter_map(|(offset, downstream)| {
                    let position = layout.position_for_caret(offset, height, downstream)?;
                    (row(position.y) == target).then_some((
                        offset,
                        downstream,
                        f32::from((position.x - goal_x).abs()),
                    ))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((offset, downstream, _)) = nearest {
                self.move_to(offset, cx);
                self.vertical_caret = Some(VerticalCaret {
                    offset,
                    revision: self.control_revision,
                    goal_x,
                    downstream,
                });
                return true;
            }
            target += step;
        }
        false
    }
}
