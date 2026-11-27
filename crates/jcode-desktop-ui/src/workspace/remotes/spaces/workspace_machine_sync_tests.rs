//! A machine's Desktop state in its own daemon: what this Desktop writes, what
//! it shows from another Desktop's changes, and how a machine that is lost or
//! too old leaves everything else working.
use super::*;
use jcode_desktop_harness::harness::{LinkStatus, MachineStateEvent};
use jcode_desktop_harness::paired;
use jcode_sdk::api::{DesktopStateChange, DesktopStateEntry};
use jcode_sdk::desktop_state::{DesktopStatePut, DesktopStateSnapshot};
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

const WORK: &str = "paired:0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";

fn machine() -> paired::Destination {
    paired::Destination {
        id: "0a".repeat(16),
        endpoint: "https://work.example".into(),
        label: "work".into(),
        working_dir: "/work".into(),
    }
}

fn entry(key: &str, rev: u64, value: Option<Value>) -> DesktopStateEntry {
    DesktopStateEntry {
        key: key.into(),
        rev,
        value,
    }
}

fn draft(text: &str) -> Value {
    json!({"v": 1, "text": text, "cursor": text.len()})
}

fn panel(session: &str, title: &str) -> Value {
    json!({"v": 1, "kind": "session", "session_id": session, "title": title})
}

fn snapshot(rev: u64, entries: Vec<DesktopStateEntry>) -> MachineStateEvent {
    MachineStateEvent::Snapshot(DesktopStateSnapshot {
        space: "desktop".into(),
        rev,
        entries,
    })
}

fn new_workspace(
    cx: &mut gpui::TestAppContext,
    with_machine: bool,
) -> (
    Entity<Workspace>,
    &mut gpui::VisualTestContext,
    Receiver<Command>,
) {
    let (bridge, commands) = harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(move |_, cx| {
        let mut w = Workspace::for_test(learning::Coach::new(), cx);
        w.bridge = bridge;
        if with_machine {
            w.remotes.paired = vec![machine()];
            paired::publish_labels(&w.remotes.paired);
            w.sync_spaces();
        }
        w
    });
    vcx.run_until_parked();
    (workspace, vcx, commands)
}

fn link(vcx: &mut gpui::VisualTestContext, w: &Entity<Workspace>, host: Option<&str>, status: LinkStatus) {
    w.update(vcx, |w, cx| {
        w.apply(
            Update::MachineLink {
                host: host.map(str::to_owned),
                status,
            },
            cx,
        );
    });
}

fn state(vcx: &mut gpui::VisualTestContext, w: &Entity<Workspace>, host: Option<&str>, event: MachineStateEvent) {
    w.update(vcx, |w, cx| {
        w.apply(
            Update::MachineState {
                host: host.map(str::to_owned),
                event,
            },
            cx,
        );
    });
}

fn changed(rev: u64, entries: Vec<DesktopStateEntry>) -> MachineStateEvent {
    MachineStateEvent::Changed {
        rev,
        device: "other".into(),
        entries,
    }
}

fn tick(vcx: &mut gpui::VisualTestContext, w: &Entity<Workspace>) {
    w.update_in(vcx, |w, window, cx| w.machine_sync_tick(window, cx));
}

/// Every write queued so far, as (expected revision, changes).
fn writes(commands: &Receiver<Command>) -> Vec<(u64, u64, Vec<DesktopStateChange>)> {
    commands
        .try_iter()
        .filter_map(|command| match command {
            Command::PutMachineState {
                token,
                expected_rev,
                changes,
                ..
            } => Some((token, expected_rev, changes)),
            _ => None,
        })
        .collect()
}

