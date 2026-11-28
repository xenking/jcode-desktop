use super::*;

#[gpui::test]
fn ghost_does_not_start_real_requests_in_gpui_tests(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, _, _| {
            input.ghost = Some(GhostState {
                machine: Default::default(),
                context: Box::new(|_| Vec::new()),
                task: None,
                disabled: false,
                cache: Default::default(),
                request_counter: None,
            });
        })
        .unwrap();
    cx.simulate_input(*window, "a realistic draft");
    window
        .update(cx, |input, _, _| {
            assert!(input.ghost.as_ref().unwrap().task.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn ghost_burst_sends_once_and_programmatic_edits_send_nothing(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let counter = std::rc::Rc::new(std::cell::Cell::new(0));
    let test_counter = counter.clone();
    window
        .update(cx, |input, _, _| {
            input.ghost = Some(GhostState {
                machine: Default::default(),
                context: Box::new(|_| Vec::new()),
                task: None,
                disabled: false,
                cache: Default::default(),
                request_counter: Some(test_counter),
            });
        })
        .unwrap();
    cx.simulate_input(*window, "abcd");
    for _ in 0..5 {
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.simulate_input(*window, "e");
    }
    assert_eq!(counter.get(), 0);
    cx.executor().advance_clock(Duration::from_millis(401));
    cx.run_until_parked();
    assert_eq!(counter.get(), 1);

    window
        .update(cx, |input, _, cx| {
            input.set_content("restored draft".into(), cx);
            input.insert_dictation("dictated words", cx);
            input.insert_pasted_text(" pasted contents", cx);
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(counter.get(), 1);
}

#[gpui::test]
fn ghost_identical_request_uses_cache_without_network(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let counter = std::rc::Rc::new(std::cell::Cell::new(0));
    let test_counter = counter.clone();
    window
        .update(cx, |input, _, _| {
            let request = crate::prompt_completion::Request {
                draft: "abcd".into(),
                context: vec![],
                cwd: None,
                project: None,
                model: crate::config::completions().model,
            };
            input.ghost = Some(GhostState {
                machine: Default::default(),
                context: Box::new(|_| Vec::new()),
                task: None,
                disabled: false,
                cache: [(serde_json::to_string(&request).unwrap(), " cached".into())].into(),
                request_counter: Some(test_counter),
            });
        })
        .unwrap();
    cx.simulate_input(*window, "abcd");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.ghost_text(), Some(" cached"))
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(counter.get(), 0);
}

/// Simulate a completion arriving for the current draft, bypassing network.
fn show_ghost(input: &mut PromptInput, suffix: &str) {
    let draft = input.content.to_string();
    let ghost = input.ghost.get_or_insert_with(|| GhostState {
        machine: Default::default(),
        context: Box::new(|_| Vec::new()),
        task: None,
        disabled: true,
        cache: Default::default(),
        request_counter: None,
    });
    let generation = ghost.machine.on_edit(&draft, true).unwrap();
    assert!(ghost.machine.on_response(generation, suffix));
}

#[gpui::test]
fn typing_keeps_composer_focus_across_ghost_and_snapshot_frames(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let initial_focus = window
        .update(cx, |input, _, _| input.focus_handle.clone())
        .unwrap();
    for (index, letter) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        cx.simulate_input(*window, letter);
        window
            .update(cx, |input, window, cx| {
                assert_eq!(input.focus_handle, initial_focus);
                assert!(initial_focus.is_focused(window));
                assert_eq!(input.snapshot().content, "abcde"[..=index]);
                if index >= 3 {
                    show_ghost(input, " suggestion");
                }
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |input, window, _| {
                assert_eq!(input.focus_handle, initial_focus);
                assert!(initial_focus.is_focused(window));
            })
            .unwrap();
    }
    window
        .update(cx, |input, _, _| assert_eq!(&*input.content, "abcde"))
        .unwrap();
}

#[gpui::test]
fn slash_draft_hides_existing_ghost_and_does_not_request_completion(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "previous draft");
    window
        .update(cx, |input, _, _| show_ghost(input, " old suggestion"))
        .unwrap();
    window
        .update(cx, |input, _, cx| input.set_content(String::new(), cx))
        .unwrap();
    cx.simulate_input(*window, "/effort high");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.ghost_text(), None);
            assert!(input.ghost.as_ref().unwrap().task.is_none());
        })
        .unwrap();
}

#[gpui::test]
fn ghost_completion_tab_accepts_escape_dismisses(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "исправь баг");
    window
        .update(cx, |input, _, _| show_ghost(input, " в композере"))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.ghost_text(), Some(" в композере"))
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "tab");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "исправь баг в композере");
            assert_eq!(input.ghost_text(), None);
        })
        .unwrap();

    // Esc hides the ghost but keeps the draft (a second Esc clears it).
    window
        .update(cx, |input, _, _| show_ghost(input, " now"))
        .unwrap();
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "исправь баг в композере");
            assert_eq!(input.ghost_text(), None);
        })
        .unwrap();

    // Typing a diverging character dismisses; moving the caret hides it.
    window
        .update(cx, |input, _, _| show_ghost(input, " now"))
        .unwrap();
    cx.simulate_input(*window, "!");
    window
        .update(cx, |input, _, _| assert_eq!(input.ghost_text(), None))
        .unwrap();
    window
        .update(cx, |input, _, _| show_ghost(input, " now"))
        .unwrap();
    cx.simulate_keystrokes(*window, "left");
    window
        .update(cx, |input, _, _| assert_eq!(input.ghost_text(), None))
        .unwrap();
}

