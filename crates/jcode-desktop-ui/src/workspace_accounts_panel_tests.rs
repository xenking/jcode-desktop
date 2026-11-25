//! Workspace tests: accounts panel.
use super::*;

#[gpui::test]
fn opening_accounts_from_the_active_accounts_panel_reuses_it(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.push_test_panel("source", cx);
        w
    });
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.entity_id());
    workspace.update_in(vcx, |w, window, cx| {
        w.open_accounts(
            &OpenAccounts {
                source,
                login_command: None,
            },
            window,
            cx,
        )
    });
    let accounts = workspace.read_with(vcx, |w, _| w.slots[w.active].panel.clone());
    workspace.update_in(vcx, |w, window, cx| {
        w.open_accounts(
            &OpenAccounts {
                source: accounts.entity_id(),
                login_command: None,
            },
            window,
            cx,
        )
    });
    workspace.read_with(vcx, |w, _| {
        assert_eq!(w.slots.iter().filter(|s| !s.closing).count(), 2);
        assert_eq!(w.slots[w.active].panel, accounts);
    });
}

#[gpui::test]
fn accounts_panel_preview_and_pending_sources_never_contact_runtime(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.set_test_bridge(bridge);
        w.push_test_panel("preview://empty", cx);
        w.slots[0].panel.update_chat(cx, |panel, _| {
            panel.preview_state = Some(crate::preview_state::PreviewState::Empty);
        });
        w
    });
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
    vcx.run_until_parked();
    // The method pill selects a provider inline. Accounts opens explicitly.
    let request = OpenAccounts {
        source: source.entity_id(),
        login_command: None,
    };
    workspace.update_in(vcx, |w, window, cx| w.open_accounts(&request, window, cx));
    vcx.run_until_parked();
    let accounts = workspace.read_with(vcx, |w, cx| {
        let accounts = w.slots[w.active].panel.clone();
        assert!(accounts.read(cx).is_accounts_panel());
        assert_eq!(
            accounts.read(cx).preview_state,
            source.read(cx).preview_state
        );
        assert!(!source.read(cx).can_refresh_account_runtime());
        accounts
    });
    let provider = vcx
        .debug_bounds("login-provider-openai-api")
        .expect("offline provider");
    vcx.simulate_click(provider.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    vcx.simulate_input("offline-only-not-a-credential");
    let submit = vcx.debug_bounds("login-submit").expect("offline submit");
    vcx.simulate_click(submit.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("login-error").is_some());
    accounts.update(vcx, |_, cx| cx.emit(crate::panel::AccountsPanelChooseModel));
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "preview auth and closing must remain offline"
    );
    assert!(source.read_with(vcx, |panel, _| panel.items.is_empty()));
    source.update(vcx, |panel, _| {
        panel.preview_state = None;
        panel.session_id = Panel::STARTUP_SESSION_ID.into();
    });
    let request = OpenAccounts {
        source: source.entity_id(),
        login_command: None,
    };
    workspace.update_in(vcx, |w, window, cx| w.open_accounts(&request, window, cx));
    let accounts = workspace.read_with(vcx, |w, _| w.slots[w.active].panel.clone());
    accounts.update(vcx, |_, cx| cx.emit(crate::panel::AccountsPanelClosed));
    vcx.run_until_parked();
    assert!(
        commands.try_recv().is_err(),
        "pending drafts have no runtime to refresh"
    );
}

#[test]
fn home_directory_labels_are_explicit_and_children_stay_compact() {
    let home = std::env::var("HOME").expect("test home");
    for format in [compact_working_dir, default_directory::compact_path] {
        assert_eq!(format(&home), "~ (home)");
        assert_eq!(format(&format!("{home}/project")), "~/project");
        assert_eq!(format(&format!("{home}-other")), format!("{home}-other"));
    }
}

#[gpui::test]
fn context_file_chip_opens_the_markdown_preview(cx: &mut gpui::TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let report = directory.path().canonicalize().unwrap().join("file-report.md");
    std::fs::write(&report, "# Opened through the composer\n").unwrap();
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("source", cx);
        workspace
    });
    vcx.update(|_, cx| cx.set_reduce_motion(true));
    vcx.simulate_resize(gpui::size(px(1200.), px(800.)));
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.chat().cloned().unwrap());
    source.update_in(vcx, |panel, window, cx| {
        panel.working_dir = Some(report.parent().unwrap().display().to_string());
        panel.focus_input(window, cx);
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.simulate_input("Read @file-report");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    let chip = vcx.debug_bounds("composer-image-chip").expect("file reference in prompt");
    vcx.simulate_click(chip.center(), Default::default());
    vcx.run_until_parked();
    workspace.update(vcx, |w, cx| {
        let row = w.active_row;
        w.camera_x[row] = w.camera_target[row];
        w.camera_started[row] = None;
        cx.notify();
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots.len(), 2);
        let id = format!("file://{}", report.display());
        assert!(w.slots.iter().any(|slot| slot.panel.session_id(cx) == id));
    });
    assert!(vcx.debug_bounds("markdown-file-preview").is_some());
}

