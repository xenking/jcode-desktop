//! Session modes are skills pinned to a session by the runtime.
use super::*;

impl Panel {
    pub(super) fn seed_mode_picker_fixture(&mut self, cx: &mut Context<Self>) {
        self.apply_event(&ApiEvent::RuntimeCommandResult {
            session_id: self.session_id.clone(), command: jcode_sdk::api::RuntimeCommand::Skills,
            data: serde_json::json!({"pinned_skills": ["selfdev", "swarm", "poteto-mode"], "skills": [
                {"name": "selfdev", "description": "Work on Jcode"},
                {"name": "swarm", "description": "Coordinate agents"},
                {"name": "poteto-mode", "description": "Concise development style"},
                {"name": "quiet", "description": "Keep output brief", "mode": true, "icon": "☾"},
                {"name": "verify-jcode", "description": "Run a native verification"},
                {"name": "write-guide", "description": "Write product documentation"}
            ]}), message: String::new(),
        }, cx);
    }

    pub(super) fn load_draft_skills(&mut self, cx: &mut Context<Self>) {
        let cwd = self.working_dir.clone();
        let directory = cwd.clone();
        let skills = cx.background_executor().spawn(async move {
            let skill_root = directory.as_deref().map(std::path::Path::new);
            let registry = jcode_base::skill::SkillRegistry::load_for_working_dir(skill_root);
            registry
                .map(|registry| {
                    let mut skills = Vec::new();
                    let mut catalog = crate::mode_catalog::ModeCatalog::new();
                    for skill in registry.list() {
                        skills.push((skill.name.clone(), skill.description.clone()));
                        catalog.insert(skill.name.clone(), crate::mode_catalog::ModeMetadata {
                            icon: skill.icon.clone(),
                            mode: skill.mode,
                        });
                    }
                    (skills, catalog)
                })
                .unwrap_or_default()
        });
        cx.spawn(async move |panel, cx| {
            let (skills, catalog) = skills.await;
            let _ = panel.update(cx, |panel, cx| {
                if !panel.session_deferred() || panel.working_dir != cwd {
                    return;
                }
                panel.skills = skills;
                panel.mode_catalog = catalog;
                panel.sync_mode_menu(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn select_draft_mode(&mut self, argument: &str, cx: &mut Context<Self>) {
        if argument.is_empty() {
            self.open_picker_search(PickerKind::Mode, cx);
            self.focus_pending = true;
            cx.notify();
            return;
        }
        let toggle = argument.strip_prefix("toggle ");
        let name = toggle.unwrap_or(argument);
        let modes = &mut self.draft_config.modes;
        if name == "off" {
            modes.clear();
        } else if !self.skills.iter().any(|(skill, _)| skill == name) {
            self.effort_notice(format!("Unknown mode: {name}"), cx);
            return;
        } else if toggle.is_some() {
            if let Some(index) = modes.iter().position(|mode| mode == name) {
                modes.remove(index);
            } else {
                modes.push(name.to_owned());
            }
        } else {
            *modes = vec![name.to_owned()];
        }
        self.pinned_modes.clone_from(modes);
        self.sync_mode_menu(cx);
        cx.notify();
    }

    /// Keep the composer's `/mode` menu current after local or runtime changes.
    /// Deferred: `/mode` results can arrive inside the composer's callbacks.
    pub(super) fn sync_mode_menu(&self, cx: &mut Context<Self>) {
        let modes = self.mode_names().to_vec();
        let catalog = self.mode_catalog.clone();
        let skills = self.skills.clone();
        let input = self.input.clone();
        let search = self
            .picker_search
            .as_ref()
            .map(|(_, search)| search.clone());
        cx.defer(move |cx| {
            input.update(cx, |input, cx| {
                input.set_skills(skills.clone(), cx);
                input.set_current_modes(modes.clone(), catalog.clone(), cx);
            });
            if let Some(search) = search {
                search.update(cx, |input, cx| {
                    input.set_skills(skills, cx);
                    input.set_current_modes(modes, catalog, cx);
                });
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

    pub(super) fn mode_names(&self) -> &[String] {
        if self.pinned_modes.is_empty() {
            self.active_skill.as_slice()
        } else {
            &self.pinned_modes
        }
    }

    pub(super) fn mode_label(&self) -> Option<String> {
        let names = self.mode_names();
        if names.is_empty() {
            return None;
        }
        Some(
            names
                .iter()
                .map(|name| {
                    let icon = crate::mode_catalog::icon(name, self.mode_catalog.get(name));
                    if names.len() == 1 {
                        format!("{icon} {}", name.strip_suffix("-mode").unwrap_or(name))
                    } else {
                        icon.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("  "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn mode_menu_click_and_tab_toggle_until_enter_or_escape(cx: &mut gpui::TestAppContext) {
        use jcode_sdk::api::RuntimeCommand;
        cx.update(crate::input::bind_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("mode-picker".into(), None, None, bridge, cx));
        let install = |panel: &mut Panel, cx: &mut Context<Panel>| {
            panel.apply_event(
                &ApiEvent::RuntimeCommandResult {
                    session_id: "mode-picker".into(),
                    command: RuntimeCommand::Skills,
                    data: serde_json::json!({"pinned_skills": ["selfdev"], "skills": [
                        {"name": "poteto-mode", "description": "Concise style"},
                        {"name": "selfdev", "description": "Development"},
                        {"name": "swarm", "description": "Several agents"},
                        {"name": "ordinary", "description": "Not a mode", "icon": "✎"}
                    ]}),
                    message: String::new(),
                },
                cx,
            );
        };
        panel.update(vcx, install);
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("panel-mode").unwrap();
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("mode-picker-active-0").is_some());
        let swarm = vcx.debug_bounds("slash-command-row-2").unwrap();
        vcx.simulate_click(swarm.center(), gpui::Modifiers::default());
        assert!(
            matches!(commands.try_recv(), Ok(Command::RuntimeCommand { command: RuntimeCommand::Mode, args, .. }) if args == ["toggle", "swarm"])
        );
        panel.update(vcx, |panel, cx| {
            panel.apply_event(&ApiEvent::RuntimeCommandResult {
                session_id: "mode-picker".into(), command: RuntimeCommand::Mode,
                data: serde_json::json!({"pinned_skills": ["selfdev", "swarm"], "pinned_skill": "selfdev"}),
                message: String::new(),
            }, cx);
            assert!(panel.picker_search.is_some());
            assert!(panel.active_skill.is_none());
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("mode-picker-active-1").is_some());
        vcx.simulate_keystrokes("tab");
        assert!(
            matches!(commands.try_recv(), Ok(Command::RuntimeCommand { command: RuntimeCommand::Mode, args, .. }) if args == ["toggle", "swarm"])
        );
        vcx.simulate_keystrokes("enter");
        assert!(panel.read_with(vcx, |panel, _| panel.picker_search.is_none()));
        let pill = vcx.debug_bounds("panel-mode").unwrap();
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        vcx.simulate_keystrokes("escape");
        assert!(panel.read_with(vcx, |panel, _| panel.picker_search.is_none()));
        assert!(commands.try_recv().is_err());
    }

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
            assert_eq!(rows, ["/mode poteto-mode", "/mode selfdev", "/mode off"]);
            assert_eq!(
                search.read(cx).selected_command_value().as_deref(),
                Some("/mode poteto-mode")
            );
        });
    }
}