#[gpui::test]
fn tab_completes_the_selected_slash_command(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "/eff");
    cx.simulate_keystrokes(*window, "tab");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "/effort")
        })
        .unwrap();

    window
        .update(cx, |input, _, cx| input.set_content(String::new(), cx))
        .unwrap();
    cx.simulate_input(*window, "/re");
    let expected: Vec<String> = window
        .update(cx, |input, _, _| {
            input
                .command_suggestions()
                .iter()
                .take(2)
                .map(|row| row.value.clone())
                .collect()
        })
        .unwrap();
    assert_eq!(expected.len(), 2);
    cx.simulate_keystrokes(*window, "tab");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), expected[0])
        })
        .unwrap();
    window
        .update(cx, |input, _, _| {
            assert!(input.command_suggestions().is_empty())
        })
        .unwrap();
}

#[gpui::test]
fn escape_hides_slash_menu_before_overlay_cancel(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let cancels = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen = cancels.clone();
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| {
                PromptInput::new(cx, "test", |_, _, _, _| {}).with_on_overlay_cancel(move |_| {
                    seen.set(seen.get() + 1);
                    true
                })
            })
        })
        .unwrap()
    });
    window
        .update(cx, |input, window, cx| {
            window.focus(&input.focus_handle, cx)
        })
        .unwrap();
    cx.simulate_input(*window, "/swarm");
    window
        .update(cx, |input, _, _| {
            assert!(!input.command_suggestions().is_empty())
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "escape");
    window
        .update(cx, |input, _, _| {
            assert!(input.command_suggestions().is_empty(), "menu hidden");
            assert_eq!(input.content.as_ref(), "/swarm", "draft kept");
        })
        .unwrap();
    assert_eq!(cancels.get(), 0, "first Esc must not stop the agent");
    cx.simulate_keystrokes(*window, "escape");
    assert_eq!(cancels.get(), 1, "with no menu, Esc stops as before");
}

#[gpui::test]
fn tab_accepts_highlighted_exact_command_without_sending(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "/help");
    window
        .update(cx, |input, _, _| {
            assert!(!input.command_suggestions().is_empty())
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "tab");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "/help");
            assert!(input.history.is_empty());
            assert!(input.command_suggestions().is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn registered_slash_commands_submit_or_keep_the_draft_with_feedback(cx: &mut TestAppContext) {
    let window = input_window(cx);
    for command in crate::commands::REGISTERED_COMMANDS {
        let model_step_visible = |input: &PromptInput| {
            command.name == "/model"
                && (input.model_stage != model_stage::ModelStage::Models
                    || !input.expanded_model_groups.is_empty())
                && !input.command_suggestions().is_empty()
        };
        window
            .update(cx, |input, _, cx| {
                input.history.clear();
                input.transient_notice = None;
                input.model_stage = Default::default();
                input.expanded_model_groups.clear();
                input.set_content(command.name.into(), cx);
            })
            .unwrap();
        // Enter may first accept the visible completion and leave it in
        // the editor. A second Enter must not make it silently disappear.
        let mut confirmed = command.name.to_owned();
        for _ in 0..2 {
            confirmed = window
                .update(cx, |input, _, _| input.content.to_string())
                .unwrap();
            cx.simulate_keystrokes(*window, "enter");
            if window
                .update(cx, |input, _, _| {
                    input.content.is_empty()
                        || input.transient_notice.is_some()
                        || model_step_visible(input)
                })
                .unwrap()
            {
                break;
            }
        }
        window
                .update(cx, |input, _, _| {
                    assert!(
                        input.history.last().is_some_and(|entry| entry == confirmed.trim() || entry.strip_prefix(confirmed.trim()).is_some_and(|argument| argument.starts_with(' ')))
                            || (!input.content.is_empty() && input.transient_notice.is_some())
                            || model_step_visible(input),
                        "{} was neither submitted nor retained with feedback: draft={:?}, history={:?}, notice={:?}, stage={:?}, expanded={:?}",
                        command.name, input.content, input.history, input.transient_notice,
                        input.model_stage, input.expanded_model_groups,
                    );
                })
                .unwrap();
    }
}

#[gpui::test]
fn tab_without_ghost_leaves_draft_alone(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "hello");
    cx.simulate_keystrokes(*window, "tab");
    window
        // Tab propagates to its pre-existing handling (plain text input).
        .update(cx, |input, _, _| {
            assert_eq!(input.content.trim_end(), "hello")
        })
        .unwrap();
}

