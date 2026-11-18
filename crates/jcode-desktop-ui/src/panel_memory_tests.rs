//! Memory parity with the TUI: recalled rows in the transcript and the
//! pipeline step in the composer status.
//!
//! Three contracts matter:
//! 1. a live `memory_injected` event adds one collapsed row;
//! 2. history carries the same row, so a reload shows it in place;
//! 3. a `memory` tool call reads as what it did, not as raw arguments.

use super::compaction::{CompactionRow, CompactionState};
use super::{ApiEvent, Item, Panel};

const MEMORY_BODY: &str =
    "auto-recalled 2 memories\n\n- [fact] the build uses mold\n- [pref] terse replies";

fn memory_rows(items: &[Item]) -> Vec<CompactionRow> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Memory(row) => Some(row.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn organization_tool_rows_are_restored_after_a_reload() {
    let message = jcode_sdk::HistoryMessage {
        timestamp_ms: None,
        response_stats: None,
        role: "memory".into(),
        content: MEMORY_BODY.into(),
        tool: None,
    };
    let (items, _) = Panel::history_items(vec![message], Vec::new());
    let rows = memory_rows(&items);
    assert_eq!(rows.len(), 1, "one stored injection, one row: {items:?}");
    assert_eq!(rows[0].label(), "auto-recalled 2 memories");
    assert_eq!(
        rows[0].detail(),
        Some("- [fact] the build uses mold\n- [pref] terse replies")
    );
}

#[test]
fn stored_memory_tool_calls_survive_a_reload() {
    let message = jcode_sdk::HistoryMessage {
        timestamp_ms: None,
        response_stats: None,
        role: "tool".into(),
        content: "Remembered.".into(),
        tool: Some(jcode_sdk::HistoryTool {
            id: "call-memory".into(),
            name: "memory".into(),
            input: serde_json::json!({"action": "remember", "content": "keep replies terse"}),
        }),
    };
    let (items, _) = Panel::history_items(vec![message], Vec::new());
    assert!(
        matches!(&items[..], [Item::Tool { name, done: true, .. }] if name == "memory"),
        "{items:?}"
    );
}

#[test]
fn memory_tool_summaries_name_the_action_like_the_tui() {
    assert_eq!(
        super::memory_tool_summary(r#"{"action":"remember","content":"keep replies terse"}"#),
        "remember: keep replies terse"
    );
    assert_eq!(
        super::memory_tool_summary(r#"{"action":"recall","query":"mold"}"#),
        "recall 'mold'"
    );
    assert_eq!(
        super::memory_tool_summary(r#"{"action":"recall"}"#),
        "recall (recent)"
    );
    assert_eq!(
        super::memory_tool_summary(r#"{"action":"forget","id":"mem_123"}"#),
        "forget mem_123"
    );
    assert_eq!(
        super::memory_tool_summary(r#"{"action":"list","scope":"global"}"#),
        "list memories"
    );
    // The TUI words apply to aliased provider names too.
    assert_eq!(
        super::tool_row_summary(
            "functions.memory",
            r#"{"action":"search","query":"socket"}"#
        ),
        "search 'socket'"
    );
}

#[gpui::test]
fn a_live_memory_event_adds_one_collapsible_row(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = memory_panel(cx);
    panel.update(vcx, |panel, cx| {
        panel.apply(&live_memory_event(), cx);
    });
    panel.read_with(vcx, |panel, _| {
        let rows = memory_rows(&panel.items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label(), "auto-recalled 2 memories");
        assert!(rows[0].has_detail());
    });
}

#[gpui::test]
fn clicking_a_memory_row_reveals_what_was_recalled(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = memory_panel(cx);
    panel.update(vcx, |panel, cx| {
        panel.apply(&live_memory_event(), cx);
    });
    vcx.run_until_parked();

    let row = vcx
        .debug_bounds("memory-divider")
        .expect("the memory row paints");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();

    panel.read_with(vcx, |panel, _| {
        let rows = memory_rows(&panel.items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, CompactionState::Expanded);
    });
}

#[gpui::test]
fn memory_activity_names_the_pipeline_step_until_output_starts(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = memory_panel(cx);
    panel.update(vcx, |panel, cx| {
        panel.status = "running".into();
        panel.apply(
            &ApiEvent::MemoryActivity {
                session_id: "memory-activity".into(),
                activity: jcode_sdk::api::MemoryActivitySnapshot {
                    state: jcode_sdk::api::MemoryState::FoundRelevant { count: 3 },
                    state_age_ms: 10,
                    pipeline: None,
                },
            },
            cx,
        );
    });
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.status_line()),
        "Found 3 memories"
    );

    panel.update(vcx, |panel, cx| {
        panel.status = "streaming".into();
        cx.notify();
    });
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.status_line()),
        "Responding",
        "model output takes over from the memory step"
    );

    panel.update(vcx, |panel, cx| {
        panel.status = "running".into();
        panel.apply(
            &ApiEvent::MemoryActivity {
                session_id: "memory-activity".into(),
                activity: jcode_sdk::api::MemoryActivitySnapshot {
                    state: jcode_sdk::api::MemoryState::Idle,
                    state_age_ms: 0,
                    pipeline: None,
                },
            },
            cx,
        );
    });
    assert_eq!(
        panel.read_with(vcx, |panel, _| panel.status_line()),
        "Working",
        "an idle snapshot stops claiming memory work"
    );
}

fn live_memory_event() -> ApiEvent {
    ApiEvent::MemoryInjected {
        session_id: "memory-activity".into(),
        count: 2,
        content: MEMORY_BODY.into(),
    }
}

fn memory_panel(
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<Panel>, &mut gpui::VisualTestContext) {
    cx.update(|cx| crate::bind_workspace_keys(cx));
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.push_test_panel("memory-activity", cx);
        workspace
    });
    vcx.run_until_parked();
    let panel = workspace
        .read_with(vcx, |workspace, _| workspace.test_panel(0))
        .expect("panel exists");
    (panel, vcx)
}
