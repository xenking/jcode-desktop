//! Session modes are skills pinned to a session by the runtime.
use super::*;

impl Panel {
    pub(super) fn seed_mode_picker_fixture(&mut self, cx: &mut Context<Self>) {
        self.apply_event(&ApiEvent::RuntimeCommandResult {
            session_id: self.session_id.clone(), command: jcode_sdk::api::RuntimeCommand::Skills,
            data: serde_json::json!({"pinned_skills": ["selfdev", "swarm", "poteto-mode"], "skills": [
                {"name": "selfdev", "description": "Work on Jcode"},
                {"name": "swarm", "description": "Coordinate agents"},
                {"name": "poteto-mode", "description": "Concise development style with deliberate subagents, plain prose, simple code and verified work. Use for requests to work in this style.", "icon": "crown"},
                {"name": "quiet", "description": "Keep output brief", "mode": true, "icon": "moon"},
                {"name": "verify-jcode", "description": "Verify the real CLI and TUI after a behavior or rendering change by driving an isolated instance and inspecting its frames"},
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

    pub(super) fn toggle_context_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_side_document() {
            return;
        }
        if self
            .picker_search
            .as_ref()
            .is_some_and(|(kind, _)| *kind == PickerKind::Context)
        {
            self.picker_search = None;
        } else {
            if self.model_picker_open {
                self.model_picker_open = false;
            }
            self.login = None;
            self.recovery_picker_open = false;
            self.open_picker_search(PickerKind::Context, cx);
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

}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn context_picker_does_not_scan_local_paths_for_remote_sessions(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        for id in ["ssh://host/session", "cloud://account/session"] {
            let panel = cx.new(|cx| Panel::new(
                id.into(), None, Some(directory.path().display().to_string()),
                crate::harness::spawn_inert(), cx,
            ));
            panel.update(cx, |panel, cx| {
                assert!(panel.composer_context_root().is_none());
                panel.open_picker_search(PickerKind::Context, cx);
                let (_, search) = panel.picker_search.as_ref().unwrap();
                assert!(search.read(cx).context_root().is_none());
                assert_eq!(search.read(cx).command_suggestion_values(), ["Attach…"]);
            });
        }
    }

    #[gpui::test]
    fn context_plus_uses_a_skill_for_one_submission(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("skill-picker".into(), None, None, bridge, cx));
        vcx.update(|_, app| Panel::connect_input(&panel, app));
        panel.update(vcx, |panel, cx| {
            panel.load_history(vec![], vec![], cx);
            panel.seed_mode_picker_fixture(cx);
        });
        vcx.run_until_parked();
        while commands.try_recv().is_ok() {}
        let plus = vcx.debug_bounds("composer-context-plus").unwrap();
        vcx.simulate_click(plus.center(), Default::default());
        vcx.run_until_parked();
        vcx.simulate_input("verify-jcode");
        vcx.simulate_keystrokes("alt-enter");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("picker-search").is_none());
        assert!(vcx.debug_bounds("composer-image-chip").is_some());
        vcx.simulate_input("Check the preview");
        vcx.simulate_keystrokes("enter");
        let sent = std::iter::from_fn(|| commands.try_recv().ok()).find_map(|command| {
            if let Command::SendSkill { skill, content, .. } = command {
                Some((skill, content.trim().to_owned()))
            } else {
                None
            }
        });
        assert_eq!(sent, Some(("verify-jcode".into(), "Check the preview".into())));
        assert!(vcx.debug_bounds("composer-image-chip").is_none());
        assert!(!panel.read_with(vcx, |panel, _| {
            panel.mode_names().iter().any(|mode| mode == "verify-jcode")
        }));
    }

    #[gpui::test]
    fn context_plus_plain_enter_pins_the_mode_and_click_unpins_it(cx: &mut gpui::TestAppContext) {
        use jcode_sdk::api::RuntimeCommand;
        cx.update(crate::input::bind_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new("mode-context-picker".into(), None, None, bridge, cx)
        });
        panel.update(vcx, |panel, cx| panel.seed_mode_picker_fixture(cx));
        vcx.run_until_parked();
        vcx.update(|window, app| {
            window.focus(&panel.read(app).input_focus_handle(app), app);
        });
        vcx.simulate_input("Keep this draft");
        let modes = ["selfdev", "swarm", "poteto-mode", "quiet"];
        for pinned in [true, false] {
            while commands.try_recv().is_ok() {}
            let plus = vcx.debug_bounds("composer-context-plus").unwrap();
            vcx.simulate_click(plus.center(), Default::default());
            vcx.run_until_parked();
            vcx.simulate_input("quiet");
            if pinned {
                vcx.simulate_keystrokes("enter");
            } else {
                let row = vcx.debug_bounds("slash-command-row-0").unwrap();
                vcx.simulate_click(row.center(), Default::default());
            }
            vcx.run_until_parked();
            assert!(matches!(
                commands.try_recv(),
                Ok(Command::RuntimeCommand { command: RuntimeCommand::Mode, args, .. })
                    if args == ["toggle", "quiet"]
            ));
            assert!(vcx.debug_bounds("picker-search").is_none());
            assert!(vcx.debug_bounds("composer-image-chip").is_none());
            assert!(vcx.update(|window, app| {
                panel.read(app).input_focus_handle(app).is_focused(window)
            }));
            assert_eq!(
                panel.read_with(vcx, |panel, cx| panel.input.read(cx).content.clone()),
                "Keep this draft"
            );

            let active = if pinned { &modes[..] } else { &modes[..3] };
            panel.update(vcx, |panel, cx| {
                panel.apply_event(
                    &ApiEvent::RuntimeCommandResult {
                        session_id: panel.session_id.clone(),
                        command: RuntimeCommand::Mode,
                        data: serde_json::json!({"pinned_skills": active}),
                        message: String::new(),
                    },
                    cx,
                );
            });
            vcx.run_until_parked();
            assert_eq!(vcx.debug_bounds("panel-mode-icon-moon-3").is_some(), pinned);
        }
    }

    #[gpui::test]
    fn context_plus_opens_multi_file_attach_and_accepts_option_click_for_one_use(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::input::bind_keys);
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new("context-picker".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| panel.seed_mode_picker_fixture(cx));
        vcx.run_until_parked();
        let open = |vcx: &mut gpui::VisualTestContext| {
            let plus = vcx.debug_bounds("composer-context-plus").unwrap();
            vcx.simulate_click(plus.center(), Default::default());
            vcx.run_until_parked();
        };
        open(vcx);
        vcx.simulate_input("attach");
        vcx.simulate_keystrokes("enter");
        assert!(vcx.did_prompt_for_paths());
        let directory = tempfile::tempdir().unwrap();
        let photo = directory.path().join("screenshot.png");
        std::fs::write(&photo, include_bytes!("../../../assets/previews/image-preview.png")).unwrap();
        vcx.simulate_path_prompt_response(|options| {
            assert!(options.files && options.multiple);
            Some(vec![photo])
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-image-chip").is_some());
        assert!(panel.read_with(vcx, |panel, _| panel.picker_search.is_none()));
        open(vcx);
        vcx.simulate_input("verify-jcode");
        while commands.try_recv().is_ok() {}
        let row = vcx.debug_bounds("slash-command-row-0").unwrap();
        vcx.simulate_click(
            row.center(),
            gpui::Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("picker-search").is_none());
        assert!(panel.read_with(vcx, |panel, cx| {
            panel.input.read(cx).content.contains("Skill: verify-jcode")
        }));
        assert!(!std::iter::from_fn(|| commands.try_recv().ok()).any(|command| {
            matches!(
                command,
                Command::RuntimeCommand {
                    command: jcode_sdk::api::RuntimeCommand::Mode,
                    ..
                }
            )
        }));
    }

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
        panel.update(vcx, |panel, cx| panel.open_picker_search(PickerKind::Mode, cx));
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
        panel.update(vcx, |panel, cx| panel.open_picker_search(PickerKind::Mode, cx));
        vcx.run_until_parked();
        vcx.simulate_keystrokes("escape");
        assert!(panel.read_with(vcx, |panel, _| panel.picker_search.is_none()));
        assert!(commands.try_recv().is_err());
    }

    #[gpui::test]
    fn plus_keeps_the_pinned_skill_icon_and_opens_the_context_picker(cx: &mut gpui::TestAppContext) {
        cx.update(crate::input::bind_keys);
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("panel-mode").is_none(),
            "no modes to choose from"
        );
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
            panel.mode_catalog.insert(
                "poteto-mode".into(),
                crate::mode_catalog::ModeMetadata {
                    icon: Some("crown".into()),
                    mode: Some(true),
                },
            );
            panel.sync_mode_menu(cx);
            cx.notify();
        });
        vcx.run_until_parked();
        let pill = vcx.debug_bounds("panel-mode").expect("mode pill");
        assert!(vcx.debug_bounds("panel-mode-icon-crown-0").is_some());
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        let poteto = vcx
            .debug_bounds("mode-picker-icon-crown-1")
            .expect("poteto has a rendered crown icon, not the frontmatter text");
        for selector in ["mode-picker-icon-selfdev-2", "mode-picker-icon-trash-3"] {
            let icon = vcx.debug_bounds(selector).expect("rendered row icon");
            assert_eq!(icon.origin.x, poteto.origin.x);
            assert_eq!(icon.size, poteto.size);
        }
        panel.read_with(vcx, |panel, cx| {
            let (kind, search) = panel.picker_search.as_ref().expect("menu open");
            assert_eq!(*kind, PickerKind::Context);
            let rows = search.read(cx).command_suggestion_values();
            assert_eq!(rows, ["Attach…", "/mode poteto-mode", "/mode selfdev", "/mode off"]);
            assert_eq!(
                search.read(cx).selected_command_value().as_deref(),
                Some("Attach…")
            );
        });
        panel.update(vcx, |panel, cx| {
            panel.active_skill = None;
            panel.sync_mode_menu(cx);
            cx.notify();
        });
        vcx.run_until_parked();
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-mode-icon-crown-0").is_none());
        let trigger = panel.read_with(vcx, |panel, _| {
            panel.picker_triggers[PickerKind::Context as usize].clone()
        });
        assert!(vcx.update(|window, _| trigger.is_focused(window)));
        let keystroke = gpui::Keystroke::parse("space").unwrap();
        vcx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        vcx.simulate_event(gpui::KeyUpEvent { keystroke });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("picker-search").is_some());
    }
}