#[gpui::test]
fn dictation_is_one_undoable_edit_and_preserves_attachments(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "typed");
    window
        .update(cx, |input, _, cx| {
            input.attachments.push(Attachment {
                label: "4×3".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
            let previous_edits = input.undo.len();
            input.append_dictation("first line\nsecond line", cx);
            assert_eq!(input.undo.len(), previous_edits + 1);
            assert_eq!(input.content.as_ref(), "typed\nfirst line\nsecond line");
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "ctrl-z");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "typed");
            assert_eq!(input.attachments.len(), 1);
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "ctrl-shift-z");
    window
            .update(cx, |input, _, _| {
                assert_eq!(input.content.as_ref(), "typed\nfirst line\nsecond line");
                assert!(matches!(&input.attachments[0].payload, AttachmentPayload::Image { encoded, .. } if encoded.as_ref() == "cG5n"));
            })
            .unwrap();
}

#[gpui::test]
fn model_menu_restores_draft_and_images_after_selection_or_cancel(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
            input.set_content("unfinished prompt".into(), cx);
            input.attachments.push(Attachment {
                label: "4×3".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
            for selected in [false, true] {
                input.open_model_menu(Vec::new(), cx);
                input.open_model_menu(Vec::new(), cx); // Reopening must not replace the saved draft.
                assert_eq!(input.content.as_ref(), "/model ");
                assert!(input.attachments.is_empty());
                let snapshot = input.snapshot();
                assert_eq!(snapshot.content, "unfinished prompt");
                assert_eq!(snapshot.attachments.len(), 1);
                if selected {
                    input.set_content(String::new(), cx);
                }
                input.close_model_menu(cx);
                assert_eq!(input.content.as_ref(), "unfinished prompt");
                assert_eq!(input.attachments.len(), 1);
                assert!(matches!(&input.attachments[0].payload, AttachmentPayload::Image { encoded, .. } if encoded.as_ref() == "cG5n"));
            }
        });
}

#[gpui::test]
fn empty_dictation_and_partials_leave_content_and_undo_untouched(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, cx| {
        input.set_content("keep this ".into(), cx);
        input.sel.range = 0..4;
        input.undo = vec![EditSnapshot {
            content: "previous".into(),
            attachments: vec![],
        }];
        input.redo = vec![EditSnapshot {
            content: "next".into(),
            attachments: vec![],
        }];
        input.append_dictation(" \n\t", cx);
        for preview in [Some(String::new()), Some("partial words".into()), None] {
            input.set_voice_preview(preview.clone(), cx);
            assert_eq!(input.voice_preview, preview);
            assert_eq!(input.content.as_ref(), "keep this ");
            assert_eq!(input.sel.range, 0..4);
            assert_eq!(
                input
                    .undo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec!["previous"]
            );
            assert_eq!(
                input
                    .redo
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>(),
                vec!["next"]
            );
        }
    });
}

#[gpui::test]
fn voice_bounds_requires_a_valid_measurement(cx: &mut TestAppContext) {
    let input = cx.new(|cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(cx, |input, _| {
        assert!(input.voice_bounds().is_none());
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(100.0), px(30.0)));
        input.last_bounds = Some(bounds);
        assert_eq!(input.voice_bounds(), Some(bounds));
        input.last_bounds = Some(Bounds::new(bounds.origin, size(px(0.0), px(30.0))));
        assert!(input.voice_bounds().is_none());
        input.last_bounds = Some(Bounds::new(point(px(f32::NAN), px(0.0)), bounds.size));
        assert!(input.voice_bounds().is_none());
    });
}

