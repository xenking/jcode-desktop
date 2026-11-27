//! A machine's Desktop state as plain data: what this Desktop writes to the
//! machine's daemon, how another Desktop's writes are read back, and how the
//! two meet.
//!
//! The daemon keeps a keyed document per machine (`layout`, `focus`,
//! `panel/<id>`, `draft/<id>`; see its `docs/DESKTOP_STATE.md`) and refuses any
//! field outside that list, so everything written here is built through the
//! daemon's own types. What other Desktops wrote is untrusted: it is read
//! leniently, an entry that does not parse is ignored, and no id or number in
//! it is used before it is checked.

use jcode_session_types::desktop_state::{
    DESKTOP_STATE_SCHEMA, DraftAttachment, DraftDoc, FocusDoc, LayoutDoc, LayoutSlot, PanelDoc,
    PanelKind,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub use jcode_session_types::desktop_state::{DESKTOP_STATE_CAPABILITY, PanelKind as Kind};

/// Entries by key, as the daemon stores them.
pub type Entries = BTreeMap<String, Value>;

pub const FOCUS: &str = "focus";
pub const LAYOUT: &str = "layout";
const PANEL: &str = "panel/";
const DRAFT: &str = "draft/";

/// Panels are laid out in at most this many rows per machine.
pub const MAX_ROWS: usize = 4;

/// One panel of a machine's space as this Desktop shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelFacts {
    /// Names the panel in the shared state: stable while the panel lives.
    pub id: String,
    pub kind: PanelKind,
    /// The daemon's own id of the session, for a session panel.
    pub session_id: Option<String>,
    pub title: Option<String>,
    /// Row within the machine's space, from 0.
    pub row: usize,
    pub width: f32,
    pub draft: Option<DraftFacts>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftFacts {
    pub text: String,
    /// Byte offset of the caret.
    pub cursor: usize,
    pub attachments: Vec<DraftAttachment>,
}

/// This Desktop's view of one machine's space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    /// In strip order: by row, then left to right.
    pub panels: Vec<PanelFacts>,
    /// Where the user is, when the user is in this space.
    pub focus: Option<(usize, Option<String>)>,
}

/// A name a panel may carry in the shared state.
pub fn valid_id(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn doc<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("state documents serialize")
}

/// What this Desktop would have the daemon hold for `facts`. `keep_focus` is
/// the focus the daemon already holds, left alone when the user is elsewhere.
pub fn entries(facts: &Facts, keep_focus: Option<&Value>) -> Entries {
    let mut out = Entries::new();
    let mut rows: Vec<Vec<LayoutSlot>> = Vec::new();
    for panel in &facts.panels {
        if panel.row >= MAX_ROWS || !valid_id(&panel.id, 64) {
            continue;
        }
        if rows.len() <= panel.row {
            rows.resize_with(panel.row + 1, Vec::new);
        }
        // Three decimals are all a width carries; fewer digits also keep the
        // number equal to itself after the daemon's round trip.
        rows[panel.row].push(LayoutSlot {
            panel: panel.id.clone(),
            width: (panel.width.clamp(0.05, 8.0) * 1000.0).round() / 1000.0,
        });
        out.insert(
            format!("{PANEL}{}", panel.id),
            doc(&PanelDoc {
                v: DESKTOP_STATE_SCHEMA,
                kind: panel.kind,
                session_id: panel.session_id.clone(),
                title: panel.title.clone().filter(|title| !title.is_empty()),
            }),
        );
        if let Some(draft) = &panel.draft
            && (!draft.text.is_empty() || !draft.attachments.is_empty())
        {
            out.insert(
                format!("{DRAFT}{}", panel.id),
                doc(&DraftDoc {
                    v: DESKTOP_STATE_SCHEMA,
                    text: draft.text.clone(),
                    cursor: draft.cursor.min(draft.text.len()) as u32,
                    model: None,
                    reasoning_effort: None,
                    attachments: draft.attachments.clone(),
                }),
            );
        }
    }
    if !rows.is_empty() {
        out.insert(
            LAYOUT.into(),
            doc(&LayoutDoc {
                v: DESKTOP_STATE_SCHEMA,
                rows,
            }),
        );
    }
    match &facts.focus {
        Some((row, panel)) => {
            out.insert(
                FOCUS.into(),
                doc(&FocusDoc {
                    v: DESKTOP_STATE_SCHEMA,
                    row: *row as u32,
                    panel: panel.clone().filter(|panel| valid_id(panel, 64)),
                }),
            );
        }
        None => {
            if let Some(held) = keep_focus {
                out.insert(FOCUS.into(), held.clone());
            }
        }
    }
    out
}

