//! Rendering for the compaction divider.
//!
//! Kept next to [`super::compaction`] rather than in `render_item` so the divider
//! can be reused by the preview, the snapshot and the live transcript without
//! three copies of the rule geometry.

use gpui::prelude::*;

use super::compaction::CompactionRow;
use super::{Context, FontWeight, Item, Theme, Window, div, px, text_selection};

impl super::Panel {
    /// One compaction divider: `─── label ───`, expanded on click.
    pub(super) fn render_compaction_divider(
        &self,
        index: usize,
        row: &CompactionRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::global();
        let running = row.state.is_running();
        let expanded = row.state.is_expanded();
        let tone = if running { theme.WARN } else { theme.TEXT_DIM };
        let rule = div().flex_1().h(px(1.)).bg(theme.PANEL_BORDER);

        let label = row.label().to_string();
        let mut line = div()
            .debug_selector(|| "compaction-divider".into())
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
            line.id(("compaction-divider", index))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |panel, cx| {
                        panel.toggle_compaction_divider(index);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            line.into_any_element()
        };

        let mut column = div().w_full().flex().flex_col().child(line);
        if expanded
            && let Some(detail) = row.detail()
        {
            // Rendered as selectable plain text so the summary can be copied out
            // of the transcript instead of only read.
            column = column.child(div().w_full().px_2().pb_1().child(text_selection::plain(
                self.transcript_selection.clone(),
                format!("{index}-compaction-detail"),
                detail,
                window,
                cx,
            )));
        }
        column.into_any_element()
    }

    /// Flip a divider between collapsed and expanded.
    pub(crate) fn toggle_compaction_divider(&mut self, index: usize) -> bool {
        // `items` derefs mutably, which bumps the transcript revision, so the
        // cached transcript documents are rebuilt after a toggle.
        let Some(Item::Compaction(row)) = self.items.get_mut(index) else {
            return false;
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
        if let Some(existing) = self
            .items
            .iter_mut()
            .rev()
            .find_map(|item| match item {
                Item::Compaction(row) if row.state.is_running() => Some(row),
                _ => None,
            })
        {
            existing.content = running.content;
            return;
        }
        self.items.push(Item::Compaction(running));
    }

    /// Replace the running divider with the completed one.
    pub(crate) fn finish_compaction_divider(&mut self, content: impl Into<String>) {
        let completed = CompactionRow::complete(content);
        if let Some(existing) = self
            .items
            .iter_mut()
            .rev()
            .find_map(|item| match item {
                Item::Compaction(row) if row.state.is_running() => Some(row),
                _ => None,
            })
        {
            *existing = completed;
            return;
        }
        // No running row (the compaction started before this client attached):
        // append the finished divider so it is not lost.
        self.items.push(Item::Compaction(completed));
    }
}