#[gpui::test]
fn voice_preview_never_adds_lines_to_composer(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    input.update(vcx, |input, cx| {
        input.spacious = true;
        input.set_voice_preview(Some(String::new()), cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("voice-live-transcript").is_none());
    let composer = vcx.debug_bounds("prompt-input").unwrap();
    input.read_with(vcx, |input, _| {
        assert_eq!(input.voice_bounds(), Some(composer));
    });
    for lines in [30, 60] {
        input.update(vcx, |input, cx| {
            input.set_voice_preview(Some("recognized words\n".repeat(lines)), cx);
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("voice-live-transcript").is_none());
        input.read_with(vcx, |input, _| {
            assert!(input.content.is_empty());
            assert!(input.undo.is_empty());
        });
    }
    input.update(vcx, |input, cx| input.set_voice_preview(None, cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("voice-live-transcript").is_none());
}

#[gpui::test]
fn model_groups_expand_with_enter_without_submitting_and_preserve_exact_routes(
    cx: &mut TestAppContext,
) {
    let window = input_window(cx);
    let routes: Vec<_> = (1..=5)
        .map(|n| jcode_sdk::ModelRouteInfo {
            account: None,
            icon: None,
            speed_options: Vec::new(),
            reasoning_efforts: None,
            model: format!("atlas-{n}"),
            provider: "OpenAI".into(),
            api_method: "openai-api-key".into(),
            available: true,
            detail: String::new(),
            usage: None,
        })
        .collect();
    window
        .update(cx, |input, _, cx| {
            input.set_model_routes(Vec::new(), &routes, None, cx);
            input.set_content("/model".into(), cx);
            assert_eq!(input.model_picker_rows().len(), 3);
            input.command_selection = 3;
            assert!(accepted_command_submission(&input.command_suggestions(), 3).is_none());
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "/model");
            assert!(input.history.is_empty());
            assert_eq!(input.model_picker_rows().len(), 5);
            assert_eq!(input.command_selection, 5);
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, cx| {
            assert_eq!(input.model_picker_rows().len(), 3);
            assert_eq!(input.command_selection, 3);
            input.toggle_model_group("OpenAI · openai-api-key".into(), cx);
            input.set_content("/model atlas-5".into(), cx);
            assert!(!input.expanded_model_groups.is_empty());
            assert_eq!(
                input.command_suggestions()[0].value,
                "/model openai-api:atlas-5"
            );
        })
        .unwrap();
    // One Enter switches. No level is in force here, so none is forced.
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, _| {
            assert_eq!(
                input.history.last().map(String::as_str),
                Some("/model openai-api:atlas-5")
            );
            assert!(input.content.is_empty());
            assert!(input.expanded_model_groups.is_empty());
        })
        .unwrap();
    window
        .update(cx, |input, _, cx| {
            input.set_content("/model".into(), cx);
            input.toggle_model_group("OpenAI · openai-api-key".into(), cx);
            input.set_content(String::new(), cx);
            input.set_content("/model".into(), cx);
            assert_eq!(input.model_picker_rows().len(), 3);
        })
        .unwrap();
}

#[gpui::test]
fn model_group_refresh_reveals_selected_choice_if_it_falls_below_top_three(
    cx: &mut TestAppContext,
) {
    let window = input_window(cx);
    let routes = |favored: &str| {
        (1..=5)
            .map(|n| {
                let model = format!("atlas-{n}");
                jcode_sdk::ModelRouteInfo {
                    account: None,
                    icon: None,
                    speed_options: Vec::new(),
                    reasoning_efforts: None,
                    usage: Some(jcode_sdk::ModelUsage {
                        count: if model == favored { 99 } else { n },
                        ..Default::default()
                    }),
                    model,
                    provider: "OpenAI".into(),
                    api_method: "openai-oauth".into(),
                    available: true,
                    detail: String::new(),
                }
            })
            .collect::<Vec<_>>()
    };
    window
        .update(cx, |input, _, cx| {
            // The current route is pinned first, then newest releases.
            input.set_model_routes(Vec::new(), &routes("atlas-1"), Some("atlas-1".into()), cx);
            input.set_content("/model".into(), cx);
            assert_eq!(
                input.command_suggestions()[0].value,
                "/model openai-oauth:atlas-1"
            );
            // The catalog changes and atlas-1 is no longer current, so it
            // drops out of the top three. The highlight must follow it.
            input.set_model_routes(Vec::new(), &routes("atlas-5"), Some("atlas-5".into()), cx);
            assert_eq!(
                input.command_suggestions()[input.command_selection].value,
                "/model openai-oauth:atlas-1"
            );
            assert_eq!(input.model_picker_rows().len(), 5);
        })
        .unwrap();
}

#[gpui::test]
fn model_usage_ranking_preserves_the_highlighted_choice_on_refresh(cx: &mut TestAppContext) {
    let window = input_window(cx);
    let route = |model: &str, count| jcode_sdk::ModelRouteInfo {
        account: None,
        icon: None,
        speed_options: Vec::new(),
        reasoning_efforts: None,
        model: model.into(),
        provider: "openai".into(),
        api_method: "oauth".into(),
        available: true,
        detail: String::new(),
        usage: Some(jcode_sdk::ModelUsage {
            count,
            last_used_unix_secs: Some(100),
            tracking_started_unix_secs: Some(1),
            selection_count: 0,
            last_selected_unix_secs: None,
        }),
    };
    window
        .update(cx, |input, _, cx| {
            input.set_model_routes(
                vec!["a-rare".into(), "z-favorite".into()],
                &[route("a-rare", 1), route("z-favorite", 12)],
                Some("z-favorite".into()),
                cx,
            );
            input.set_content("/model".into(), cx);
            let suggestions = input.command_suggestions();
            assert_eq!(suggestions[0].value, "/model z-favorite");
            assert!(suggestions[0].active_choice);
            assert!(suggestions[0].help.is_empty());
            assert!(
                suggestions[0]
                    .detail
                    .as_ref()
                    .unwrap()
                    .contains("12 tracked turns")
            );
        })
        .unwrap();
    cx.simulate_keystrokes(*window, "down");
    window
        .update(cx, |input, _, cx| {
            assert_eq!(
                input.command_suggestions()[input.command_selection].value,
                "/model a-rare"
            );
            input.set_model_routes(
                vec!["a-rare".into(), "z-favorite".into()],
                &[route("a-rare", 20), route("z-favorite", 12)],
                Some("z-favorite".into()),
                cx,
            );
            let suggestions = input.command_suggestions();
            // The highlight follows the route, not the slot, when usage
            // reorders the list.
            assert_eq!(suggestions[input.command_selection].value, "/model a-rare");
            assert_eq!(
                suggestions[input.command_selection].action,
                Some(model_stage::StageStep::Submit("/model a-rare".into()))
            );
            input.set_content("/model favorite".into(), cx);
            assert_eq!(input.command_suggestions()[0].value, "/model z-favorite");
        })
        .unwrap();
}

#[gpui::test]
fn appended_dictation_is_laid_out_below_existing_draft(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "Existing draft");
    window
        .update(cx, |input, _, cx| {
            input.append_dictation("Dictated words", cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |input, window, _| {
            assert_eq!(input.content.as_ref(), "Existing draft\nDictated words");
            assert_eq!(
                input.visual_line_count, 2,
                "both explicit lines must render"
            );
            let layout = input.last_layout.as_ref().unwrap();
            let caret = layout
                .position_for_index(input.content.len(), window.line_height())
                .expect("appended text must have a visible caret position");
            assert_eq!(caret.y, window.line_height());
        })
        .unwrap();
}

#[gpui::test]
fn multiline_layout_preserves_blank_lines_unicode_and_hit_testing(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("αβ\n\n猫 text\n".into(), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |input, window, _| {
            let height = window.line_height();
            let layout = input.last_layout.as_ref().unwrap();
            assert_eq!(input.visual_line_count, 4);
            for (index, row) in [(0, 0), (5, 1), (6, 2), (15, 3)] {
                let position = layout.position_for_index(index, height).unwrap();
                assert_eq!(position, point(px(0.), height * row));
                let hit = layout
                    .closest_index_for_position(position + point(px(0.), height / 2.), height)
                    .unwrap_or_else(|index| index);
                assert_eq!(hit, index);
            }
            assert!(
                layout
                    .position_for_index(input.content.len() + 1, height)
                    .is_none()
            );
            assert_eq!(
                selection_quads(
                    layout,
                    0..input.content.len(),
                    input.last_bounds.unwrap(),
                    height
                )
                .len(),
                4
            );
        })
        .unwrap();
}

#[gpui::test]
fn appended_dictation_scrolls_into_view_after_multiline_growth(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, _, cx| {
            input.set_content("Original line\n".repeat(30), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |input, _, cx| {
            input.append_dictation("New dictated line\nAnother dictated line", cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |input, _window, _| {
            let height = input.last_bounds.unwrap().size.height / input.visual_line_count as f32;
            let caret = input
                .last_layout
                .as_ref()
                .unwrap()
                .position_for_index(input.content.len(), height)
                .unwrap();
            let top = input.last_bounds.unwrap().top() + caret.y;
            let viewport = input.editor_scroll.bounds();
            assert!(top >= viewport.top() - px(1.));
            assert!(
                top + height <= viewport.bottom() + px(1.),
                "dictation caret must be visible: {top:?} {viewport:?}"
            );
        })
        .unwrap();
}

#[gpui::test]
fn long_prompts_wrap_and_expand_vertically(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, &"a long prompt ".repeat(200));
    cx.run_until_parked();

    window
        .update(cx, |input, _, _| {
            assert!(
                input.visual_line_count > 1,
                "long prompt should occupy multiple visual lines"
            );
        })
        .unwrap();
}

/// The workspace binds Ctrl+B to ToggleSidebar while the prompt binds it to
/// MoveWordLeft. The prompt's context-scoped binding must win while the
/// prompt has focus, otherwise typing would silently move window chrome.
#[gpui::test]
fn ctrl_b_moves_by_word_in_the_prompt_despite_the_sidebar_shortcut(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.update(crate::bind_workspace_keys);
    cx.simulate_input(*window, "one two three");
    cx.run_until_parked();

    let mut cx = gpui::VisualTestContext::from_window(*window, cx);
    cx.simulate_keystrokes("ctrl-b");
    cx.run_until_parked();

    window
        .update(&mut cx, |input, _, _| {
            assert_eq!(
                &input.content[input.sel.range.start..],
                "three",
                "Ctrl+B should move the caret one word left inside the prompt"
            );
        })
        .unwrap();
}

// ---- Characterization pins -------------------------------------------
//
// They go through the input handler and the editing actions. The composer
// steps by grapheme cluster and treats whitespace as the only word break,
// unlike the code editor (scalars, character classes) and the login field
// (scalars, no word motion). Boundary math is tested in
// jcode-desktop-model::text_field.

#[gpui::test]
fn arrows_and_deletion_step_by_grapheme_cluster(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, window, cx| {
            let family = "👨\u{200d}👩\u{200d}👧";
            input.replace_text_in_range(None, &format!("ae\u{301}{family}b"), window, cx);
            input.left(&Left, window, cx);
            input.left(&Left, window, cx);
            assert_eq!(
                &input.content[input.sel.range.start..],
                format!("{family}b")
            );
            input.left(&Left, window, cx);
            assert_eq!(
                &input.content[input.sel.range.start..],
                format!("e\u{301}{family}b")
            );
            // Backspace after the family removes the whole cluster.
            input.right(&Right, window, cx);
            input.right(&Right, window, cx);
            input.backspace(&Backspace, window, cx);
            assert_eq!(input.content.as_ref(), "ae\u{301}b");
            input.backspace(&Backspace, window, cx);
            assert_eq!(input.content.as_ref(), "ab", "one step removes e + accent");
            input.delete(&Delete, window, cx);
            assert_eq!(input.content.as_ref(), "a");
        })
        .unwrap();
}

#[gpui::test]
fn ranges_from_the_platform_floor_inside_a_surrogate_pair(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, window, cx| {
            input.replace_text_in_range(None, "a😀b", window, cx);
            let mut actual = None;
            let text = input.text_for_range(0..2, &mut actual, window, cx);
            // The composer used to round up to include the emoji. It now
            // floors like the code editor and the login field do.
            assert_eq!(text.as_deref(), Some("a"), "offset 2 splits the emoji");
            assert_eq!(actual, Some(0..1));
            let text = input.text_for_range(1..99, &mut actual, window, cx);
            assert_eq!(text.as_deref(), Some("😀b"));
            assert_eq!(actual, Some(1..4));
        })
        .unwrap();
}

#[gpui::test]
fn gpui_ime_edits_report_selection_marked_text_and_reset_reversal(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, window, cx| {
            input.replace_text_in_range(None, "héllo wörld", window, cx);
            input.select_word_left(&SelectWordLeft, window, cx);
            let selection = input.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selection.range, 6..11);
            assert!(selection.reversed);
            input.select_right(&SelectRight, window, cx);
            assert_eq!(input.sel.range, 8..13, "reversed selection shrinks");
            // Marking over a selection replaces it and drops the reversal.
            input.replace_and_mark_text_in_range(None, "ü", Some(0..1), window, cx);
            assert_eq!(input.content.as_ref(), "héllo wü");
            assert_eq!(input.marked_text_range(window, cx), Some(7..8));
            let selection = input.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selection.range, 7..8);
            assert!(!selection.reversed);
            // An empty marked string clears the composition.
            input.replace_and_mark_text_in_range(None, "", None, window, cx);
            assert_eq!(input.marked_text_range(window, cx), None);
            assert_eq!(input.content.as_ref(), "héllo w");
        })
        .unwrap();
}

#[gpui::test]
fn selection_extension_flips_and_collapsing_arrows_pick_an_edge(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, window, cx| {
            input.replace_text_in_range(None, "one two three", window, cx);
            input.move_to(4, cx);
            input.select_word_right(&SelectWordRight, window, cx);
            assert_eq!(input.sel.range, 4..8);
            input.select_word_left(&SelectWordLeft, window, cx);
            input.select_word_left(&SelectWordLeft, window, cx);
            assert_eq!(input.sel.range, 0..4);
            assert!(input.sel.reversed);
            input.right(&Right, window, cx);
            assert_eq!(input.sel.range, 4..4, "right collapses to the end");
            input.select_all(&SelectAll, window, cx);
            assert_eq!(input.sel.range, 0..13);
            assert!(!input.sel.reversed);
            input.left(&Left, window, cx);
            assert_eq!(input.sel.range, 0..0, "left collapses to the start");
            input.move_to(99, cx);
            assert_eq!(input.sel.range, 13..13, "offsets clamp to the text");
            input.delete_word_back(&DeleteWordBack, window, cx);
            assert_eq!(input.content.as_ref(), "one two ");
            input.kill_to_start(&KillToStart, window, cx);
            assert_eq!(input.content.as_ref(), "");
        })
        .unwrap();
}

