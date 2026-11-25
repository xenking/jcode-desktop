//! Workspace chrome observes a projection of each panel, not its transcript.
//! Streaming text and transcript scrolling do not replace stable chrome metadata.
use super::*;
use crate::panel_item::changelog_panel::ChangelogPanel;
use crate::panel_item::file_panel::FilePanel;
use crate::terminal::TerminalPanel;
use gpui::Focusable;

pub(super) struct SidebarPanelMetadata {
    pub session_id: String,
    pub title: gpui::SharedString,
    pub working_dir: Option<String>,
    pub status: String,
    pub overview_preview: String,
    pub listable: bool,
    voice: bool,
    activity: Option<gpui::AnyView>,
    todos: Option<(usize, usize)>,
    pub minimap_state: crate::panel::MinimapSessionState,
    pub tab_ring: Option<crate::panel::TabRing>,
    pub tab_outline: Option<gpui::AnyView>,
    pub keyboard_focus: [Option<FocusHandle>; 4],
    pub openrig: bool,
    pub terminal: bool,
    pub panel_focus: FocusHandle,
    pub surface_focused: bool,
    pub finished_unseen: bool,
    pub pending: bool,
    pub can_rename: bool,
}

impl SidebarPanelMetadata {
    fn overview_preview(panel: &Panel) -> &str {
        let text = panel
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                crate::panel::Item::Assistant(text) | crate::panel::Item::User(text) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .unwrap_or_default();
        let end = text
            .char_indices()
            .nth(220)
            .map_or(text.len(), |(offset, _)| offset);
        &text[..end]
    }

    fn from_panel(panel: &Panel, cx: &App) -> Self {
        let (keyboard_focus, panel_focus) = panel.workspace_focus_handles(cx);
        let (surface_focused, finished_unseen, openrig) = panel.workspace_surface_state();
        Self {
            session_id: panel.session_id.clone(),
            title: panel.title.clone(),
            working_dir: panel.working_dir.clone(),
            status: panel.status.clone(),
            overview_preview: Self::overview_preview(panel).into(),
            listable: panel.listable_conversation(cx),
            voice: panel.supports_voice(),
            activity: panel.sidebar_activity(),
            todos: panel.latest_todo_progress(),
            minimap_state: panel.minimap_state(),
            tab_ring: panel.tab_ring(),
            tab_outline: Some(panel.tab_outline()),
            keyboard_focus,
            panel_focus,
            surface_focused,
            finished_unseen,
            openrig,
            terminal: false,
            pending: panel.is_pending_session(),
            can_rename: panel.can_fork()
                && (!panel.session_id.contains("://")
                    || harness::is_remote_session(&panel.session_id)),
        }
    }

    fn from_terminal(terminal: &TerminalPanel, cx: &App) -> Self {
        let focus = terminal.focus_handle(cx);
        Self {
            session_id: PanelItem::TERMINAL_SESSION_ID.into(),
            title: "terminal".into(),
            working_dir: terminal.working_dir().map(str::to_owned),
            status: "idle".into(),
            overview_preview: String::new(),
            listable: false,
            voice: false,
            activity: None,
            todos: None,
            minimap_state: crate::panel::MinimapSessionState::Idle,
            tab_ring: None,
            tab_outline: None,
            keyboard_focus: [Some(focus.clone()), None, None, None],
            panel_focus: focus,
            surface_focused: terminal.surface_focused(),
            finished_unseen: false,
            openrig: false,
            terminal: true,
            pending: false,
            can_rename: false,
        }
    }