/// The daemon accepts a write: it answers with the entries that changed.
fn applied(
    vcx: &mut gpui::VisualTestContext,
    w: &Entity<Workspace>,
    host: Option<&str>,
    write: &(u64, u64, Vec<DesktopStateChange>),
    rev: u64,
) {
    let entries = write
        .2
        .iter()
        .map(|change| entry(&change.key, rev, change.value.clone()))
        .collect();
    w.update(vcx, |w, cx| {
        w.apply(
            Update::MachineStatePut {
                host: host.map(str::to_owned),
                token: write.0,
                result: Ok(DesktopStatePut::Applied { rev, entries }),
            },
            cx,
        );
    });
}

fn set_draft(vcx: &mut gpui::VisualTestContext, w: &Entity<Workspace>, index: usize, text: &str) {
    w.update(vcx, |w, cx| {
        let text = text.to_owned();
        w.slots[index]
            .panel
            .update_chat(cx, |panel, cx| panel.apply_shared_draft(text.clone(), text.len(), Vec::new(), cx));
    });
}

fn draft_text(vcx: &mut gpui::VisualTestContext, w: &Entity<Workspace>, index: usize) -> String {
    w.read_with(vcx, |w, cx| w.slots[index].panel.read_chat(cx).unwrap().shared_draft(cx).0)
}

fn put_value(changes: &[DesktopStateChange], key: &str) -> Option<Value> {
    changes.iter().find(|c| c.key == key)?.value.clone()
}

#[gpui::test]
fn a_draft_typed_here_is_written_to_the_machines_daemon(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, false);
    w.update(vcx, |w, cx| w.push_test_panel("session_fox_1_aa", cx));
    link(vcx, &w, None, LinkStatus::Connected);
    state(vcx, &w, None, snapshot(0, vec![]));
    // An empty machine is seeded with what is open here.
    let seed = writes(&commands);
    assert_eq!(seed.len(), 1);
    let (_, expected_rev, changes) = &seed[0];
    assert_eq!(*expected_rev, 0);
    assert!(put_value(changes, "layout").is_some());
    assert_eq!(
        put_value(changes, "panel/session_fox_1_aa").unwrap()["session_id"],
        "session_fox_1_aa"
    );
    applied(vcx, &w, None, &seed[0], 1);

    set_draft(vcx, &w, 0, "dear daemon");
    tick(vcx, &w);
    let typed = writes(&commands);
    assert_eq!(typed.len(), 1, "only the changed entry is written");
    assert_eq!(typed[0].1, 1, "on top of the revision the machine is at");
    assert_eq!(
        put_value(&typed[0].2, "draft/session_fox_1_aa").unwrap()["text"],
        "dear daemon"
    );
    assert!(put_value(&typed[0].2, "layout").is_none());

    // Nothing changed since: nothing is written, however often it is compared.
    applied(vcx, &w, None, &typed[0], 2);
    tick(vcx, &w);
    tick(vcx, &w);
    assert!(writes(&commands).is_empty());
}

#[gpui::test]
fn another_desktops_draft_appears_in_the_panel_without_echoing_back(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, false);
    w.update(vcx, |w, cx| w.push_test_panel("session_fox_1_aa", cx));
    link(vcx, &w, None, LinkStatus::Connected);
    state(vcx, &w, None, snapshot(0, vec![]));
    applied(vcx, &w, None, &writes(&commands)[0], 1);

    state(
        vcx,
        &w,
        None,
        changed(2, vec![entry("draft/session_fox_1_aa", 2, Some(draft("from the other desktop")))]),
    );
    assert_eq!(draft_text(vcx, &w, 0), "from the other desktop");
    tick(vcx, &w);
    tick(vcx, &w);
    assert!(writes(&commands).is_empty(), "showing it is not a change to write back");
    // The other Desktop sent the prompt: its draft is gone here too.
    state(vcx, &w, None, changed(3, vec![entry("draft/session_fox_1_aa", 3, None)]));
    assert_eq!(draft_text(vcx, &w, 0), "");
}

