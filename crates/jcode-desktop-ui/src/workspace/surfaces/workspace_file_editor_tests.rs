use super::*;

#[gpui::test]
fn opened_file_accepts_native_input_and_saves_atomically(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {\r\n}\r\n").unwrap();
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("project", cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_code_file(path.clone(), window, cx);
        });
    });
    vcx.run_until_parked();
    let editor = vcx
        .debug_bounds("code-editor-text")
        .expect("file opens in an editor");
    vcx.simulate_click(
        gpui::point(editor.left() + px(200.0), editor.top() + px(10.0)),
        gpui::Modifiers::default(),
    );
    vcx.simulate_keystrokes("end enter");
    vcx.simulate_input("println!(\"Привет\");");
    vcx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "fn main() {\r\n}\r\n"
    );
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    });
    vcx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "fn main() {\r\n    println!(\"Привет\");\r\n}\r\n"
    );

    vcx.simulate_input(" // unsaved");
    vcx.run_until_parked();
    vcx.dispatch_action(ClosePanel);
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("code-editor-text").is_some());
    vcx.dispatch_action(ClosePanel);
    vcx.run_until_parked();
    assert!(workspace.read_with(vcx, |workspace, cx| {
        workspace
            .slots
            .iter()
            .all(|slot| slot.closing || !slot.panel.session_id(cx).starts_with("file://"))
    }));
    assert!(!std::fs::read_to_string(&path).unwrap().contains("unsaved"));
}

#[gpui::test]
fn markdown_preview_tracks_source_edits_without_opening_a_second_panel(
    cx: &mut gpui::TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("README.md");
    std::fs::write(&path, "# Original\n").unwrap();
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) =
        cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_code_file(path.clone(), window, cx)
        });
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("markdown-file-preview").is_some());
    let toggle = vcx.debug_bounds("toggle-file-preview").unwrap().center();
    vcx.simulate_click(toggle, gpui::Modifiers::default());
    vcx.run_until_parked();
    let editor = vcx.debug_bounds("code-editor-text").unwrap().center();
    vcx.simulate_click(editor, gpui::Modifiers::default());
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    vcx.simulate_keystrokes(&format!("{modifier}-a"));
    vcx.simulate_input("# Updated");
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("code-file-badge").is_some());
    let toggle = vcx.debug_bounds("toggle-file-preview").unwrap().center();
    vcx.simulate_click(toggle, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("markdown-file-preview").is_some());
    assert!(vcx.debug_bounds("code-editor-text").is_none());
    assert_eq!(
        workspace.read_with(vcx, |workspace, _| workspace.slots.len()),
        1
    );
    vcx.simulate_keystrokes(&format!("{modifier}-s"));
    vcx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Updated");
    assert!(vcx.debug_bounds("code-file-badge").is_none());
}

fn open_file_workspace<'a>(
    path: &std::path::Path,
    cx: &'a mut gpui::TestAppContext,
) -> (Entity<Workspace>, &'a mut gpui::VisualTestContext) {
    cx.update(crate::bind_workspace_keys);
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.push_test_panel("project", cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_code_file(path.to_path_buf(), window, cx);
        });
    });
    vcx.run_until_parked();
    (workspace, vcx)
}

#[gpui::test]
fn opened_source_file_takes_focus_and_is_listed_once(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let (workspace, vcx) = open_file_workspace(&path, cx);
    let id = format!("file://{}", path.display());
    vcx.update(|window, cx| {
        workspace.read_with(cx, |workspace, cx| {
            assert_eq!(workspace.slots.len(), 2);
            let slot = &workspace.slots[workspace.active];
            assert_eq!(slot.panel.session_id(cx), id);
            assert_eq!(
                slot.panel.profile().working_dir(cx),
                path.parent().and_then(|dir| dir.to_str())
            );
            assert!(slot.panel.surface().input_focus_handle(cx).is_focused(window));
        });
        // Opening the same file again activates the open panel.
        workspace.update(cx, |workspace, cx| {
            workspace.open_code_file(path.clone(), window, cx);
        });
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), 2);
        let open = workspace
            .slots
            .iter()
            .filter(|slot| slot.panel.session_id(cx) == id)
            .count();
        assert_eq!(open, 1);
    });
}

#[gpui::test]
fn opened_file_survives_hot_reload_snapshot(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let (workspace, vcx) = open_file_workspace(&path, cx);
    let id = format!("file://{}", path.display());
    let saved = vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            let snapshot = workspace.snapshot_inner(window, cx, true).unwrap();
            assert!(
                snapshot
                    .slots
                    .iter()
                    .any(|slot| slot.panel.session_id == id)
            );
            workspace.apply_snapshot(snapshot.clone(), cx);
            workspace.restore_focus(window, cx);
            snapshot
        })
    });
    vcx.run_until_parked();
    workspace.read_with(vcx, |workspace, cx| {
        assert_eq!(workspace.slots.len(), saved.slots.len());
        let restored = workspace
            .slots
            .iter()
            .find(|slot| slot.panel.session_id(cx) == id)
            .expect("file panel restored");
        assert_eq!(
            restored.panel.profile().working_dir(cx),
            path.parent().and_then(|dir| dir.to_str())
        );
    });
    // The restored panel loads its contents again.
    assert!(vcx.debug_bounds("code-editor-text").is_some());
}