#[gpui::test]
fn ported_editing_chords_dispatch_through_the_real_keymap(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "one two");
    cx.simulate_keystrokes(*window, "alt-b ctrl-w");
    window
        .update(cx, |input, _, _| assert_eq!(input.content.as_ref(), "two"))
        .unwrap();

    cx.simulate_keystrokes(*window, "ctrl-z ctrl-shift-z ctrl-u");
    window
        .update(cx, |input, _, _| assert!(input.content.is_empty()))
        .unwrap();
}

#[gpui::test]
fn standalone_btw_opens_history_only_for_normal_composer_submit(cx: &mut TestAppContext) {
    let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = submitted.clone();
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|_, cx| {
        PromptInput::new_with_queue_files(cx, "test", move |kind, text, images, _, queued, _, _| {
            seen.lock()
                .unwrap()
                .push((kind, text, images.len(), queued));
        })
        .with_standalone_btw()
    });
    vcx.update(|window, cx| {
        window.focus(&input.read(cx).focus_handle.clone(), cx);
    });
    vcx.simulate_input("btw");
    vcx.simulate_keystrokes("enter");
    vcx.simulate_input("btw this matters");
    vcx.simulate_keystrokes("enter");
    vcx.simulate_input("btw");
    vcx.simulate_keystrokes("ctrl-enter");
    vcx.simulate_input("btw");
    vcx.update(|window, cx| {
        input.update(cx, |input, cx| assert!(input.control_submit(window, cx)));
    });
    vcx.simulate_input("btw");
    input.update(vcx, |input, _| {
        input.attachments.push(Attachment {
            label: "fixture".into(),
            payload: AttachmentPayload::Image {
                media_type: "image/png".into(),
                encoded: "cG5n".into(),
                preview: preview_image("image/png", Vec::new()).unwrap(),
            },
        });
    });
    vcx.simulate_keystrokes("enter");
    use SubmissionKind::{Command, Message};
    assert_eq!(
        submitted.lock().unwrap().as_slice(),
        &[
            (Command, "btw".into(), 0, false),
            (Message, "btw this matters".into(), 0, false),
            (Message, "btw".into(), 0, true),
            (Message, "btw".into(), 0, false),
            (Message, "btw".into(), 1, false),
        ]
    );
}

