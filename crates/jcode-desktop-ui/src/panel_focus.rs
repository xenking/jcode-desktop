//! Keyboard focus of a panel and its composer.
use super::*;

impl Panel {

    pub(super) fn resume_typing(
        &self,
        event: &gpui::KeyDownEvent,
        workspace_focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let key = &event.keystroke;
        if key.modifiers.control
            || key.modifiers.platform
            || key.modifiers.alt
            || !key
                .key_char
                .as_deref()
                .is_some_and(|text| !text.is_empty() && text.chars().all(|c| !c.is_control()))
            || self.transcript_only
            || self.ask_card.is_some()
            || self.routing_gate.is_some()
            || self.login.is_some()
            || self.recovery_picker_open
            || self.input_focus_handle(cx) != self.input.read(cx).editor_focus()
            || !(workspace_focused
                || self
                    .transcript_selection
                    .read(cx)
                    .focus_handle()
                    .is_focused(window))
        {
            return false;
        }
        self.focus_input(window, cx);
        let input = self.input.clone();
        let text = key.key_char.clone().unwrap();
        // The input's on_change callback updates this panel synchronously.
        // Release the panel borrow before inserting the first character.
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                gpui::EntityInputHandler::replace_text_in_range(input, None, &text, window, cx);
            });
        });
        true
    }

    pub(super) fn input_handle(&self, cx: &App) -> FocusHandle {
        if let Some(handle) = self.login_input_focus_handle(cx) {
            return handle;
        }
        if let Some((_, search)) = &self.picker_search {
            return search.read(cx).editor_focus();
        }
        if let Some(handle) = self.document_focus_handle(cx) {
            return handle;
        }
        if self.image_preview.is_some() || self.diff_review.is_some() || self.is_accounts_panel() {
            self.focus_handle.clone()
        } else if self.unfinished_work.is_some()
            || self.orchestration.is_some()
            || self.openrig.is_some()
            || self.is_side_document()
            || self.applet.is_some()
        {
            // Read-only panels do not render their prompt input. Focusing that
            // detached handle prevents workspace actions such as FocusLeft from
            // bubbling through the rendered panel tree.
            self.focus_handle.clone()
        } else {
            self.input.read(cx).editor_focus()
        }
    }
}
