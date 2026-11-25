//! Terminal slot lifecycle through the workspace: open, focus, surface focus,
//! snapshot and restore across a hot reload, and close.
use super::*;
use crate::terminal::TerminalPanel;

fn terminal_at(workspace: &Workspace, index: usize, cx: &App) -> Entity<TerminalPanel> {
    workspace.slots[index]
        .panel
        .read(cx)
        .test_terminal()
        .expect("slot holds a terminal")
}

fn open_with_terminal(
    cx: &mut gpui::TestAppContext,
) -> (
    Entity<Workspace>,
    &mut gpui::VisualTestContext,
    std::sync::mpsc::Receiver<Command>,
) {
    cx.update(crate::bind_workspace_keys);
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.layout_mode = crate::config::LayoutMode::Normal;
        workspace.push_test_panel("chat-session", cx);
        workspace
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.set_active(0, cx);
            workspace.new_terminal(&NewTerminal, window, cx);
        });
    });
    vcx.run_until_parked();
    while commands.try_recv().is_ok() {}
    (workspace, vcx, commands)
}

fn draw(vcx: &mut gpui::VisualTestContext) {
    vcx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(1600.), px(900.)),
        |_, _| gpui::div(),
    );
    vcx.run_until_parked();
}

#[gpui::test]
fn terminal_slot_opens_focused_and_paints(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, _commands) = open_with_terminal(cx);
    draw(vcx);
    vcx.update(|window, cx| {
        let w = workspace.read(cx);
        assert_eq!(w.slots.len(), 2);
        assert_eq!(w.active, 1, "the terminal opens right of the active panel");
        let state = w.navigation_state(window, cx);
        assert_eq!(state["keyboard_panel"], 1, "the terminal owns keyboard focus");
        let panels = &state["rows"][0]["panels"];
        assert_eq!(panels[1]["session"], "terminal");
        assert_eq!(panels[0]["terminal"], serde_json::Value::Null);
        assert!(panels[1]["terminal"].is_object(), "terminal diagnostics exist");
        let terminal = terminal_at(w, 1, cx);
        assert!(terminal.read(cx).focus_handle(cx).is_focused(window));
        assert!(terminal.read(cx).surface_focused());
        let metadata = w.panel_metadata(&w.slots[1], cx);
        assert_eq!(metadata.session_id, "terminal");
        assert_eq!(metadata.title.as_ref(), "terminal");
        assert_eq!(metadata.working_dir, w.default_working_dir());
        assert!(metadata.terminal);
        assert!(!metadata.supports_voice());
        assert!(!metadata.pending);
        assert!(!metadata.can_rename);
    });
    assert!(vcx.debug_bounds("plain-terminal").is_some());
}

#[gpui::test]
fn terminal_surface_focus_follows_the_active_slot(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, _commands) = open_with_terminal(cx);
    draw(vcx);
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.set_active(0, cx);
            workspace.focus_active(window, cx);
        });
    });
    draw(vcx);
    vcx.update(|window, cx| {
        let w = workspace.read(cx);
        let terminal = terminal_at(w, 1, cx);
        assert!(!terminal.read(cx).surface_focused());
        assert!(!terminal.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(w.navigation_state(window, cx)["keyboard_panel"], 0);
    });
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.set_active(1, cx);
            workspace.focus_active(window, cx);
        });
    });
    draw(vcx);
    vcx.update(|window, cx| {
        let w = workspace.read(cx);
        let terminal = terminal_at(w, 1, cx);
        assert!(terminal.read(cx).surface_focused());
        assert!(terminal.read(cx).focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn terminal_slot_survives_snapshot_and_restore(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, _commands) = open_with_terminal(cx);
    draw(vcx);
    let snapshot = vcx.update(|window, cx| workspace.read(cx).snapshot_for_reload(window, cx).unwrap());
    let saved = &snapshot.slots[1].panel;
    assert_eq!(saved.session_id, "terminal");
    assert_eq!(saved.title, "terminal");
    assert_eq!(
        saved.working_dir,
        workspace.read_with(vcx, |w, _| w.default_working_dir())
    );
    assert_eq!(saved.terminal_resource_id, None, "the inert host never creates a PTY");
    assert_eq!(saved.terminal_output_cursor, Some(0));
    assert_eq!(snapshot.focus, FocusSnapshot::Panel(1));
    assert_eq!(snapshot.active, 1);

    let decoded = WorkspaceSnapshot::decode(&snapshot.encode().unwrap()).unwrap();
    let (restored, rcx) = cx.add_window_view(|_, cx| {
        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
        workspace.layout_mode = crate::config::LayoutMode::Normal;
        workspace
    });
    rcx.update(|window, cx| {
        restored.update(cx, |workspace, cx| {
            workspace.apply_snapshot(decoded, cx);
            workspace.restore_focus(window, cx);
        });
    });
    draw(rcx);
    rcx.update(|window, cx| {
        let w = restored.read(cx);
        assert_eq!(w.slots.len(), 2);
        assert_eq!(w.active, 1);
        let state = w.navigation_state(window, cx);
        assert_eq!(state["rows"][0]["panels"][0]["session"], "chat-session");
        assert_eq!(state["rows"][0]["panels"][1]["session"], "terminal");
        assert_eq!(state["keyboard_panel"], 1, "focus returns to the terminal");
        let terminal = terminal_at(w, 1, cx);
        assert!(terminal.read(cx).focus_handle(cx).is_focused(window));
        let resaved = w.snapshot(window, cx).unwrap();
        assert_eq!(resaved.slots[1].panel.session_id, "terminal");
        assert_eq!(resaved.slots[1].panel.working_dir, snapshot.slots[1].panel.working_dir);
    });
    assert!(rcx.debug_bounds("plain-terminal").is_some());
}

#[gpui::test]
fn closing_a_terminal_slot_never_unwatches_a_session(cx: &mut gpui::TestAppContext) {
    let (workspace, vcx, commands) = open_with_terminal(cx);
    draw(vcx);
    vcx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.close_panel(&ClosePanel, window, cx));
    });
    vcx.run_until_parked();
    while let Ok(command) = commands.try_recv() {
        assert!(
            !matches!(command, Command::Unwatch { .. }),
            "a terminal has no runtime session to unwatch"
        );
    }
    std::thread::sleep(transition::policy(Transition::PanelClose).duration * 2);
    workspace.update(vcx, |_, cx| cx.notify());
    draw(vcx);
    vcx.update(|window, cx| {
        let w = workspace.read(cx);
        assert!(!w.sidebar_closed_sessions.contains("terminal"));
        assert_eq!(w.slots.len(), 1, "the closed terminal slot is removed");
        assert_eq!(w.navigation_state(window, cx)["rows"][0]["panels"][0]["session"], "chat-session");
    });
}
