//! Opening and closing the composer popovers.
use super::*;

impl Panel {

    pub(super) fn open_search(&mut self, kind: PickerKind, cx: &mut Context<Self>) {
        self.swarm_list_open = false;
        self.routing_popover = None;
        self.select_background_job(None, cx);
        if kind == PickerKind::Context {
            self.load_mcp_servers(cx);
        }
        let settings = self.input.read(cx).picker_settings();
        let models = self.available_models.clone();
        let deferred = self.session_deferred();
        let submit = cx.weak_entity();
        let cancel = cx.weak_entity();
        let outside = cx.weak_entity();
        let speed = cx.weak_entity();
        let accepted = cx.weak_entity();
        let draft = self.input.downgrade();
        let search = cx.new(|cx| {
            let placeholder = match kind {
                PickerKind::Model => "Search models…",
                PickerKind::Effort => "Search effort…",
                PickerKind::Mode => "Search modes and skills • Tab toggles • Enter closes",
                PickerKind::Context => "Search skills and context…",
            };
            let mut input = PromptInput::new(cx, placeholder, move |content, _, _, app| {
                if let Some(panel) = submit.upgrade() {
                    panel.update(app, |panel, cx| {
                        if matches!(kind, PickerKind::Mode | PickerKind::Context) {
                            if content.starts_with("/mode toggle ") || content == "/mode off" {
                                if panel.session_deferred() {
                                    panel.handle_slash_command(&content, cx);
                                } else {
                                    panel.handle_runtime_command(&content, cx);
                                }
                            }
                            return;
                        }
                        if content.starts_with("/model ") || content.starts_with("/effort ") {
                            panel.handle_slash_command(&content, cx);
                        }
                        panel.picker_search = None;
                        panel.model_picker_open = false;
                        panel.composer_popover_trigger = None;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                }
            })
            .with_on_model_speed(move |choice, app| {
                let _ = speed.update(app, |panel, cx| panel.choose_model_speed(choice, cx));
            })
            .with_on_overlay_cancel(move |app| {
                if let Some(panel) = cancel.upgrade() {
                    panel.update(app, |panel, cx| {
                        panel.picker_search = None;
                        panel.model_picker_open = false;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                    true
                } else {
                    false
                }
            })
            .with_on_picker_outside(move |focus, window, app| {
                let _ = outside.update(app, |panel, cx| {
                    panel.dismiss_composer_popover(focus, true, window, cx);
                });
            });
            input.apply_picker_settings(settings, cx);
            input.set_pending_session(deferred, cx);
            input.set_speed_picker(self.speed_picker(), cx);
            input.start_picker_search(
                match kind {
                    PickerKind::Model => "/model",
                    PickerKind::Effort => "/effort",
                    PickerKind::Mode | PickerKind::Context => "/mode",
                },
                models,
                cx,
            );
            if kind == PickerKind::Context {
                input.set_skills(self.skills.clone(), cx);
                input.set_current_modes(self.mode_names().to_vec(), self.mode_catalog.clone(), cx);
                input.start_context_picker(self.composer_context_root(), draft, move |app| {
                    let _ = accepted.update(app, |panel, cx| {
                        panel.picker_search = None;
                        panel.composer_popover_trigger = None;
                        panel.focus_pending = true;
                        cx.notify();
                    });
                }, cx);
                input.set_mcp_servers(self.mcp_servers.clone(), cx);
            }
            input
        });
        self.picker_search = Some((kind, search));
        self.focus_pending = true;
        cx.notify();
    }

    pub(super) fn dismiss(
        &mut self,
        expected: &FocusHandle,
        restore_if_focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // An outside click can open a different picker. Never dismiss that one.
        if self.composer_popover_focus(cx).as_ref() != Some(expected) {
            return;
        }
        let restore = restore_if_focused && expected.contains_focused(window, cx);
        if self.picker_search.take().is_some() {
            self.model_picker_open = false;
        } else {
            self.swarm_list_open = false;
            self.routing_popover = None;
            self.select_background_job(None, cx);
        }
        self.composer_popover_trigger = None;
        if restore {
            self.focus_input(window, cx);
        }
        cx.notify();
    }

    pub(super) fn toggle_effort(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        if self
            .picker_search
            .as_ref()
            .is_some_and(|(kind, _)| *kind == PickerKind::Effort)
        {
            self.picker_search = None;
        } else {
            if self.model_picker_open {
                if self.picker_search.is_some() {
                    self.close_model_picker(cx);
                } else {
                    // A typed /model is still a draft, not a transient menu.
                    self.model_picker_open = false;
                }
            }
            self.login = None;
            self.recovery_picker_open = false;
            self.open_picker_search(PickerKind::Effort, cx);
        }
        self.focus_input(window, cx);
        cx.notify();
    }
}
