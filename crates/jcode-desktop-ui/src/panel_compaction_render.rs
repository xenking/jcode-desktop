//! Rendering for transcript divider rows.
//!
//! Kept next to [`super::compaction`] rather than in `render_item` so the divider
//! can be reused by the preview, the snapshot and the live transcript without
//! three copies of the rule geometry. Compactions and automatic memory
//! injections share it: both are a collapsed rule line that expands in place.

use gpui::prelude::*;

use super::compaction::CompactionRow;
use super::{Context, FontWeight, Item, Theme, Window, div, px, text_selection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Divider {
    /// `─── compacted · snapcompact · 180k→26k ───`
    Compaction,
    /// `─── auto-recalled 3 memories ───`
    Memory,
}

impl Divider {
    fn selector(self) -> &'static str {
        match self {
            Self::Compaction => "compaction-divider",
            Self::Memory => "memory-divider",
        }
    }

    fn detail_key(self, index: usize) -> String {
        match self {
            Self::Compaction => format!("{index}-compaction-detail"),
            Self::Memory => format!("{index}-memory-detail"),
        }
    }
}

impl super::Panel {
    /// One compaction divider: `─── label ───`, expanded on click.
    pub(super) fn render_compaction_divider(
        &self,
        index: usize,
        row: &CompactionRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_transcript_divider(index, row, Divider::Compaction, window, cx)
    }

    /// One memory-injection row: the same rule, labelled with what was recalled.
    pub(super) fn render_memory_divider(
        &self,
        index: usize,
        row: &CompactionRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_transcript_divider(index, row, Divider::Memory, window, cx)
    }

    fn render_transcript_divider(
        &self,
        index: usize,
        row: &CompactionRow,
        kind: Divider,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let running = row.state.is_running();
        let expanded = row.state.is_expanded();
        let tone = if running { theme.WARN } else { theme.TEXT_DIM };
        let rule = div().flex_1().h(px(1.)).bg(theme.PANEL_BORDER);

        let selector = kind.selector();
        let label = row.label().to_string();
        let mut line = div()
            .relative()
            .children(crate::fixture_control_targets::marker(
                || selector.into(),
                || Some(self.session_id.clone()),
            ))
            .debug_selector(move || selector.into())
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .py_1()
            .child(rule)
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(tone)
                    .child(label),
            );
        if row.has_detail() {
            line = line.child(
                div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(theme.TEXT_DIM)
                    .child(if expanded { "▾" } else { "▸" }),
            );
        }
        line = line.child(div().flex_1().h(px(1.)).bg(theme.PANEL_BORDER));

        // Only a divider with a body is clickable, and `id`/`on_click` turn the
        // element stateful, so erase it before wrapping it in the column.
        let line = if row.has_detail() {
            let entity = cx.entity().downgrade();
            line.id((selector, index))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |panel, cx| {
                        panel.toggle_transcript_divider(index, kind);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            line.into_any_element()
        };

        let mut column = div().w_full().flex().flex_col().child(line);
        if expanded && let Some(detail) = row.detail() {
            // Rendered as selectable plain text so the recalled memories can be
            // copied out of the transcript instead of only read.
            column = column.child(div().w_full().px_2().pb_1().child(text_selection::plain(
                self.transcript_selection.clone(),
                kind.detail_key(index),
                detail,
                window,
                cx,
            )));
        }
        column.into_any_element()
    }

    /// Flip a divider between collapsed and expanded.
    pub(crate) fn toggle_compaction_divider(&mut self, index: usize) -> bool {
        self.toggle_transcript_divider(index, Divider::Compaction)
    }

    pub(crate) fn toggle_transcript_divider(&mut self, index: usize, kind: Divider) -> bool {
        // `items` derefs mutably, which bumps the transcript revision, so the
        // cached transcript documents are rebuilt after a toggle.
        let row = match kind {
            Divider::Compaction => {
                let Some(Item::Compaction(row)) = self.items.get_mut(index) else {
                    return false;
                };
                row
            }
            Divider::Memory => {
                let Some(Item::Memory(row)) = self.items.get_mut(index) else {
                    return false;
                };
                row
            }
        };
        row.toggle()
    }

    /// Insert or refresh the running divider.
    ///
    /// Only one compaction can run at a time, so a repeated start updates the
    /// existing row instead of stacking dividers.
    pub(crate) fn show_compaction_running(&mut self, content: impl Into<String>) {
        let content = content.into();
        let running = CompactionRow::running(content);
        if let Some(existing) = self.items.iter_mut().rev().find_map(|item| match item {
            Item::Compaction(row) if row.state.is_running() => Some(row),
            _ => None,
        }) {
            existing.content = running.content;
            return;
        }
        self.items.push(Item::Compaction(running));
    }

    /// Replace the running divider with the completed one.
    pub(crate) fn finish_compaction_divider(&mut self, content: impl Into<String>) {
        let completed = CompactionRow::complete(content);
        if let Some(existing) = self.items.iter_mut().rev().find_map(|item| match item {
            Item::Compaction(row) if row.state.is_running() => Some(row),
            _ => None,
        }) {
            *existing = completed;
            return;
        }
        // No running row (the compaction started before this client attached):
        // append the finished divider so it is not lost.
        self.items.push(Item::Compaction(completed));
    }
}