#[gpui::test]
fn the_composer_being_typed_in_keeps_its_draft_over_another_desktops(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, false);
    w.update(vcx, |w, cx| w.push_test_panel("session_fox_1_aa", cx));
    link(vcx, &w, None, LinkStatus::Connected);
    state(vcx, &w, None, snapshot(0, vec![]));
    applied(vcx, &w, None, &writes(&commands)[0], 1);
    set_draft(vcx, &w, 0, "mine, typing now");
    // The user is in this composer when the other Desktop's draft arrives.
    w.update(vcx, |w, _| {
        w.machine_sync.typing.insert("draft/session_fox_1_aa".into());
    });
    state(
        vcx,
        &w,
        None,
        changed(2, vec![entry("draft/session_fox_1_aa", 2, Some(draft("theirs")))]),
    );
    assert_eq!(draft_text(vcx, &w, 0), "mine, typing now");
    let rewrites = writes(&commands);
    assert_eq!(rewrites.len(), 1, "the focused draft is written over theirs");
    assert_eq!(rewrites[0].1, 2, "on top of their revision, so it wins");
    assert_eq!(
        put_value(&rewrites[0].2, "draft/session_fox_1_aa").unwrap()["text"],
        "mine, typing now"
    );
}

#[gpui::test]
fn a_fresh_desktop_restores_a_paired_machines_panels_layout_and_drafts_from_its_daemon(
    cx: &mut gpui::TestAppContext,
) {
    let (w, vcx, commands) = new_workspace(cx, true);
    link(vcx, &w, Some(WORK), LinkStatus::Connected);
    state(
        vcx,
        &w,
        Some(WORK),
        snapshot(
            5,
            vec![
                entry(
                    "layout",
                    5,
                    Some(json!({"v": 1, "rows": [
                        [{"panel": "session_work_1", "width": 0.5}, {"panel": "session_work_2", "width": 0.5}],
                        [],
                        [{"panel": "session_work_3", "width": 1.0}],
                    ]})),
                ),
                entry("panel/session_work_1", 1, Some(panel("session_work_1", "Deploy checks"))),
                entry("panel/session_work_2", 2, Some(panel("session_work_2", "Fix flaky CI"))),
                entry("panel/session_work_3", 3, Some(panel("session_work_3", "Triage alerts"))),
                entry("draft/session_work_2", 4, Some(draft("half a thought"))),
            ],
        ),
    );
    let rows: Vec<(String, usize)> = w.read_with(vcx, |w, cx| {
        w.slots
            .iter()
            .map(|slot| (slot.panel.session_id(cx).to_owned(), slot.row))
            .collect()
    });
    let remote = |name: &str| harness::remote_session_id(WORK, name);
    assert_eq!(
        rows,
        [
            (remote("session_work_1"), ROWS_PER_SPACE),
            (remote("session_work_2"), ROWS_PER_SPACE),
            (remote("session_work_3"), ROWS_PER_SPACE + 2),
        ],
        "each panel is back in its own workspace of that machine, in order"
    );
    assert_eq!(draft_text(vcx, &w, 1), "half a thought");
    assert_eq!(w.read_with(vcx, |w, _| w.slots[0].width_fraction), 0.5);
    assert!(writes(&commands).is_empty(), "restoring changes nothing on the daemon");
    // And none of it was written to this machine's own state.
    assert!(w.read_with(vcx, |w, _| w.machine_state_applied(None)).is_none());
}