#[gpui::test]
fn ctrl_enter_preserves_images_and_respects_empty_and_disabled_input(cx: &mut TestAppContext) {
    let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = submitted.clone();
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|_, cx| {
        PromptInput::new_with_queue(cx, "test", move |text, images, queued, _, _| {
            seen.lock().unwrap().push((text, images, queued));
        })
    });
    vcx.update(|window, cx| {
        window.focus(&input.read(cx).focus_handle.clone(), cx);
    });
    vcx.simulate_keystrokes("ctrl-enter");
    assert!(submitted.lock().unwrap().is_empty());
    input.update(vcx, |input, cx| {
        input.attachments.push(Attachment {
            label: "4×3".into(),
            payload: AttachmentPayload::Image {
                media_type: "image/png".into(),
                encoded: "cG5n".into(),
                preview: preview_image("image/png", Vec::new()).unwrap(),
            },
        });
        input.set_submission_enabled(false, cx);
    });
    vcx.simulate_keystrokes("ctrl-enter");
    assert!(submitted.lock().unwrap().is_empty());
    input.update(vcx, |input, cx| {
        assert_eq!(input.attachments.len(), 1);
        input.set_submission_enabled(true, cx);
    });
    vcx.simulate_keystrokes("ctrl-enter");
    assert_eq!(
        &*submitted.lock().unwrap(),
        &[(
            "[image]".into(),
            vec![("image/png".into(), "cG5n".into())],
            true
        )]
    );
    input.read_with(vcx, |input, _| {
        assert!(input.attachments.is_empty());
        assert_eq!(input.visual_line_count, 1);
        assert_eq!(input.history, vec!["[image]"]);
    });
}