#[gpui::test]
fn transcript_file_mentions_open_native_previews(cx: &mut gpui::TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    std::fs::create_dir(base.join("docs")).unwrap();
    for name in ["report.md", "details.md", "dm.md"] {
        std::fs::write(base.join("docs").join(name), "# Report\n").unwrap();
    }
    std::fs::write(base.join("notes.txt"), "Text preview\n").unwrap();
    cx.update(crate::bind_workspace_keys);

    for (item, selector, path, preview) in [
        (
            crate::panel::Item::Assistant("`docs/report.md`\n\n`docs/missing.md`".into()),
            "markdown-link:docs/report.md",
            "docs/report.md",
            "markdown-file-preview",
        ),
        (
            crate::panel::Item::Tool {
                call_id: "final".into(),
                name: "user_response".into(),
                input: r#"{"title":"Final","content":"`docs/details.md:12`"}"#.into(),
                output: String::new(),
                done: true,
                error: None,
            },
            "markdown-link:docs/details.md:12",
            "docs/details.md",
            "markdown-file-preview",
        ),
        (
            crate::panel::Item::AgentMessage {
                from_session: "peer".into(),
                from_name: None,
                message: "docs/dm.md.".into(),
                timestamp_ms: None,
            },
            "markdown-link:docs/dm.md",
            "docs/dm.md",
            "markdown-file-preview",
        ),
        (
            crate::panel::Item::Assistant("`notes.txt:12`".into()),
            "markdown-link:notes.txt:12",
            "notes.txt",
            "code-file-contents",
        ),
    ] {
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel("source", cx);
            workspace
        });
        vcx.update(|_, cx| cx.set_reduce_motion(true));
        vcx.simulate_resize(gpui::size(px(1200.), px(800.)));
        let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
        source.update(vcx, |panel, cx| {
            panel.working_dir = Some(base.display().to_string());
            panel.items.push(item);
            cx.notify();
        });
        vcx.run_until_parked();
        if let Some(summary) = vcx.debug_bounds("agent-message-summary") {
            vcx.simulate_click(summary.center(), Default::default());
            vcx.run_until_parked();
        }
        if let Some(tool) = vcx.debug_bounds("tool-header") {
            vcx.simulate_click(tool.center(), Default::default());
            vcx.run_until_parked();
            assert!(vcx.debug_bounds("user-response-card").is_some());
        }
        assert!(vcx.debug_bounds("markdown-link:docs/missing.md").is_none());
        let bounds = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("expected a rendered {selector}"));
        let pointer = gpui::point(bounds.left() + px(28.), bounds.center().y);
        if selector == "markdown-link:docs/report.md" {
            let has_underline = |vcx: &mut gpui::VisualTestContext| {
                vcx.update(|window, _| {
                    let scale = window.scale_factor();
                    let bounds = bounds.scale(scale);
                    let pointer = pointer.scale(scale);
                    window.painted_quads().iter().any(|quad| {
                        quad.bounds.size.height == gpui::ScaledPixels::from(scale)
                            && bounds.contains(&quad.bounds.center())
                            && quad.bounds.left() <= pointer.x
                            && pointer.x <= quad.bounds.right()
                    })
                })
            };
            assert!(!has_underline(vcx));
            vcx.simulate_mouse_move(pointer, None, Default::default());
            vcx.run_until_parked();
            assert!(has_underline(vcx));
            vcx.simulate_mouse_move(
                gpui::point(pointer.x, bounds.bottom() + px(12.)),
                None,
                Default::default(),
            );
            vcx.run_until_parked();
            assert!(!has_underline(vcx));
        }
        vcx.simulate_click(pointer, Default::default());
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            let row = w.active_row;
            w.camera_x[row] = w.camera_target[row];
            w.camera_started[row] = None;
            cx.notify();
        });
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(
                w.slots[w.active].panel.session_id(cx),
                format!("file://{}", base.join(path).display()),
            );
        });
        assert!(vcx.debug_bounds(preview).is_some(), "{preview}");
    }
}

