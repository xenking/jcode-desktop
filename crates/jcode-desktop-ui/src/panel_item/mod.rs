//! One workspace panel, whatever its kind. Workspace chrome (layout, focus,
//! reload snapshots, sidebar metadata) goes through these methods. Code that
//! only makes sense for a conversation asks for [`PanelItem::chat`].
pub(crate) mod changelog_panel;
pub(crate) mod file_panel;

use crate::input::PromptInputSnapshot;
use crate::panel::{Panel, PanelSnapshot};
use crate::terminal::TerminalPanel;
use changelog_panel::ChangelogPanel;
use file_panel::FilePanel;
use gpui::{AnyView, App, Entity, EntityId, FocusHandle, Focusable, Window};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PanelItem {
    Chat(Entity<Panel>),
    Terminal(Entity<TerminalPanel>),
    /// A source or Markdown file, editable and previewable. No session.
    File(Entity<FilePanel>),
    /// The local update notes. No session, and never saved.
    Changelog(Entity<ChangelogPanel>),
}

impl PanelItem {
    /// Saved id of a terminal slot. Terminals have no runtime session.
    pub(crate) const TERMINAL_SESSION_ID: &str = "terminal";

    pub(crate) fn chat(&self) -> Option<&Entity<Panel>> {
        match self {
            Self::Chat(panel) => Some(panel),
            Self::Terminal(_) | Self::File(_) | Self::Changelog(_) => None,
        }
    }

    pub(crate) fn read_chat<'a>(&self, cx: &'a App) -> Option<&'a Panel> {
        self.chat().map(|panel| panel.read(cx))
    }

    /// Update the conversation state. Other kinds have none, so they skip `update`.
    pub(crate) fn update_chat<R, C: gpui::AppContext>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut Panel, &mut gpui::Context<Panel>) -> R,
    ) -> Option<R> {
        self.chat().map(|panel| panel.update(cx, update))
    }

    pub(crate) fn entity_id(&self) -> EntityId {
        match self {
            Self::Chat(panel) => panel.entity_id(),
            Self::Terminal(terminal) => terminal.entity_id(),
            Self::File(file) => file.entity_id(),
            Self::Changelog(changelog) => changelog.entity_id(),
        }
    }

    pub(crate) fn session_id<'a>(&self, cx: &'a App) -> &'a str {
        match self {
            Self::Chat(panel) => &panel.read(cx).session_id,
            Self::Terminal(_) => Self::TERMINAL_SESSION_ID,
            Self::File(file) => file.read(cx).session_id(),
            Self::Changelog(_) => ChangelogPanel::SESSION_ID,
        }
    }

    /// Tests address most slots as conversations.
    #[cfg(test)]
    #[track_caller]
    fn expect_chat(&self) -> &Entity<Panel> {
        self.chat().expect("slot holds a chat panel")
    }

    #[cfg(test)]
    #[track_caller]
    pub(crate) fn read<'a>(&self, cx: &'a App) -> &'a Panel {
        self.expect_chat().read(cx)
    }

    #[cfg(test)]
    #[track_caller]
    pub(crate) fn read_with<R, C: gpui::AppContext>(
        &self,
        cx: &C,
        read: impl FnOnce(&Panel, &App) -> R,
    ) -> R {
        self.expect_chat().read_with(cx, read)
    }

    #[cfg(test)]
    #[track_caller]
    pub(crate) fn update<R, C: gpui::AppContext>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut Panel, &mut gpui::Context<Panel>) -> R,
    ) -> R {
        self.expect_chat().update(cx, update)
    }

    /// Focus, surface state and the view to mount.
    pub(crate) fn surface(&self) -> Surface<'_> {
        Surface(self)
    }

    /// What the item is: its kind, title, directory and the snapshot a reload keeps.
    pub(crate) fn profile(&self) -> Profile<'_> {
        Profile(self)
    }
}

/// Focus and surface state of one item.
pub(crate) struct Surface<'a>(&'a PanelItem);

impl Surface<'_> {
    /// Where keys typed into this panel go.
    pub(crate) fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        match self.0 {
            PanelItem::Chat(panel) => panel.read(cx).input_focus_handle(cx),
            PanelItem::Terminal(terminal) => terminal.read(cx).focus_handle(cx),
            PanelItem::File(file) => file.read(cx).input_focus_handle(cx),
            PanelItem::Changelog(changelog) => changelog.read(cx).focus_handle(cx),
        }
    }

    pub(crate) fn focus_input(&self, window: &mut Window, cx: &mut App) {
        let handle = self.input_focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// Whether `set_surface_focused(focused)` would change anything. Callers
    /// drawing a frame check this first, since GPUI counts any update made
    /// while drawing as a change to the views reading it.
    pub(crate) fn needs_update(&self, focused: bool, cx: &App) -> bool {
        match self.0 {
            PanelItem::Chat(panel) => panel.read(cx).needs_surface_update(focused),
            PanelItem::Terminal(terminal) => terminal.read(cx).surface_focused() != focused,
            PanelItem::File(file) => file.read(cx).surface_focused() != focused,
            PanelItem::Changelog(changelog) => changelog.read(cx).surface_focused() != focused,
        }
    }

    pub(crate) fn set_focused(&self, focused: bool, cx: &mut App) {
        match self.0 {
            PanelItem::Chat(panel) => {
                panel.update(cx, |panel, cx| panel.set_surface_focused(focused, cx));
            }
            PanelItem::Terminal(terminal) => {
                terminal.update(cx, |terminal, cx| terminal.set_surface_focused(focused, cx));
            }
            PanelItem::File(file) => {
                file.update(cx, |file, cx| file.set_surface_focused(focused, cx));
            }
            PanelItem::Changelog(changelog) => {
                changelog.update(cx, |changelog, cx| changelog.set_surface_focused(focused, cx));
            }
        }
    }

    /// Whether a vertical scroll over this panel belongs to it rather than
    /// moving between strips.
    pub(crate) fn scrollable(&self, cx: &App) -> bool {
        match self.0 {
            PanelItem::Chat(panel) => panel.read(cx).has_scrollable_conversation(),
            PanelItem::Terminal(_) | PanelItem::File(_) | PanelItem::Changelog(_) => true,
        }
    }

    /// Whether closing may proceed. A file with unsaved edits asks first.
    pub(crate) fn confirm_close(&self, cx: &mut App) -> bool {
        match self.0 {
            PanelItem::Chat(_) | PanelItem::Terminal(_) | PanelItem::Changelog(_) => true,
            PanelItem::File(file) => file.update(cx, |file, cx| file.confirm_close(cx)),
        }
    }

    pub(crate) fn view(&self) -> AnyView {
        match self.0 {
            PanelItem::Chat(panel) => panel.clone().into(),
            PanelItem::Terminal(terminal) => terminal.clone().into(),
            PanelItem::File(file) => file.clone().into(),
            PanelItem::Changelog(changelog) => changelog.clone().into(),
        }
    }
}