#[gpui::test]
fn history_and_escape_dispatch_through_the_real_keymap(cx: &mut TestAppContext) {
    let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = submitted.clone();
    cx.update(bind_keys);
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| {
                PromptInput::new(cx, "test", move |text, _, _, _| {
                    seen.lock().unwrap().push(text)
                })
            })
        })
        .unwrap()
    });
    window
        .update(cx, |input, window, cx| {
            window.focus(&input.focus_handle, cx)
        })
        .unwrap();

    cx.simulate_input(*window, "first");
    cx.simulate_keystrokes(*window, "enter");
    cx.simulate_input(*window, "draft");
    cx.simulate_keystrokes(*window, "ctrl-[ ctrl-] up escape");

    assert_eq!(&*submitted.lock().unwrap(), &["first"]);
    window
        .update(cx, |input, _, _| assert!(input.content.is_empty()))
        .unwrap();
}

#[gpui::test]
fn other_sessions_todos_never_become_composer_examples(cx: &mut TestAppContext) {
    let home = std::env::temp_dir().join(format!(
        "jcode-desktop-example-prompts-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(home.join("todos")).unwrap();
    std::fs::write(
        home.join("todos/closed.json"),
        r#"[{"content":"Ship the onboarding flow","status":"pending"},
                {"content":"Fix the parser crash","status":"in_progress"},
                {"content":"Already done","status":"completed"}]"#,
    )
    .unwrap();
    std::fs::write(
        home.join("todos/running.json"),
        r#"[{"content":"Still being worked on","status":"pending"}]"#,
    )
    .unwrap();
    let session = |id: &str, status: &str| {
        let mut info: jcode_sdk::SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": id,
            "status": status,
            "working_dir": "/repo/app",
        }))
        .unwrap();
        info.title = Some(id.into());
        info
    };
    let prompts = crate::example_prompts::load_from(
        &home,
        &[session("closed", "idle"), session("running", "running")],
    );
    std::fs::remove_dir_all(&home).unwrap();

    let (input, vcx) = cx.add_window_view(|_, cx| {
        let mut input =
            PromptInput::new(cx, "Type something…", |_, _, _, _| {}).with_example_prompts();
        input.set_working_dir(Some("/repo/app"));
        input
    });
    vcx.update(|_, cx| crate::example_prompts::set(prompts, cx));
    vcx.run_until_parked();
    let picked = |vcx: &mut gpui::VisualTestContext| {
        input.update(vcx, |input, _| {
            let state = input.motion.borrow();
            let picked = state
                .prompts
                .as_ref()
                .expect("composer painted its examples");
            (picked.prompts[0].to_string(), picked.prompts[1].to_string())
        })
    };
    let (first, second) = picked(vcx);
    assert!(motion::FALLBACK_PROMPTS.contains(&first.as_str()));
    assert!(motion::FALLBACK_PROMPTS.contains(&second.as_str()));

    // A composer in another project falls back to the generic catalog.
    input.update(vcx, |input, cx| {
        input.set_working_dir(Some("/elsewhere"));
        cx.notify();
    });
    vcx.run_until_parked();
    let (first, second) = picked(vcx);
    assert!(
        motion::FALLBACK_PROMPTS.contains(&first.as_str()),
        "{first}"
    );
    assert!(
        motion::FALLBACK_PROMPTS.contains(&second.as_str()),
        "{second}"
    );
}