#[gpui::test]
fn markdown_file_preview_opens_chat_report_and_follows_relative_links(
    cx: &mut gpui::TestAppContext,
) {
    use base64::Engine as _;

    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let report = base.join("report.md");
    let details = base.join("details here.md");
    std::fs::write(
        &report,
        "# Report\n\n![chart](chart.png)\n\n[See details](details%20here.md#results)\n",
    )
    .unwrap();
    std::fs::write(
        &details,
        format!(
            "# Details\n\n{}\n## Results\n\nDone",
            "Paragraph.\n\n".repeat(70)
        ),
    )
    .unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAACAAAAASCAIAAAC1qksFAAAAH0lEQVR4nGPQqn1NU8QwasGoBaMWjFowasGoBfSwAACFt4iuk2SIigAAAABJRU5ErkJggg==")
        .unwrap();
    std::fs::write(base.join("chart.png"), png).unwrap();

    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("source", cx);
        workspace
    });
    vcx.update(|_, cx| cx.set_reduce_motion(true));
    vcx.simulate_resize(gpui::size(px(1200.), px(800.)));
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
    source.update(vcx, |panel, cx| {
        panel.working_dir = Some(base.display().to_string());
        panel.items.push(crate::panel::Item::Assistant(
            "[Open report](report.md)".into(),
        ));
        cx.notify();
    });
    vcx.run_until_parked();
    let click_link = |vcx: &mut gpui::VisualTestContext, selector: &'static str| {
        let bounds = vcx.debug_bounds(selector).unwrap();
        vcx.simulate_click(
            gpui::point(bounds.left() + px(28.), bounds.center().y),
            gpui::Modifiers::default(),
        );
        vcx.run_until_parked();
        workspace.update(vcx, |w, cx| {
            let row = w.active_row;
            w.camera_x[row] = w.camera_target[row];
            w.camera_started[row] = None;
            cx.notify();
        });
        vcx.run_until_parked();
    };
    click_link(vcx, "markdown-link:report.md");
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(
            w.slots[w.active].panel.session_id(cx),
            format!("file://{}", report.display())
        );
    });
    assert!(vcx.debug_bounds("markdown-file-preview").is_some());
    assert!(vcx.debug_bounds("markdown-file-image").is_some());

    for selector in ["code-file-contents", "markdown-file-preview"] {
        let toggle = vcx.debug_bounds("toggle-file-preview").unwrap();
        vcx.simulate_click(toggle.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds(selector).is_some(), "{selector}");
    }

    click_link(vcx, "markdown-link:details%20here.md#results");
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots.len(), 3);
        assert_eq!(
            w.slots[w.active].panel.session_id(cx),
            format!("file://{}", details.display())
        );
    });
    let document = vcx.debug_bounds("markdown-file-preview").unwrap();
    let results_heading = vcx.debug_bounds("selectable-text-file-71").unwrap();
    assert!(document.contains(&results_heading.center()));
}

#[gpui::test]
fn accounts_panel_footer_opens_new_adjacent_preserves_source_and_escape_returns(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("source", cx);
        workspace
    });
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
    source.update(vcx, |panel, cx| {
        panel.items.push(crate::panel::Item::Assistant(
            "Keep this conversation".into(),
        ));
        panel
            .input
            .update(cx, |input, cx| input.set_content("unsent draft".into(), cx));
        cx.notify();
    });
    vcx.run_until_parked();
    let before = source.read_with(vcx, |panel, cx| panel.snapshot(cx));
    // The method pill selects a provider inline. Accounts opens explicitly.
    let request = OpenAccounts {
        source: source.entity_id(),
        login_command: None,
    };
    workspace.update_in(vcx, |w, window, cx| w.open_accounts(&request, window, cx));
    vcx.run_until_parked();
    let accounts = workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots.len(), 2);
        assert_eq!(w.slots[0].panel, source);
        assert_eq!(w.active, 1);
        assert_eq!(w.slots[0].row, w.slots[1].row);
        assert!(
            w.slots[1]
                .panel
                .read_chat(cx)
                .is_some_and(Panel::is_accounts_panel)
        );
        assert!(!w.slots[1].panel.read_chat(cx).is_some_and(Panel::can_fork));
        w.slots[1].panel.clone()
    });
    assert!(vcx.debug_bounds("accounts-panel").is_some());
    assert!(vcx.debug_bounds("login-dialog").is_some());
    vcx.update(|window, cx| {
        assert!(accounts.read(cx).input_focus_handle(cx).is_focused(window));
        let snapshot = workspace.read(cx).snapshot(window, cx).unwrap();
        assert_eq!(snapshot.slots.len(), 1);
        assert_eq!(snapshot.active, 0);
    });
    assert!(
        commands.try_recv().is_err(),
        "opening accounts must not contact harness"
    );
    vcx.simulate_keystrokes("x escape");
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| assert_eq!(w.slots[w.active].panel, source));
    vcx.update(|window, cx| assert!(source.read(cx).input_focus_handle(cx).is_focused(window)));
    let after = source.read_with(vcx, |panel, cx| panel.snapshot(cx));
    assert_eq!(before.draft, after.draft);
    assert_eq!(before.scroll_y, after.scroll_y);
    assert_eq!(source.read_with(vcx, |panel, _| panel.items.len()), 1);
    assert!(
        matches!(commands.try_recv(), Ok(Command::RefreshRuntime { session_id }) if session_id == "source")
    );
    assert!(commands.try_recv().is_err());
}

