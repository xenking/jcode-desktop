//! Switching folders from a panel's location pill.
//!
//! The runtime fixes a session's working directory when the session is
//! created. An untouched session can therefore be replaced in place, keeping
//! its panel, focus and draft. A conversation that already has history keeps
//! its folder, and the chosen one opens beside it as a new session.
use super::folder_picker::{Chosen, FolderPicker};
use super::*;

impl Workspace {
    /// Open Jcode's own directory browser. This deliberately does not call the
    /// platform path prompt, so the workflow stays inside the application.
    /// A session's working directory is fixed by the runtime, so choosing a
    /// different folder intentionally opens a new session instead of silently
    /// changing the meaning of an existing transcript.
    pub(super) fn open_folder(
        &mut self,
        _: &OpenFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_folder_picker(None, None, window, cx);
    }

    /// `start` is where browsing begins, and `source` the panel whose folder
    /// the choice replaces.
    pub(super) fn show_folder_picker(
        &mut self,
        source: Option<gpui::EntityId>,
        start: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remove_default_directory_panel(cx);
        let dir = start.unwrap_or_else(|| {
            self.default_working_dir()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"))
        });
        let workspace = cx.weak_entity();
        let picker = cx.new(|cx| FolderPicker::new(workspace, false, dir, source, cx));
        cx.subscribe(&picker, |this, _, chosen: &Chosen, cx| {
            this.open_chosen_folder(chosen, cx)
        })
        .detach();
        self.modal_layer
            .update(cx, |layer, cx| layer.show(picker, window, cx));
    }

    /// The picker's choice: replace the folder of the panel that asked, or
    /// open a new session there.
    fn open_chosen_folder(&mut self, chosen: &Chosen, cx: &mut Context<Self>) {
        // Enter removes the focused search before the runtime responds.
        // Restore a mounted target on the next render so navigation keeps
        // working even if session creation is slow or fails.
        self.focus_pending = true;
        if !self.switch_source_directory(chosen.source, &chosen.directory, cx) {
            self.open_local_draft(Some(chosen.directory.clone()), cx);
        }
        cx.notify();
    }