#[gpui::test]
fn a_panel_another_desktop_closed_closes_here_and_a_moved_one_moves(cx: &mut gpui::TestAppContext) {
    let (w, vcx, _commands) = new_workspace(cx, true);
    link(vcx, &w, Some(WORK), LinkStatus::Connected);
    let layout = |rows: Value| entry("layout", 9, Some(json!({"v": 1, "rows": rows})));
    state(
        vcx,
        &w,
        Some(WORK),
        snapshot(
            4,
            vec![
                layout(json!([[{"panel": "session_a", "width": 0.5}, {"panel": "session_b", "width": 0.5}]])),
                entry("panel/session_a", 1, Some(panel("session_a", "A"))),
                entry("panel/session_b", 2, Some(panel("session_b", "B"))),
            ],
        ),
    );
    // The other Desktop moves A to workspace 3 and puts B first... and closes B.
    state(
        vcx,
        &w,
        Some(WORK),
        changed(
            6,
            vec![
                layout(json!([[], [], [{"panel": "session_a", "width": 1.0}]])),
                entry("panel/session_b", 6, None),
            ],
        ),
    );
    w.read_with(vcx, |w, cx| {
        let a = w.slots.iter().find(|s| s.panel.session_id(cx).ends_with("session_a")).unwrap();
        assert_eq!((a.row, a.width_fraction), (ROWS_PER_SPACE + 2, 1.0));
        assert_eq!(w.gone_sessions, [harness::remote_session_id(WORK, "session_b")]);
    });
}

#[gpui::test]
fn panels_reorder_to_the_order_another_desktop_chose(cx: &mut gpui::TestAppContext) {
    let (w, vcx, _commands) = new_workspace(cx, false);
    link(vcx, &w, None, LinkStatus::Connected);
    let layout = |order: [&str; 3]| {
        let slots: Vec<Value> = order.iter().map(|id| json!({"panel": id, "width": 0.3})).collect();
        entry("layout", 9, Some(json!({"v": 1, "rows": [slots]})))
    };
    let panels = ["session_a", "session_b", "session_c"]
        .map(|id| entry(&format!("panel/{id}"), 1, Some(panel(id, id))));
    let mut first = vec![layout(["session_a", "session_b", "session_c"])];
    first.extend(panels);
    state(vcx, &w, None, snapshot(3, first));
    state(vcx, &w, None, changed(4, vec![layout(["session_c", "session_a", "session_b"])]));
    let order: Vec<String> = w.read_with(vcx, |w, cx| {
        w.slots.iter().map(|slot| slot.panel.session_id(cx).to_owned()).collect()
    });
    assert_eq!(order, ["session_c", "session_a", "session_b"]);
}

#[gpui::test]
fn focus_comes_back_to_the_last_panel_only_inside_the_space_the_user_is_in(cx: &mut gpui::TestAppContext) {
    let (w, vcx, _commands) = new_workspace(cx, true);
    let entries = || {
        vec![
            entry(
                "layout",
                3,
                Some(json!({"v": 1, "rows": [[{"panel": "session_a", "width": 0.5}, {"panel": "session_b", "width": 0.5}]]})),
            ),
            entry("panel/session_a", 1, Some(panel("session_a", "A"))),
            entry("panel/session_b", 2, Some(panel("session_b", "B"))),
            entry("focus", 3, Some(json!({"v": 1, "row": 0, "panel": "session_b"}))),
        ]
    };
    // The user is on this machine's workspaces: the other machine's focus waits.
    link(vcx, &w, Some(WORK), LinkStatus::Connected);
    state(vcx, &w, Some(WORK), snapshot(3, entries()));
    assert_eq!(w.read_with(vcx, |w, _| w.active_row), 0);

    // In a space of its own, the user lands where they left off.
    let (w, vcx, _commands) = new_workspace(cx, true);
    w.update(vcx, |w, _| w.select_row(ROWS_PER_SPACE, 0));
    link(vcx, &w, Some(WORK), LinkStatus::Connected);
    state(vcx, &w, Some(WORK), snapshot(3, entries()));
    w.read_with(vcx, |w, cx| {
        assert_eq!(w.active_row, ROWS_PER_SPACE);
        assert!(w.slots[w.active].panel.session_id(cx).ends_with("session_b"));
    });
}