/// What the daemon holds, read back. Entries that do not parse are dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Remote {
    /// Panel ids and widths by row; rows past [`MAX_ROWS`] are ignored.
    pub rows: Vec<Vec<(String, f32)>>,
    pub panels: BTreeMap<String, PanelDoc>,
    pub drafts: BTreeMap<String, DraftDoc>,
    pub focus: Option<(usize, Option<String>)>,
}

fn typed<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

/// Read `entries` the way another Desktop wrote them.
pub fn parse(entries: &Entries) -> Remote {
    let mut remote = Remote::default();
    for (key, value) in entries {
        if key == LAYOUT {
            if let Some(layout) = typed::<LayoutDoc>(value).filter(|l| l.v == DESKTOP_STATE_SCHEMA)
            {
                remote.rows = layout
                    .rows
                    .into_iter()
                    .take(MAX_ROWS)
                    .map(|row| {
                        row.into_iter()
                            .filter(|slot| valid_id(&slot.panel, 64))
                            .map(|slot| (slot.panel, slot.width.clamp(0.05, 8.0)))
                            .collect()
                    })
                    .collect();
            }
        } else if key == FOCUS {
            if let Some(focus) = typed::<FocusDoc>(value).filter(|f| f.v == DESKTOP_STATE_SCHEMA) {
                remote.focus = Some((
                    (focus.row as usize).min(MAX_ROWS - 1),
                    focus.panel.filter(|panel| valid_id(panel, 64)),
                ));
            }
        } else if let Some(id) = key.strip_prefix(PANEL).filter(|id| valid_id(id, 64)) {
            if let Some(panel) = typed::<PanelDoc>(value).filter(|p| p.v == DESKTOP_STATE_SCHEMA)
                && panel
                    .session_id
                    .as_deref()
                    .is_none_or(|session| valid_id(session, 128))
            {
                remote.panels.insert(id.to_owned(), panel);
            }
        } else if let Some(id) = key.strip_prefix(DRAFT).filter(|id| valid_id(id, 64))
            && let Some(draft) = typed::<DraftDoc>(value).filter(|d| d.v == DESKTOP_STATE_SCHEMA)
        {
            remote.drafts.insert(id.to_owned(), draft);
        }
    }
    remote
}

/// The same entries with every number as the daemon would hand it back, so
/// equal documents compare equal. Entries that do not parse are dropped.
pub fn canonical(entries: &Entries) -> Entries {
    entries
        .iter()
        .filter_map(|(key, value)| {
            let canonical = if key == LAYOUT {
                doc(&typed::<LayoutDoc>(value)?)
            } else if key == FOCUS {
                doc(&typed::<FocusDoc>(value)?)
            } else if key.starts_with(PANEL) {
                doc(&typed::<PanelDoc>(value)?)
            } else if key.starts_with(DRAFT) {
                doc(&typed::<DraftDoc>(value)?)
            } else {
                return None;
            };
            Some((key.clone(), canonical))
        })
        .collect()
}

/// What to do about a difference between this Desktop and the daemon.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Keys to write; `None` deletes the key.
    pub push: Vec<(String, Option<Value>)>,
    /// Keys whose daemon value (or absence) this Desktop must now show.
    pub adopt: BTreeSet<String>,
}

