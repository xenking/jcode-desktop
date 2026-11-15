//! Tests for the compaction divider.
//!
//! Three contracts matter:
//! 1. the row survives a reload, because it is stored history, not a notice;
//! 2. a collapsed divider is one rule line and a click reveals the summary;
//! 3. the running divider is replaced by the completed one, never stacked.

use super::super::{Item, Panel};
use super::{CompactionRow, CompactionState};

const DIVIDER_BODY: &str = "compacted · snapcompact · 180k→26k · 12s\n\n\
trigger: reactive\n\
messages summarized: 84\n\
The user asked for a compaction divider.";

fn compaction_rows(items: &[Item]) -> Vec<CompactionRow> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Compaction(row) => Some(row.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_divider_splits_into_label_and_detail() {
    let row = CompactionRow::complete(DIVIDER_BODY);
    assert_eq!(row.label(), "compacted · snapcompact · 180k→26k · 12s");
    assert_eq!(row.state, CompactionState::Collapsed);
    let detail = row.detail().expect("detail");
    assert!(detail.starts_with("trigger: reactive"));
    assert!(detail.contains("The user asked for a compaction divider."));
}

#[test]
fn toggling_a_divider_flips_only_collapsed_and_expanded() {
    let mut row = CompactionRow::complete(DIVIDER_BODY);
    assert!(row.toggle());
    assert_eq!(row.state, CompactionState::Expanded);
    assert!(row.toggle());
    assert_eq!(row.state, CompactionState::Collapsed);

    let mut running = CompactionRow::running("compacting… · reactive");
    assert!(!running.toggle(), "a running divider has nothing to expand");
    assert_eq!(running.state, CompactionState::Running);
}

#[test]
fn idle_compaction_divider_says_it_compacted_while_waiting() {
    let row = CompactionRow::complete(
        "compacted while waiting · summary · 476k→58k · 48s\n\ntrigger: idle_precompact",
    );
    assert_eq!(
        row.label(),
        "compacted while waiting · summary · 476k→58k · 48s"
    );
}

/// The persistence contract: a stored session renders a divider row, so
/// reopening the session shows the compaction instead of a blank transcript.
#[test]
fn a_stored_compaction_renders_a_divider_after_a_reload() {
    let mut session = jcode_base::session::Session::create_with_id(
        "session_desktop_compaction_divider".to_string(),
        None,
        None,
    );
    session.record_compaction_event(
        &jcode_base::compaction::CompactionEvent {
            trigger: "reactive".to_string(),
            method: Some("snapcompact".to_string()),
            pre_tokens: Some(180_000),
            post_tokens: Some(26_000),
            tokens_saved: Some(154_000),
            duration_ms: Some(12_300),
            messages_dropped: None,
            messages_compacted: Some(84),
            summary_chars: Some(1_200),
            active_messages: Some(12),
        },
        Some("snapcompact"),
        Some("The user asked for a compaction divider."),
    );

    // Render → wire shape → items, which is exactly the path a resumed panel and
    // the provisional on-disk preview both take.
    let (messages, images) = crate::persisted_history::render(&session);
    let (items, _) = Panel::history_items(messages, images);
    let rows = compaction_rows(&items);
    assert_eq!(rows.len(), 1, "one stored compaction, one divider");
    assert_eq!(rows[0].label(), "compacted · snapcompact · 180k→26k · 12s");
    assert!(rows[0].has_detail());
    assert_eq!(rows[0].state, CompactionState::Collapsed);
}

#[gpui::test]
fn a_click_expands_the_divider_to_the_summary(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| crate::bind_workspace_keys(cx));
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    panel.update(vcx, |panel, cx| {
        panel
            .items
            .push(Item::Compaction(CompactionRow::complete(DIVIDER_BODY)));
        cx.notify();
    });
    vcx.run_until_parked();

    let divider = vcx
        .debug_bounds("compaction-divider")
        .expect("the divider paints");
    assert!(
        panel.read_with(vcx, |panel, _| {
            matches!(
                panel.items.iter().find(|item| matches!(item, Item::Compaction(_))),
                Some(Item::Compaction(row)) if row.state == CompactionState::Collapsed
            )
        }),
        "the divider starts collapsed"
    );

    vcx.simulate_click(divider.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    assert!(
        panel.read_with(vcx, |panel, _| {
            matches!(
                panel.items.iter().find(|item| matches!(item, Item::Compaction(_))),
                Some(Item::Compaction(row)) if row.state == CompactionState::Expanded
            )
        }),
        "clicking the divider expands it"
    );
}

#[gpui::test]
fn a_running_divider_is_replaced_by_the_completed_one(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| crate::bind_workspace_keys(cx));
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");

    panel.update(vcx, |panel, cx| {
        panel.show_compaction_running("compacting… · reactive · 476000 tokens");
        // A second start (another trigger) updates the same row.
        panel.show_compaction_running("compacting… · idle_precompact · 476000 tokens");
        panel.finish_compaction_divider(DIVIDER_BODY);
        cx.notify();
    });
    vcx.run_until_parked();

    let rows = panel.read_with(vcx, |panel, _| compaction_rows(&panel.items));
    assert_eq!(rows.len(), 1, "one compaction, one divider: {rows:?}");
    assert_eq!(rows[0].state, CompactionState::Collapsed);
    assert_eq!(rows[0].label(), "compacted · snapcompact · 180k→26k · 12s");
}
