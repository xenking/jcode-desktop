//! The compaction divider in the transcript.
//!
//! A compaction is a single full-width rule with a centered label, e.g.
//! `───── compacted · snapcompact · 180k→26k · 12s ─────`, and it expands in
//! place to show the summary and the elided-message detail.
//!
//! The row is stored, not transient: the body comes from the daemon (which also
//! writes it into session history), so a live divider and the one that comes back
//! after a reload are the same text. This module only owns presentation and the
//! collapse state.

/// Collapse state of a divider row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CompactionState {
    /// A compaction is running: `compacting…`, no detail, nothing to expand.
    Running,
    Collapsed,
    Expanded,
}

impl Default for CompactionState {
    fn default() -> Self {
        Self::Collapsed
    }
}

impl CompactionState {
    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn is_expanded(self) -> bool {
        matches!(self, Self::Expanded)
    }

    /// Flip between collapsed and expanded. A running divider cannot expand.
    pub fn toggled(self) -> Self {
        match self {
            Self::Running => Self::Running,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Collapsed,
        }
    }
}

/// One compaction divider in the transcript.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompactionRow {
    /// `label`, blank line, `detail`. A running divider has only the label.
    pub content: String,
    pub state: CompactionState,
}

impl CompactionRow {
    /// A completed compaction, stored or delivered by a live event.
    pub fn complete(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            state: CompactionState::Collapsed,
        }
    }

    /// The in-progress divider.
    pub fn running(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            state: CompactionState::Running,
        }
    }

    /// The label line: what a collapsed divider shows.
    pub fn label(&self) -> &str {
        self.content
            .split_once("\n\n")
            .map(|(label, _)| label)
            .unwrap_or(self.content.as_str())
            .trim()
    }

    /// The expandable body, when the row has one.
    pub fn detail(&self) -> Option<&str> {
        self.content
            .split_once("\n\n")
            .map(|(_, detail)| detail.trim_end())
            .filter(|detail| !detail.is_empty())
    }

    pub fn has_detail(&self) -> bool {
        self.detail().is_some()
    }

    pub fn toggle(&mut self) -> bool {
        let next = self.state.toggled();
        if next == self.state {
            return false;
        }
        self.state = next;
        true
    }
}

#[cfg(test)]
#[path = "panel_compaction_tests.rs"]
mod tests;