/// Three-way merge of this Desktop (`local`), the daemon (`remote`) and what
/// the two last agreed on (`applied`).
///
/// - Changed only here: write it. Changed only there: show it.
/// - Changed on both and different: the daemon wins, except for a draft in a
///   `protected` composer (the user is typing there) and for `focus`, which is
///   every Desktop's own. Those are written over. A protected composer is also
///   left alone when only the daemon changed it.
/// - `first`: this Desktop has not yet seen the daemon's state. A daemon that
///   holds state is the truth (this Desktop's cache is older); an empty one is
///   seeded from this Desktop.
pub fn reconcile(
    applied: &Entries,
    remote: &Entries,
    local: &Entries,
    protected: &BTreeSet<String>,
    first: bool,
) -> Plan {
    let mut plan = Plan::default();
    let keys: BTreeSet<&String> = applied
        .keys()
        .chain(remote.keys())
        .chain(local.keys())
        .collect();
    let seeded = first && remote.is_empty();
    let truth = first && !remote.is_empty();
    for key in keys {
        let (a, r, l) = (applied.get(key), remote.get(key), local.get(key));
        if truth {
            // Focus is the user's own: a daemon without one never clears it.
            if r != l && !(key == FOCUS && r.is_none()) {
                plan.adopt.insert(key.clone());
            }
            continue;
        }
        let (local_changed, remote_changed) = if seeded {
            (l.is_some(), false)
        } else {
            (l != a, r != a)
        };
        match (local_changed, remote_changed) {
            (false, false) => {}
            (true, false) => plan.push.push((key.clone(), l.cloned())),
            // The composer being typed in keeps its text even when the user has
            // not changed it since the last write. Nothing is written back, so
            // two Desktops in one composer do not overwrite each other forever;
            // the daemon's text shows once the user leaves the composer.
            (false, true) if protected.contains(key) => {}
            (false, true) => {
                plan.adopt.insert(key.clone());
            }
            (true, true) if l == r => {}
            (true, true) => {
                if key == FOCUS || protected.contains(key) {
                    plan.push.push((key.clone(), l.cloned()));
                } else {
                    plan.adopt.insert(key.clone());
                }
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_session_types::desktop_state::validate_entry;
    use serde_json::json;

    fn session(id: &str, row: usize, text: &str) -> PanelFacts {
        PanelFacts {
            id: id.into(),
            kind: PanelKind::Session,
            session_id: Some(id.into()),
            title: Some(format!("title {id}")),
            row,
            width: 0.5,
            draft: Some(DraftFacts {
                text: text.into(),
                cursor: text.len(),
                attachments: if text.is_empty() {
                    Vec::new()
                } else {
                    vec![DraftAttachment {
                        media_type: "image/png".into(),
                        label: Some("shot".into()),
                        data: "aGk=".into(),
                    }]
                },
            }),
        }
    }

    #[test]
    fn everything_written_passes_the_daemons_own_allowlist() {
        let facts = Facts {
            panels: vec![
                session("session_a_1", 0, "hello"),
                session("session_b_2", 2, ""),
                PanelFacts {
                    id: "c-9".into(),
                    kind: PanelKind::Compose,
                    session_id: None,
                    title: None,
                    row: 2,
                    width: 1.0,
                    draft: None,
                },
            ],
            focus: Some((2, Some("session_b_2".into()))),
        };
        let written = entries(&facts, None);
        assert_eq!(
            written.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "draft/session_a_1",
                "focus",
                "layout",
                "panel/c-9",
                "panel/session_a_1",
                "panel/session_b_2"
            ]
        );
        for (key, value) in &written {
            validate_entry(key, value).unwrap_or_else(|v| panic!("{key}: {}", v.message));
        }
        // Rows with no panel stay empty rows, so a panel keeps its row.
        assert_eq!(written["layout"]["rows"][1], json!([]));
    }

    #[test]
    fn what_is_written_reads_back_the_same_for_another_desktop() {
        let facts = Facts {
            panels: vec![
                session("session_a_1", 1, "héllo"),
                session("session_b_2", 1, ""),
            ],
            focus: Some((1, Some("session_a_1".into()))),
        };
        let remote = parse(&canonical(&entries(&facts, None)));
        assert_eq!(
            remote.rows[1],
            [
                ("session_a_1".to_owned(), 0.5),
                ("session_b_2".to_owned(), 0.5)
            ]
        );
        assert_eq!(remote.drafts["session_a_1"].text, "héllo");
        assert_eq!(remote.drafts["session_a_1"].attachments[0].data, "aGk=");
        assert!(
            !remote.drafts.contains_key("session_b_2"),
            "an empty draft is no entry"
        );
        assert_eq!(remote.focus, Some((1, Some("session_a_1".into()))));
        assert_eq!(
            remote.panels["session_b_2"].session_id.as_deref(),
            Some("session_b_2")
        );
    }

    #[test]
    fn a_focus_held_by_the_daemon_is_kept_while_the_user_is_in_another_space() {
        let held = json!({"v": 1, "row": 3});
        let kept = entries(&Facts::default(), Some(&held));
        assert_eq!(kept[FOCUS], held);
        assert!(entries(&Facts::default(), None).is_empty());
    }

    #[test]
    fn another_desktops_entries_are_read_defensively() {
        let mut hostile = Entries::new();
        hostile.insert("layout".into(), json!({"v": 1, "rows": [[{"panel": "../etc/passwd", "width": 0.5}, {"panel": "ok", "width": 99.0}], [], [], [], [{"panel": "fifth", "width": 1.0}]]}));
        hostile.insert(
            "panel/ok".into(),
            json!({"v": 1, "kind": "session", "session_id": "paired://x/y"}),
        );
        hostile.insert(
            "panel/fine".into(),
            json!({"v": 1, "kind": "session", "session_id": "session_1"}),
        );
        hostile.insert("panel/bad id".into(), json!({"v": 1, "kind": "compose"}));
        hostile.insert("draft/ok".into(), json!({"v": 2, "text": "x"}));
        hostile.insert("focus".into(), json!({"v": 1, "row": 99, "panel": "a b"}));
        let remote = parse(&hostile);
        assert_eq!(
            remote.rows.len(),
            MAX_ROWS,
            "rows past the machine's four are ignored"
        );
        assert_eq!(
            remote.rows[0],
            [("ok".to_owned(), 8.0)],
            "a path is no panel and a width is bounded"
        );
        assert!(
            !remote.panels.contains_key("ok"),
            "a routed session id is refused"
        );
        assert!(remote.panels.contains_key("fine"));
        assert!(!remote.panels.contains_key("bad id"));
        assert!(
            remote.drafts.is_empty(),
            "an unknown schema version is ignored"
        );
        assert_eq!(remote.focus, Some((MAX_ROWS - 1, None)));
    }

    fn e(pairs: &[(&str, Value)]) -> Entries {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn plan(applied: &Entries, remote: &Entries, local: &Entries, first: bool) -> Plan {
        reconcile(applied, remote, local, &BTreeSet::new(), first)
    }

    #[test]
    fn an_empty_daemon_is_seeded_from_this_desktop() {
        let local = e(&[("layout", json!(1)), ("panel/a", json!(2))]);
        let p = plan(&Entries::new(), &Entries::new(), &local, true);
        assert_eq!(p.push.len(), 2);
        assert!(p.adopt.is_empty());
    }

    #[test]
    fn a_daemon_that_holds_state_is_the_truth_on_first_sight_even_over_a_cached_panel() {
        let remote = e(&[("panel/a", json!(1)), ("focus", json!({"row": 1}))]);
        let local = e(&[
            ("panel/a", json!(1)),
            ("panel/stale", json!(9)),
            ("focus", json!({"row": 0})),
        ]);
        let p = plan(&Entries::new(), &remote, &local, true);
        assert!(
            p.push.is_empty(),
            "nothing from the cache is written over the daemon"
        );
        assert_eq!(
            p.adopt.iter().map(String::as_str).collect::<Vec<_>>(),
            ["focus", "panel/stale"]
        );
    }

    #[test]
    fn a_change_made_on_one_side_only_moves_to_the_other() {
        let base = e(&[("draft/a", json!("one"))]);
        let typed_here = e(&[("draft/a", json!("two"))]);
        let p = plan(&base, &base, &typed_here, false);
        assert_eq!(p.push, [("draft/a".to_owned(), Some(json!("two")))]);
        let p = plan(&base, &typed_here, &base, false);
        assert_eq!(p.adopt.iter().collect::<Vec<_>>(), ["draft/a"]);
        assert!(p.push.is_empty());
        // A deletion is a change like any other.
        let p = plan(&base, &Entries::new(), &base, false);
        assert_eq!(p.adopt.iter().collect::<Vec<_>>(), ["draft/a"]);
        let p = plan(&base, &base, &Entries::new(), false);
        assert_eq!(p.push, [("draft/a".to_owned(), None)]);
    }

    #[test]
    fn a_draft_typed_on_both_sides_keeps_the_focused_composer_and_otherwise_takes_the_daemons() {
        let base = e(&[("draft/a", json!("base"))]);
        let here = e(&[("draft/a", json!("mine"))]);
        let there = e(&[("draft/a", json!("theirs"))]);
        let focused: BTreeSet<String> = ["draft/a".to_owned()].into();
        let p = reconcile(&base, &there, &here, &focused, false);
        assert_eq!(p.push, [("draft/a".to_owned(), Some(json!("mine")))]);
        assert!(p.adopt.is_empty());
        let p = reconcile(&base, &there, &here, &BTreeSet::new(), false);
        assert_eq!(p.adopt.iter().collect::<Vec<_>>(), ["draft/a"]);
        assert!(p.push.is_empty());
        // Both sides arriving at the same text needs no write at all.
        assert_eq!(plan(&base, &here, &here, false), Plan::default());
    }

    #[test]
    fn the_focused_composer_keeps_its_text_when_only_another_desktop_changed_it() {
        let base = e(&[("draft/a", json!("mine, already written"))]);
        let there = e(&[("draft/a", json!("theirs"))]);
        let focused: BTreeSet<String> = ["draft/a".to_owned()].into();
        let p = reconcile(&base, &there, &base, &focused, false);
        assert_eq!(p, Plan::default());
        let p = reconcile(&base, &there, &base, &BTreeSet::new(), false);
        assert_eq!(p.adopt.iter().collect::<Vec<_>>(), ["draft/a"]);
    }

    #[test]
    fn focus_is_every_desktops_own_so_this_desktop_never_gives_it_up() {
        let base = e(&[("focus", json!({"row": 0}))]);
        let p = plan(
            &base,
            &e(&[("focus", json!({"row": 2}))]),
            &e(&[("focus", json!({"row": 1}))]),
            false,
        );
        assert_eq!(p.push, [("focus".to_owned(), Some(json!({"row": 1})))]);
    }

    #[test]
    fn offline_edits_and_the_daemons_other_edits_both_survive_a_reconnect() {
        let base = e(&[("draft/a", json!("a")), ("draft/b", json!("b"))]);
        let local = e(&[
            ("draft/a", json!("a, edited offline")),
            ("draft/b", json!("b")),
        ]);
        let remote = e(&[
            ("draft/a", json!("a")),
            ("draft/b", json!("b, edited elsewhere")),
        ]);
        let p = plan(&base, &remote, &local, false);
        assert_eq!(
            p.push,
            [("draft/a".to_owned(), Some(json!("a, edited offline")))]
        );
        assert_eq!(p.adopt.iter().collect::<Vec<_>>(), ["draft/b"]);
    }

    #[test]
    fn numbers_compare_the_way_the_daemon_stores_them() {
        // A width such as 0.333 survives text and the daemon's f32 round trip.
        let facts = Facts {
            panels: vec![PanelFacts {
                width: 1.0 / 3.0,
                ..session("a", 0, "")
            }],
            focus: None,
        };
        let written = canonical(&entries(&facts, None));
        let text = serde_json::to_string(&written).unwrap();
        let back: Entries = serde_json::from_str(&text).unwrap();
        assert_eq!(canonical(&back), written);
    }
}