#[gpui::test]
fn what_was_typed_while_the_machine_was_lost_is_written_after_it_comes_back(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, false);
    w.update(vcx, |w, cx| {
        w.push_test_panel("session_a", cx);
        w.push_test_panel("session_b", cx);
    });
    link(vcx, &w, None, LinkStatus::Connected);
    state(vcx, &w, None, snapshot(0, vec![]));
    applied(vcx, &w, None, &writes(&commands)[0], 1);

    link(vcx, &w, None, LinkStatus::Lost);
    set_draft(vcx, &w, 0, "typed offline");
    tick(vcx, &w);
    assert!(writes(&commands).is_empty(), "nothing is sent while the machine is lost");

    // It returns having changed the other panel's draft meanwhile.
    link(vcx, &w, None, LinkStatus::Connected);
    state(
        vcx,
        &w,
        None,
        snapshot(
            2,
            vec![
                entry("panel/session_a", 1, Some(panel("session_a", "session_a"))),
                entry("panel/session_b", 1, Some(panel("session_b", "session_b"))),
                entry("layout", 1, Some(json!({"v": 1, "rows": [[{"panel": "session_a", "width": 0.5}, {"panel": "session_b", "width": 0.5}]]}))),
                entry("focus", 1, Some(json!({"v": 1, "row": 0, "panel": "session_b"}))),
                entry("draft/session_b", 2, Some(draft("typed elsewhere"))),
            ],
        ),
    );
    assert_eq!(draft_text(vcx, &w, 1), "typed elsewhere");
    assert_eq!(draft_text(vcx, &w, 0), "typed offline");
    let sent = writes(&commands);
    assert_eq!(sent.len(), 1);
    assert_eq!(put_value(&sent[0].2, "draft/session_a").unwrap()["text"], "typed offline");
    assert!(put_value(&sent[0].2, "draft/session_b").is_none());
}

#[gpui::test]
fn a_machine_that_needs_an_update_says_so_and_nothing_is_written_there(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, true);
    w.update(vcx, |w, cx| w.push_test_panel("session_a", cx));
    link(vcx, &w, Some(WORK), LinkStatus::NeedsUpdate);
    tick(vcx, &w);
    assert!(vcx.debug_bounds("space-status-1").is_some());
    assert_eq!(
        w.read_with(vcx, |w, _| w.remotes.machine_status.get(WORK).copied()),
        Some(machines::MachineStatus::NeedsUpdate)
    );
    assert!(writes(&commands).is_empty());
    // This machine's own panels carry on as before.
    assert_eq!(w.read_with(vcx, |w, _| w.slots.len()), 1);
}

#[gpui::test]
fn this_machine_with_an_old_jcode_says_so_even_with_nothing_paired(cx: &mut gpui::TestAppContext) {
    let (w, vcx, _commands) = new_workspace(cx, false);
    assert!(vcx.debug_bounds("space-section-0").is_none());
    link(vcx, &w, None, LinkStatus::NeedsUpdate);
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("space-section-0").is_some());
    assert!(vcx.debug_bounds("space-status-0").is_some());
    // Connected and lost say nothing here: only an update is the owner's to do.
    link(vcx, &w, None, LinkStatus::Connected);
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("space-section-0").is_none());
}

#[gpui::test]
fn machines_attach_only_when_needed(cx: &mut gpui::TestAppContext) {
    let (w, vcx, commands) = new_workspace(cx, true);
    let attached = |commands: &Receiver<Command>| -> Vec<Option<String>> {
        commands
            .try_iter()
            .filter_map(|command| match command {
                Command::AttachMachine { host } => Some(host),
                _ => None,
            })
            .collect()
    };
    tick(vcx, &w);
    assert_eq!(attached(&commands), [None], "this machine always; the paired one waits");
    tick(vcx, &w);
    assert!(attached(&commands).is_empty(), "each machine is asked for once");
    w.update(vcx, |w, _| w.select_row(ROWS_PER_SPACE, 0));
    tick(vcx, &w);
    assert_eq!(attached(&commands), [Some(WORK.to_owned())], "in its space, it attaches");
}