#[gpui::test]
fn accounts_panel_uses_source_row_reuses_live_panel_and_choose_model_returns_to_source(
    cx: &mut gpui::TestAppContext,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.set_test_bridge(bridge);
        w.push_test_panel("source", cx);
        w.slots[0].width_fraction = 1.0;
        w.active_row = 1;
        w.push_test_panel("other-row", cx);
        w.active = 1;
        w
    });
    vcx.simulate_resize(gpui::size(px(1200.), px(800.)));
    let source = workspace.read_with(vcx, |w, _| w.slots[0].panel.clone());
    let request = OpenAccounts {
        source: source.entity_id(),
        login_command: None,
    };
    workspace.update_in(vcx, |w, window, cx| w.open_accounts(&request, window, cx));
    let first = workspace.read_with(vcx, |w, _| {
        assert_eq!(w.active, 1);
        assert_eq!(w.active_row, 0);
        assert_eq!(w.slots[1].row, 0);
        assert!(
            w.slots[0].width_fraction < 1.0,
            "source demotes to show adjacent accounts"
        );
        w.slots[1].panel.clone()
    });
    workspace.update_in(vcx, |w, window, cx| w.open_accounts(&request, window, cx));
    let second = workspace.read_with(vcx, |w, _| {
        assert_eq!(w.slots.len(), 3);
        assert_eq!(w.slots[1].panel, first);
        w.slots[1].panel.clone()
    });
    second.update(vcx, |_, cx| cx.emit(crate::panel::AccountsPanelChooseModel));
    vcx.run_until_parked();
    vcx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| div(),
    );
    workspace.read_with(vcx, |w, cx| {
        assert_eq!(w.slots[w.active].panel, source);
        assert!(
            !w.slots
                .iter()
                .any(|slot| slot.panel == first && !slot.closing)
        );
        assert_eq!(w.active_row, 0);
        assert!(!source.read(cx).is_accounts_panel());
    });
    assert!(vcx.debug_bounds("recovery-model-picker").is_some());
    assert!(
        matches!(commands.try_recv(), Ok(Command::RefreshRuntime { session_id }) if session_id == "source")
    );
    assert!(commands.try_recv().is_err());
    workspace.update_in(vcx, |w, window, cx| {
        // GPUI's executor does not advance these wall-clock animations.
        // Remove the outgoing accounts surface before opening another one,
        // otherwise its duplicate login-close selector can win debug_bounds.
        let settled = Instant::now() + Duration::from_secs(1);
        w.remove_finished_closing_panels(settled, window, cx);
        w.open_accounts(&request, window, cx);
        w.row_progress.sample(settled);
        for slot in &mut w.slots {
            slot.animated_width.sample(settled);
        }
        w.camera_snap_pending[w.active_row] = true;
        cx.notify();
    });
    vcx.run_until_parked();
    vcx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1200.), px(800.)),
        |_, _| div(),
    );
    let close = vcx
        .debug_bounds("login-close")
        .expect("dedicated Close button paints");
    assert!(
        close.left() >= px(0.) && close.right() <= px(1200.),
        "Close must be inside the resized viewport: {close:?}"
    );
    vcx.simulate_click(close.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    workspace.read_with(vcx, |w, _| assert_eq!(w.slots[w.active].panel, source));
    assert!(
        matches!(commands.try_recv(), Ok(Command::RefreshRuntime { session_id }) if session_id == "source")
    );
    assert!(commands.try_recv().is_err());
}