    pub(super) fn switch_directory(
        &mut self,
        request: &SwitchDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| !slot.closing && slot.panel.entity_id() == request.source)
        else {
            return;
        };
        let current = self.slots[index].panel.working_dir(cx).map(str::to_owned);
        self.set_active(index, cx);
        // Start beside the current folder, where sibling repositories live.
        let start = current
            .as_deref()
            .map(Path::new)
            .and_then(|dir| dir.parent().filter(|parent| parent.is_dir()))
            .map(Path::to_path_buf);
        self.show_folder_picker(Some(request.source), start, window, cx);
    }

    /// Point the panel that opened the browser at `directory`. Returns false
    /// when the caller should open an ordinary new session instead.
    pub(super) fn switch_source_directory(
        &mut self,
        source: Option<gpui::EntityId>,
        directory: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = source.and_then(|source| {
            self.slots
                .iter()
                .position(|slot| !slot.closing && slot.panel.entity_id() == source)
        }) else {
            return false;
        };
        let Some(panel) = self.slots[index].panel.chat().cloned() else {
            return false;
        };
        self.set_active(index, cx);
        if panel.read(cx).working_dir.as_deref() == Some(directory) {
            return true;
        }
        if !panel.read(cx).can_switch_directory_in_place() {
            // New sessions open to the right of the active panel, so the
            // chosen folder lands next to the conversation it came from.
            return false;
        }
        let request_id = if panel.read(cx).session_deferred() {
            panel.read(cx).session_id.clone()
        } else {
            pending::next_draft_id()
        };
        let previous = panel.update(cx, |panel, cx| {
            panel.retarget_draft(request_id.clone(), directory.to_owned(), cx)
        });
        for (draft, _) in &mut self.pending_collection_drafts {
            if *draft == previous {
                draft.clone_from(&request_id);
            }
        }
        // A draft that was still being created is unwatched when its reply
        // finds no panel. An established one is released here.
        if !Panel::is_pending_session_id(&previous) {
            self.bridge.send(Command::Unwatch {
                session_id: previous,
            });
        }
        // The retargeted draft creates its session with its first prompt.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    fn setup<'a>(
        cx: &'a mut gpui::TestAppContext,
        current: &Path,
    ) -> (
        Entity<Workspace>,
        &'a mut gpui::VisualTestContext,
        Receiver<Command>,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let current = current.to_string_lossy().into_owned();
        let (workspace, vcx) = cx.add_window_view(|window, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.remotes.default_host = None;
            workspace.push_test_panel("existing", cx);
            workspace.slots[0].panel.update_chat(cx, |panel, cx| {
                panel.working_dir = Some(current);
                panel.load_history(Vec::new(), Vec::new(), cx);
            });
            workspace.restore_focus(window, cx);
            workspace
        });
        vcx.run_until_parked();
        while commands.try_recv().is_ok() {}
        (workspace, vcx, commands)
    }

    fn click_location(vcx: &mut gpui::VisualTestContext) {
        let pill = vcx
            .debug_bounds("composer-location")
            .expect("the location pill paints");
        vcx.simulate_click(pill.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
    }

    #[gpui::test]
    fn clicking_the_location_switches_an_untouched_session_in_place(cx: &mut gpui::TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("alpha");
        let other = root.path().join("beta");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let (workspace, vcx, commands) = setup(cx, &current);

        click_location(vcx);
        workspace.update(vcx, |workspace, cx| {
            assert_eq!(workspace.picker_dir(cx).as_deref(), Some(root.path()));
            assert!(workspace.picker_source(cx).is_some());
        });
        assert!(vcx.debug_bounds("folder-picker-overlay").is_some());

        vcx.simulate_keystrokes("b e t a enter");
        vcx.run_until_parked();
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::Unwatch { session_id }) if session_id == "existing"
        ));
        assert!(
            commands.try_recv().is_err(),
            "the new folder gets a session with its first prompt"
        );
        workspace.update(vcx, |workspace, cx| workspace.request_deferred_sessions(cx));
        let request_id = match commands.try_recv() {
            Ok(Command::CreateSession {
                working_dir,
                request_id: Some(id),
            }) => {
                assert_eq!(working_dir.as_deref(), other.to_str());
                id
            }
            _ => panic!("switching must create a session in the chosen folder"),
        };
        workspace.update(vcx, |workspace, cx| {
            assert_eq!(workspace.slots.len(), 1, "the panel is reused");
            assert!(workspace.picker_dir(cx).is_none());
            assert!(workspace.picker_source(cx).is_none());
            let panel = workspace.slots[0].panel.read(cx);
            assert_eq!(panel.session_id, request_id);
            assert_eq!(panel.working_dir.as_deref(), other.to_str());
        });
    }

    #[gpui::test]
    fn a_conversation_with_history_keeps_its_folder(cx: &mut gpui::TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("alpha");
        let other = root.path().join("beta");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let (workspace, vcx, commands) = setup(cx, &current);
        workspace.update(vcx, |workspace, cx| {
            workspace.slots[0].panel.update_chat(cx, |panel, cx| {
                *panel.items = vec![crate::panel::Item::User("Hello".into())];
                cx.notify();
            });
        });
        vcx.run_until_parked();

        click_location(vcx);
        vcx.simulate_keystrokes("b e t a enter");
        vcx.run_until_parked();
        workspace.update(vcx, |workspace, cx| workspace.request_deferred_sessions(cx));
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::CreateSession { working_dir, .. }) if working_dir.as_deref() == other.to_str()
        ));
        workspace.update(vcx, |workspace, cx| {
            assert_eq!(workspace.slots.len(), 2, "the folder opens beside it");
            let original = workspace.slots[0].panel.read(cx);
            assert_eq!(original.session_id, "existing");
            assert_eq!(original.working_dir.as_deref(), current.to_str());
            assert_eq!(workspace.active, 1);
            assert_eq!(workspace.slots[1].panel.working_dir(cx), other.to_str());
        });
    }

    #[gpui::test]
    fn cancelling_leaves_the_session_untouched(cx: &mut gpui::TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("alpha");
        std::fs::create_dir_all(&current).unwrap();
        let (workspace, vcx, commands) = setup(cx, &current);

        click_location(vcx);
        let cancel = vcx.debug_bounds("folder-picker-cancel").unwrap();
        vcx.simulate_click(cancel.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(commands.try_recv().is_err());
        workspace.update(vcx, |workspace, cx| {
            assert!(workspace.picker_source(cx).is_none());
            assert_eq!(workspace.slots[0].panel.session_id(cx), "existing");
        });
    }
}