    /// Chrome for a kind with no session: idle, never renamed, nothing to run.
    fn local(
        session_id: &str,
        title: gpui::SharedString,
        working_dir: Option<&str>,
        input_focus: FocusHandle,
        selection_focus: FocusHandle,
        panel_focus: FocusHandle,
        surface_focused: bool,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            title,
            working_dir: working_dir.map(str::to_owned),
            status: "idle".into(),
            overview_preview: String::new(),
            listable: true,
            voice: false,
            activity: None,
            todos: None,
            minimap_state: crate::panel::MinimapSessionState::Idle,
            tab_ring: None,
            tab_outline: None,
            keyboard_focus: [None, Some(input_focus), None, Some(selection_focus)],
            panel_focus,
            surface_focused,
            finished_unseen: false,
            openrig: false,
            terminal: false,
            pending: false,
            can_rename: false,
        }
    }

    fn from_file(file: &FilePanel, cx: &App) -> Self {
        Self::local(
            file.session_id(),
            file.title().clone(),
            file.working_dir(),
            file.input_focus_handle(cx),
            file.selection_focus_handle(cx),
            file.panel_focus_handle().clone(),
            file.surface_focused(),
        )
    }

    fn from_changelog(changelog: &ChangelogPanel, cx: &App) -> Self {
        let focus = changelog.focus_handle(cx);
        Self::local(
            ChangelogPanel::SESSION_ID,
            changelog.title(),
            None,
            focus.clone(),
            changelog.selection_focus_handle(cx),
            focus,
            changelog.surface_focused(),
        )
    }

    fn from_item(item: &PanelItem, cx: &App) -> Self {
        match item {
            PanelItem::Chat(panel) => Self::from_panel(panel.read(cx), cx),
            PanelItem::Terminal(terminal) => Self::from_terminal(terminal.read(cx), cx),
            PanelItem::File(file) => Self::from_file(file.read(cx), cx),
            PanelItem::Changelog(changelog) => Self::from_changelog(changelog.read(cx), cx),
        }
    }

    fn matches(&self, item: &PanelItem, cx: &App) -> bool {
        match item {
            PanelItem::Chat(panel) => self.matches_panel(panel.read(cx), cx),
            PanelItem::Terminal(terminal) => {
                self.surface_focused == terminal.read(cx).surface_focused()
            }
            PanelItem::File(file) => {
                let file = file.read(cx);
                self.surface_focused == file.surface_focused()
                    && self.title == *file.title()
                    && self.keyboard_focus[1].as_ref() == Some(&file.input_focus_handle(cx))
            }
            PanelItem::Changelog(changelog) => {
                self.surface_focused == changelog.read(cx).surface_focused()
            }
        }
    }

    fn matches_panel(&self, panel: &Panel, cx: &App) -> bool {
        self.session_id == panel.session_id
            && self.title == panel.title
            && self.working_dir == panel.working_dir
            && self.status == panel.status
            && self.overview_preview == Self::overview_preview(panel)
            && self.listable == panel.listable_conversation(cx)
            && self.voice == panel.supports_voice()
            && self.activity == panel.sidebar_activity()
            && self.todos == panel.latest_todo_progress()
            && self.minimap_state == panel.minimap_state()
            && self.tab_ring == panel.tab_ring()
            && panel.workspace_focus_handles(cx)
                == (self.keyboard_focus.clone(), self.panel_focus.clone())
            && (self.surface_focused, self.finished_unseen, self.openrig)
                == panel.workspace_surface_state()
            && self.pending == panel.is_pending_session()
            && self.can_rename
                == (panel.can_fork()
                    && (!panel.session_id.contains("://")
                        || harness::is_remote_session(&panel.session_id)))
    }

    pub fn supports_voice(&self) -> bool {
        self.voice
    }
    pub fn sidebar_activity(&self) -> Option<gpui::AnyView> {
        self.activity.clone()
    }
    pub fn latest_todo_progress(&self) -> Option<(usize, usize)> {
        self.todos
    }
}

impl Workspace {
    pub(super) fn panel_metadata<'a>(&self, slot: &Slot, cx: &'a App) -> &'a SidebarPanelMetadata {
        &self.sidebar_panel_metadata.as_ref().unwrap().read(cx)[&slot.panel.entity_id()]
    }

    /// Observe panel notifications outside drawing. Only metadata changes
    /// invalidate the workspace, not updates to the transcript or composer.
    pub(super) fn refresh_sidebar_panel_metadata(&mut self, cx: &mut Context<Self>) {
        let metadata = self
            .sidebar_panel_metadata
            .get_or_insert_with(|| cx.new(|_| HashMap::new()))
            .clone();
        if self.panel_metadata_observers.len() == self.slots.len()
            && self.slots.iter().all(|slot| {
                self.panel_metadata_observers
                    .contains_key(&slot.panel.entity_id())
            })
        {
            return;
        }
        self.panel_metadata_observers
            .retain(|id, _| self.slots.iter().any(|slot| slot.panel.entity_id() == *id));
        let workspace_id = cx.entity_id();
        let mut added = Vec::new();
        for slot in &self.slots {
            let id = slot.panel.entity_id();
            if self.panel_metadata_observers.contains_key(&id) {
                continue;
            }
            added.push((id, SidebarPanelMetadata::from_item(&slot.panel, cx)));
            let metadata = metadata.downgrade();
            let refresh = move |item: PanelItem, cx: &mut App| {
                let Some(metadata) = metadata.upgrade() else {
                    return;
                };
                if metadata
                    .read(cx)
                    .get(&id)
                    .is_some_and(|old| old.matches(&item, cx))
                {
                    return;
                }
                let snapshot = SidebarPanelMetadata::from_item(&item, cx);
                metadata.update(cx, |metadata, _| {
                    metadata.insert(id, snapshot);
                });
                cx.notify(workspace_id);
            };
            let observer = match &slot.panel {
                PanelItem::Chat(panel) => App::observe(cx, panel, move |panel, cx| {
                    refresh(PanelItem::Chat(panel), cx);
                }),
                PanelItem::Terminal(terminal) => App::observe(cx, terminal, move |terminal, cx| {
                    refresh(PanelItem::Terminal(terminal), cx);
                }),
                PanelItem::File(file) => App::observe(cx, file, move |file, cx| {
                    refresh(PanelItem::File(file), cx);
                }),
                PanelItem::Changelog(changelog) => App::observe(cx, changelog, move |changelog, cx| {
                    refresh(PanelItem::Changelog(changelog), cx);
                }),
            };
            self.panel_metadata_observers.insert(id, observer);
        }
        metadata.update(cx, |metadata, _| {
            metadata.retain(|id, _| self.panel_metadata_observers.contains_key(id));
            metadata.extend(added);
        });
    }
}