#[gpui::test]
fn composer_editor_has_no_prompt_prefix_in_compact_or_spacious_mode(cx: &mut TestAppContext) {
    let (input, vcx) =
        cx.add_window_view(|_, cx| PromptInput::new(cx, "Type something…", |_, _, _, _| {}));
    for spacious in [false, true] {
        input.update(vcx, |input, cx| {
            input.spacious = spacious;
            cx.notify();
        });
        vcx.run_until_parked();
        let composer = vcx.debug_bounds("prompt-input").unwrap();
        let editor = vcx.debug_bounds("prompt-editor").unwrap();
        let left = editor.left() - composer.left();
        let right = composer.right() - editor.right();
        assert!(
            f32::from(left - right).abs() < 1.0,
            "editor must not reserve extra left padding for a prompt marker"
        );
    }
}

#[gpui::test]
fn prompt_stash_survives_snapshot_restart(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    vcx.update(|window, cx| input.update(cx, |input, cx| window.focus(&input.focus_handle, cx)));
    vcx.simulate_keystrokes("a ctrl-s");
    let encoded = input.read_with(vcx, |input, _| {
        serde_json::to_string(&input.snapshot()).unwrap()
    });
    let snapshot = serde_json::from_str(&encoded).unwrap();
    let (restarted, vcx) =
        cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    vcx.update(|window, cx| {
        restarted.update(cx, |input, cx| {
            input.restore(snapshot, cx);
            window.focus(&input.focus_handle, cx);
        })
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-stashed").is_some());
    vcx.simulate_keystrokes("ctrl-s");
    restarted.read_with(vcx, |input, _| assert_eq!(input.content.as_ref(), "a"));
}

#[gpui::test]
fn prompt_stash_returns_after_send(cx: &mut TestAppContext) {
    let window = input_window(cx);
    cx.simulate_input(*window, "saved draft");
    cx.simulate_keystrokes(*window, "ctrl-s");
    window
        .update(cx, |input, _, _| assert!(input.content.is_empty()))
        .unwrap();
    cx.simulate_input(*window, "send this");
    cx.simulate_keystrokes(*window, "enter");
    window
        .update(cx, |input, _, _| {
            assert_eq!(input.content.as_ref(), "saved draft")
        })
        .unwrap();
}

#[gpui::test]
fn prompt_stash_swaps_and_keeps_indicator(cx: &mut TestAppContext) {
    cx.update(bind_keys);
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    vcx.update(|window, cx| input.update(cx, |input, cx| window.focus(&input.focus_handle, cx)));
    vcx.simulate_keystrokes("a ctrl-s");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("prompt-stashed").is_some());
    vcx.simulate_keystrokes("b ctrl-s");
    input.update(vcx, |input, _| {
        assert_eq!(input.content.as_ref(), "a");
        assert_eq!(input.stashed_prompt.as_ref().unwrap().0, "b");
    });
    vcx.simulate_keystrokes("ctrl-s");
    input.update(vcx, |input, _| assert_eq!(input.content.as_ref(), "b"));
}

#[gpui::test]
fn submitting_wrapped_prompt_resets_height_before_the_next_paint(cx: &mut TestAppContext) {
    let (input, vcx) = cx.add_window_view(|_, cx| PromptInput::new(cx, "test", |_, _, _, _| {}));
    vcx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.content = "A wrapped prompt occupying several visual lines".into();
            input.visual_line_count = 12;
            input.submit(&Submit, window, cx);
            assert!(input.content.is_empty());
            assert_eq!(
                input.visual_line_count, 1,
                "cleared editor retained old wrapped height"
            );
        });
    });
}

#[gpui::test]
fn image_only_prompt_submits_the_attachment_and_clears_the_composer(cx: &mut TestAppContext) {
    let submitted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = submitted.clone();
    cx.update(bind_keys);
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| {
                PromptInput::new(cx, "test", move |text, images, _, _| {
                    seen.lock().unwrap().push((text, images));
                })
            })
        })
        .unwrap()
    });
    window
        .update(cx, |input, window, cx| {
            input.attachments.push(Attachment {
                label: "4×3".into(),
                payload: AttachmentPayload::Image {
                    media_type: "image/png".into(),
                    encoded: "cG5n".into(),
                    preview: preview_image("image/png", Vec::new()).unwrap(),
                },
            });
            window.focus(&input.focus_handle, cx);
        })
        .unwrap();

    cx.simulate_keystrokes(*window, "enter");

    assert_eq!(
        &*submitted.lock().unwrap(),
        &[(
            "[image]".to_string(),
            vec![("image/png".to_string(), "cG5n".to_string())]
        )]
    );
    window
        .update(cx, |input, _, _| {
            assert!(input.content.is_empty());
            assert!(input.attachments.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn removing_an_attachment_updates_the_visible_count(cx: &mut TestAppContext) {
    let window = input_window(cx);
    window
        .update(cx, |input, _, cx| {
            for label in ["4×3", "8×6"] {
                input.attachments.push(Attachment {
                    label: label.into(),
                    payload: AttachmentPayload::Image {
                        media_type: "image/png".into(),
                        encoded: "cG5n".into(),
                        preview: preview_image("image/png", Vec::new()).unwrap(),
                    },
                });
            }
            input.remove_attachment(0, cx);
            assert_eq!(input.attachments.len(), 1);
            assert_eq!(input.attachments[0].label.as_ref(), "8×6");
            assert_eq!(input.attachment_notice.as_deref(), None);
        })
        .unwrap();
}
