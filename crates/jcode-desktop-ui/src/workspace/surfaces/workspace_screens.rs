//! The state of the full-screen surfaces that take over the keyboard: the
//! session picker and the launch error. Data only, so the focus logic that
//! consults them sits above them without knowing their behavior.
use super::*;
use crate::global_search::{self, PickRow};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(super) enum Filter {
    #[default]
    All,
    Active,
    Saved,
    /// Sessions whose process died mid-work, so they can be recovered.
    Crashed,
}

/// Legacy is only a decode fallback. Every new snapshot explicitly records
/// Closed or Open so --resume cannot reopen a deliberately dismissed picker.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) enum Snapshot {
    #[default]
    Legacy,
    Closed,
    Open {
        search: PromptInputSnapshot,
        query: String,
        selected: Option<String>,
        filter: Filter,
        start_on_close: bool,
    },
}

pub(super) struct State {
    pub(super) search: Entity<PromptInput>,
    pub(super) query: String,
    pub(super) selected: Option<String>,
    pub(super) filter: Filter,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    pub(super) preview_focus: FocusHandle,
    pub(super) start_on_close: bool,
    pub(super) preview_id: Option<String>,
    /// The real chat panel in transcript-only mode, hydrated through the same
    /// `load_history` path a resumed session uses, so the preview is identical.
    pub(super) preview: Option<Result<Entity<Panel>, String>>,
    pub(super) preview_task: Option<gpui::Task<()>>,
    /// Sessions the daemon is generating for right now, from its streaming
    /// markers. Refreshed off the UI thread while the picker is open.
    pub(super) streaming: HashSet<String>,
    pub(super) _presence_task: Option<gpui::Task<()>>,
    /// The selected message hit inside `selected`, if a hit row is selected.
    pub(super) selected_hit: Option<usize>,
    /// Conversation search through the daemon's full-text index.
    pub(super) daemon: global_search::Search,
    pub(super) _debounce: Option<gpui::Task<()>>,
    /// The row the preview was last scrolled to, so it scrolls once per
    /// selection instead of fighting the user's own scrolling.
    pub(super) preview_revealed: Option<PickRow>,
}

#[derive(Default)]
pub(super) struct LaunchState {
    #[cfg(not(test))]
    pub(super) pending: bool,
    pub(super) error: Option<String>,
    #[cfg(test)]
    pub(super) requests: Vec<PathBuf>,
}
