//! Session mode: the skill the runtime keeps pinned to this session
//! (`/mode <skill>`, or `/selfdev`, `/poteto-mode` and other skill commands).
//! The composer shows it as a pill beside effort, and the pill opens the same
//! searchable `/mode` menu, so switching modes works like switching effort.
use super::*;

impl Panel {
    /// Keep the composer's `/mode` menu marking the pinned skill.
    /// Deferred: `/mode` results can arrive inside the composer's callbacks.
    pub(super) fn sync_mode_menu(&self, cx: &mut Context<Self>) {
        let mode = self.active_skill.clone();
        let input = self.input.clone();
        let search = self
            .picker_search
            .as_ref()
            .map(|(_, search)| search.clone());
        cx.defer(move |cx| {
            input.update(cx, |input, cx| input.set_current_mode(mode.clone(), cx));
            if let Some(search) = search {
                search.update(cx, |input, cx| input.set_current_mode(mode, cx));
            }
        });
    }

    /// Toggle the `/mode` menu from the mode pill.
    pub(super) fn toggle_mode_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        if self
            .picker_search
            .as_ref()
            .is_some_and(|(kind, _)| *kind == PickerKind::Mode)
        {
            self.picker_search = None;
        } else {
            if self.model_picker_open {
                self.model_picker_open = false;
            }
            self.login = None;
            self.recovery_picker_open = false;
            self.open_picker_search(PickerKind::Mode, cx);
        }
        self.focus_input(window, cx);
        cx.notify();
    }

    /// The pinned skill, as the mode pill shows it.
    pub(super) fn mode_label(&self) -> Option<String> {
        self.active_skill
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.strip_suffix("-mode").unwrap_or(name).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn mode_pill_names_the_pinned_skill_and_opens_the_mode_menu(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-mode").is_none(), "no mode, no pill");
        panel.update(vcx, |panel, cx| {
            panel.skills = vec![
                ("poteto-mode".into(), "Concise style".into()),
                ("selfdev".into(), "Fork workflow".into()),
            ];
            let skills = panel.skills.clone();
            panel
                .input
                .update(cx, |input, cx| input.set_skills(skills, cx));
            panel.active_skill = Some("poteto-mode".into());
            panel.sync_mode_menu(cx);
            cx.notify();
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("panel-mode").expect("mode pill");
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        panel.read_with(vcx, |panel, cx| {
            let (kind, search) = panel.picker_search.as_ref().expect("menu open");
            assert_eq!(*kind, PickerKind::Mode);
            let rows = search.read(cx).command_suggestion_values();
            assert_eq!(rows, ["/mode off", "/mode poteto-mode", "/mode selfdev"]);
            assert_eq!(
                search.read(cx).selected_command_value().as_deref(),
                Some("/mode poteto-mode")
            );
        });
    }
}