/// What one item is.
pub(crate) struct Profile<'a>(&'a PanelItem);

impl Profile<'_> {
    pub(crate) fn is_changelog(&self) -> bool {
        matches!(self.0, PanelItem::Changelog(_))
    }

    pub(crate) fn file(&self) -> Option<&Entity<FilePanel>> {
        match self.0 {
            PanelItem::File(file) => Some(file),
            PanelItem::Chat(_) | PanelItem::Terminal(_) | PanelItem::Changelog(_) => None,
        }
    }

    pub(crate) fn terminal(&self) -> Option<&Entity<TerminalPanel>> {
        match self.0 {
            PanelItem::Chat(_) | PanelItem::File(_) | PanelItem::Changelog(_) => None,
            PanelItem::Terminal(terminal) => Some(terminal),
        }
    }

    pub(crate) fn title(&self, cx: &App) -> gpui::SharedString {
        match self.0 {
            PanelItem::Chat(panel) => panel.read(cx).title.clone(),
            PanelItem::Terminal(_) => "terminal".into(),
            PanelItem::File(file) => file.read(cx).title().clone(),
            PanelItem::Changelog(changelog) => changelog.read(cx).title(),
        }
    }

    pub(crate) fn working_dir<'a>(&self, cx: &'a App) -> Option<&'a str> {
        match self.0 {
            PanelItem::Chat(panel) => panel.read(cx).working_dir.as_deref(),
            PanelItem::Terminal(terminal) => terminal.read(cx).working_dir(),
            PanelItem::File(file) => file.read(cx).working_dir(),
            PanelItem::Changelog(_) => None,
        }
    }

    /// Opt-in workspace diagnostics for real-PTY terminal acceptance checks.
    pub(crate) fn terminal_debug(&self, cx: &App) -> Option<serde_json::Value> {
        match self.0 {
            PanelItem::Chat(_) | PanelItem::File(_) | PanelItem::Changelog(_) => None,
            PanelItem::Terminal(terminal) => Some(terminal.read(cx).debug_snapshot()),
        }
    }

    pub(crate) fn snapshot(&self, for_reload: bool, cx: &App) -> PanelSnapshot {
        match self.0 {
            PanelItem::Chat(panel) if for_reload => panel.read(cx).snapshot_for_reload(cx),
            PanelItem::Chat(panel) => panel.read(cx).snapshot(cx),
            PanelItem::Terminal(terminal) => {
                let terminal = terminal.read(cx);
                PanelSnapshot {
                    terminal_resource_id: terminal.resource_id(),
                    terminal_output_cursor: Some(terminal.output_cursor()),
                    ..PanelSnapshot::bare(
                        PanelItem::TERMINAL_SESSION_ID,
                        "terminal",
                        terminal.working_dir(),
                    )
                }
            }
            PanelItem::File(file) => file.read(cx).snapshot(),
            // Notes are offered again after a reload, so the workspace never saves them.
            PanelItem::Changelog(_) => {
                PanelSnapshot::bare(ChangelogPanel::SESSION_ID, ChangelogPanel::TITLE, None)
            }
        }
    }
}

impl PanelSnapshot {
    /// The saved form of a kind with no conversation: an identity and nothing else.
    pub(crate) fn bare(session_id: &str, title: &str, working_dir: Option<&str>) -> Self {
        PanelSnapshot {
            draft_config: Default::default(),
            clean_chat_override: None,
            transcript: None,
            turn_outcome: None,
            finished_unseen: false,
            image_pane_open: false,
            side_document: None,
            prompt_queue: Default::default(),
            session_id: session_id.into(),
            title: title.into(),
            working_dir: working_dir.map(str::to_owned),
            draft: PromptInputSnapshot {
                content: String::new(),
                selection_start: 0,
                selection_end: 0,
                selection_reversed: false,
                history: Vec::new(),
                history_index: None,
                live_draft: String::new(),
                history_draft: None,
                stashed_prompt: None,
                attachments: Vec::new(),
            },
            ask_draft: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            stick_to_bottom: true,
            terminal_resource_id: None,
            terminal_output_cursor: None,
            startup_layout: None,
        }
    }
}
