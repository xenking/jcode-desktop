//! Swarm ownership is distinct from transcript ancestry: user forks stay roots.
use super::*;
use crate::panel::expandable;
use jcode_sdk::SessionInfo;

#[derive(Clone)]
pub(super) struct Child {
    pub session: SessionInfo,
    pub depth: usize,
}

#[derive(Default)]
pub(super) struct Groups {
    pub children: HashMap<String, Vec<Child>>,
    pub nested: HashSet<String>,
}

pub(super) fn groups(sessions: &[SessionInfo]) -> Groups {
    let by_id = sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect::<HashSet<_>>();
    let mut children_by_parent = HashMap::<&str, Vec<&SessionInfo>>::new();
    let mut roots = Vec::new();
    for session in sessions {
        if let Some(parent) = session
            .parent_session_id
            .as_deref()
            .filter(|parent| by_id.contains(parent))
        {
            children_by_parent.entry(parent).or_default().push(session);
        } else {
            roots.push(session);
        }
    }
    let mut result = Groups::default();
    // A single-parent cycle (and every descendant of it) has no reachable root.
    // Leaving those nodes unvisited preserves their independent sidebar rows.
    // Missing-parent sessions are roots, so their valid descendants still nest.
    for root in roots {
        let Some(first_children) = children_by_parent.get(root.session_id.as_str()) else {
            continue;
        };
        let mut children = Vec::new();
        let mut pending = first_children
            .iter()
            .rev()
            .map(|child| (*child, 1))
            .collect::<Vec<_>>();
        while let Some((session, depth)) = pending.pop() {
            // Catalog IDs are normally unique. Duplicate records must neither
            // create a reachable traversal cycle nor hide this root itself.
            if session.session_id == root.session_id
                || !result.nested.insert(session.session_id.clone())
            {
                continue;
            }
            children.push(Child {
                session: session.clone(),
                depth,
            });
            if let Some(descendants) = children_by_parent.get(session.session_id.as_str()) {
                // Reverse the stack insertion to preserve catalog sibling order.
                // Iteration also avoids recursion depth proportional to the swarm.
                pending.extend(descendants.iter().rev().map(|child| (*child, depth + 1)));
            }
        }
        result.children.insert(root.session_id.clone(), children);
    }
    result
}

/// Add every swarm descendant of a kept session. Headless workers are never
/// opened in this window, so the window-scoped filter would drop them and
/// the coordinator would look like it has no workers.
pub(super) fn keep_workers_of(sessions: &[SessionInfo], kept: &mut HashSet<String>) {
    loop {
        let before = kept.len();
        for session in sessions {
            if session
                .parent_session_id
                .as_deref()
                .is_some_and(|parent| parent != session.session_id && kept.contains(parent))
            {
                kept.insert(session.session_id.clone());
            }
        }
        if kept.len() == before {
            break;
        }
    }
}

/// Coordinators that own live work, whichever client drives them.
///
/// A swarm is not a window: the TUI shows a coordinator's workers inside the
/// coordinator's session, and the same workers keep running while nobody has
/// that coordinator open. The window-scoped sidebar filter only keeps sessions
/// this window opened or some client is attached to, so a coordinator driven
/// from the TUI, from another window, or headless disappears from Desktop
/// together with everything it owns. Listing live swarms keeps the two clients
/// showing the same work.
///
/// The outermost coordinator in the catalog wins, so a live grandchild keeps
/// its whole chain visible. A live worker whose coordinator is no longer in the
/// catalog stays listed on its own rather than vanishing.
pub(super) fn live_swarm_roots(
    sessions: &[SessionInfo],
    metas: &HashMap<String, jcode_sdk::WorkerMeta>,
    now_ms: u64,
) -> HashSet<String> {
    let by_id = sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<HashMap<_, _>>();
    let mut roots = HashSet::new();
    for session in sessions {
        if !crate::swarm_view::resolve_worker_state(session, metas.get(&session.session_id), now_ms)
            .is_live()
        {
            continue;
        }
        let Some(parent) = session
            .parent_session_id
            .as_deref()
            .filter(|parent| *parent != session.session_id)
        else {
            // A coordinator is listed through its live workers, not its own
            // record: a swarm whose work finished is history like any other.
            continue;
        };
        if !by_id.contains_key(parent) {
            roots.insert(session.session_id.clone());
            continue;
        }
        let mut root = parent;
        let mut seen = HashSet::from([session.session_id.as_str()]);
        while let Some(next) = by_id
            .get(root)
            .and_then(|session| session.parent_session_id.as_deref())
            .filter(|next| *next != root && seen.insert(*next))
        {
            root = next;
        }
        roots.insert(root.to_string());
    }
    roots
}

/// Layout identity of a coordinator's nested worker rows.
pub(super) fn layout_key(workers: &[(usize, crate::swarm_view::Worker)], folded: usize) -> String {
    workers
        .iter()
        .map(|(depth, w)| format!("{depth}:{}:{}", w.name, w.state.label()))
        .chain((folded > 0).then(|| format!("+{folded}")))
        .collect::<Vec<_>>()
        .join(",")
}

/// Live workers plus the latest few finished ones, with how many finished
/// ones were folded into the `+N finished` line.
pub(super) fn visible(
    workers: &[(usize, crate::swarm_view::Worker)],
) -> (Vec<(usize, crate::swarm_view::Worker)>, usize) {
    crate::swarm_view::visible_workers(workers, |(_, worker)| worker.state)
}

pub(super) fn finished_key(session_id: &str) -> String {
    format!("sidebar-finished:{session_id}")
}

/// Compact rows nested under a coordinator: status glyph, the worker's animal
/// (its name is in the tooltip), task, model, spend and live state. Each row
/// opens the worker's chat.
pub(super) fn render_workers(
    sidebar_index: usize,
    session_id: &str,
    workers: &[(usize, crate::swarm_view::Worker)],
    folded: usize,
    under_checkout: bool,
    marks: &mut crate::swarm_glyph::Marks,
    expanded_lines: &HashSet<String>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let theme = Theme::global();
    let base = if under_checkout { 34.0 } else { 26.0 };
    let key = finished_key(session_id);
    let expanded = expanded_lines.contains(&key);
    div()
        .debug_selector(move || format!("sidebar-workers-{sidebar_index}"))
        .flex()
        .flex_col()
        .mb(px(2.0))
        .children(workers.iter().enumerate().map(|(index, (depth, worker))| {
            let id = worker.session_id.clone();
            // The row stays one line while collapsed. A worker whose task name
            // does not fit gets a chevron that wraps it in place, so the whole
            // assignment is readable without leaving the sidebar.
            let expand_key = expandable::key::worker(&worker.session_id);
            let expanded = expanded_lines.contains(&expand_key);
            let toggle_key = expand_key.clone();
            let label_truncated = worker
                .label
                .as_deref()
                .is_some_and(|label| expandable::preview(label, expandable::LABEL).is_some());
            let mark = worker.state.glyph().map(|glyph| {
                marks.element(
                    &format!("worker:{}", worker.session_id),
                    crate::swarm_glyph::Mark::glyph(glyph, 11.0),
                    cx,
                )
            });
            let glyph_selector = worker
                .state
                .glyph()
                .map(|glyph| format!("{glyph:?}").to_ascii_lowercase());
            let tooltip = worker.tooltip();
            let tooltip_name = worker.name.clone();
            let model = worker.model_label();
            let effort = worker
                .effort_level()
                .and_then(|effort| crate::effort_icon::icon(&effort, 10.0));
            let provider = crate::provider_mark::render(
                worker.meta.provider.as_deref(),
                worker.meta.model.as_deref(),
                10.0,
                theme.TEXT_FAINT,
            );
            // The sidebar is one narrow column: the task, the model and what
            // the worker is doing win the space. Spend stays one hover away.
            div()
                .id(gpui::SharedString::from(format!(
                    "sidebar-worker-{sidebar_index}-{index}"
                )))
                .debug_selector(move || format!("sidebar-worker-{sidebar_index}-{index}"))
                .ml(px(base + (*depth as f32 - 1.0) * 10.0))
                .mr(px(crate::scrollbar::GUTTER + 8.0))
                .min_h(px(18.0))
                .px_1()
                .flex()
                .when(expanded, |el| el.items_start().flex_wrap())
                .when(!expanded, |el| el.items_center())
                .gap(px(3.0))
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(theme.PANEL_BG))
                .text_size(px(10.5))
                .text_color(theme.TEXT_DIM)
                .tooltip(move |_, cx| {
                    cx.new(|_| {
                        crate::panel::usage::RowsTooltip::separated(
                            tooltip_name.clone(),
                            tooltip
                                .strip_prefix(&format!("{} · ", tooltip_name))
                                .unwrap_or(&tooltip),
                        )
                    })
                    .into()
                })
                .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_swarm_worker(&id, cx);
                    this.focus_active(window, cx);
                }))
                .child(
                    div()
                        .flex_none()
                        .size(px(11.0))
                        .debug_selector(move || {
                            format!(
                                "sidebar-worker-glyph-{sidebar_index}-{index}-{}",
                                glyph_selector.as_deref().unwrap_or("none")
                            )
                        })
                        .children(mark),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.0))
                        .children(worker.icon.map(|icon| icon.to_owned())),
                )
                .children(worker.label.clone().map(|label| {
                    div()
                        .min_w_0()
                        // The task is what the row is for; the model, tokens
                        // and state keep their width, so the label takes the
                        // rest instead of collapsing to nothing.
                        .flex_1()
                        .min_w(px(24.0))
                        .when(!expanded, |el| el.truncate())
                        .when(expanded, |el| el.max_w_full())
                        .text_color(theme.TEXT_FAINT)
                        .child(label)
                }))
                // A worker with no task label keeps the trailing cells right.
                .when(worker.label.is_none(), |el| el.child(div().flex_1()))
                .when(label_truncated, |el| {
                    let key = toggle_key.clone();
                    el.child(expandable::glyph_toggle(
                        format!("sidebar-worker-expand-{sidebar_index}-{index}").into(),
                        expanded,
                        cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            expandable::flip(&mut this.expanded_lines, &key);
                            cx.notify();
                        }),
                    ))
                })
                .children(model.map(|model| {
                    div()
                        .debug_selector(move || {
                            format!("sidebar-worker-model-{sidebar_index}-{index}")
                        })
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        // Metadata sits a half step below the task, so the
                        // task keeps a readable width in this column.
                        .text_size(px(9.5))
                        .text_color(theme.TEXT_FAINT)
                        .child(provider)
                        .child(model)
                        // The rung rides beside the model: one circle is the
                        // level, and the tooltip still spells it out.
                        .children(effort.map(|effort| {
                            div()
                                .debug_selector(move || {
                                    format!("sidebar-worker-effort-{sidebar_index}-{index}")
                                })
                                .flex_none()
                                .child(effort)
                        }))
                }))
                .child(
                    div()
                        .debug_selector(move || {
                            format!("sidebar-worker-state-{sidebar_index}-{index}")
                        })
                        .flex_none()
                        .text_size(px(9.5))
                        .text_color(theme.TEXT_FAINT)
                        // The clock that used to read "running 26m" for every
                        // worker is the composer's; this column is too narrow
                        // for it and the state word is the part that differs.
                        .child(worker.state_text()),
                )
        }))
        .when(folded > 0, |el| {
            el.child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "sidebar-workers-folded-{sidebar_index}"
                    )))
                    .debug_selector(move || format!("sidebar-workers-folded-{sidebar_index}"))
                    .ml(px(base + 5.0))
                    .h(px(16.0))
                    .flex()
                    .items_center()
                    .text_size(px(10.0))
                    .text_color(theme.TEXT_FAINT)
                    .cursor_pointer()
                    .hover(|el| el.text_color(theme.TEXT))
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        expandable::flip(&mut this.expanded_lines, &key);
                        cx.notify();
                    }))
                    .child(if expanded {
                        "Hide finished".to_owned()
                    } else {
                        format!("+{folded} finished")
                    }),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offline screenshot fixture is what the rows are reviewed against.
    /// If its worker records do not resolve, every row silently loses its
    /// model and spend, which is exactly what the screenshot is meant to show.
    #[test]
    fn screenshot_fixture_workers_resolve_their_records() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../jcode-desktop-harness/src/fixtures");
        let identity = "session_ant_1794675020922_f6e720f45031cad8".to_string();
        let meta = jcode_sdk::worker_meta_for(&fixtures.join("microbe_home"), &[identity.clone()]);
        assert_eq!(
            meta[&identity].model.as_deref(),
            Some("claude-opus-5-5"),
            "the fixture records are read from <home>/sessions"
        );
        let spent = "session_rose_1794717290730_574aea8d9f6adcc3".to_string();
        let usage = jcode_sdk::worker_meta_for(&fixtures.join("usage_home"), &[spent.clone()]);
        assert!(
            usage[&spent].input_tokens.unwrap_or(0) > 0,
            "the usage fixture carries real token counts"
        );
    }
    fn session(id: &str, parent: Option<&str>) -> SessionInfo {
        let mut session = super::super::tests::session_info(id, Some(id));
        session.parent_session_id = parent.map(str::to_owned);
        session
    }

    // The pre-optimization algorithm is an independent equivalence oracle and
    // same-binary benchmark control. Keep its ancestry walks and full rescans.
    fn legacy_groups(sessions: &[SessionInfo]) -> Groups {
        let by_id = sessions
            .iter()
            .map(|s| (s.session_id.as_str(), s))
            .collect::<HashMap<_, _>>();
        let mut result = Groups::default();
        for session in sessions {
            let mut current = session;
            let mut seen = HashSet::from([session.session_id.as_str()]);
            let mut depth = 0;
            let mut cycle = false;
            while let Some(parent) = current
                .parent_session_id
                .as_deref()
                .and_then(|id| by_id.get(id))
            {
                if !seen.insert(parent.session_id.as_str()) {
                    cycle = true;
                    break;
                }
                depth += 1;
                current = parent;
            }
            if depth > 0 && !cycle {
                result.nested.insert(session.session_id.clone());
                result
                    .children
                    .entry(current.session_id.clone())
                    .or_default()
                    .push(Child {
                        session: session.clone(),
                        depth,
                    });
            }
        }
        for children in result.children.values_mut() {
            let original = std::mem::take(children);
            fn append(parent: &str, source: &[Child], target: &mut Vec<Child>) {
                for child in source
                    .iter()
                    .filter(|c| c.session.parent_session_id.as_deref() == Some(parent))
                {
                    target.push(child.clone());
                    append(&child.session.session_id, source, target);
                }
            }
            let root = original.first().map(|child| {
                let mut current = &child.session;
                while let Some(parent) = current
                    .parent_session_id
                    .as_deref()
                    .and_then(|id| by_id.get(id))
                {
                    current = parent;
                }
                current.session_id.clone()
            });
            if let Some(root) = root {
                append(&root, &original, children);
            }
        }
        result
    }

    fn assert_equivalent(sessions: &[SessionInfo]) {
        let actual = groups(sessions);
        let expected = legacy_groups(sessions);
        assert_eq!(actual.nested, expected.nested);
        assert_eq!(actual.children.len(), expected.children.len());
        for (root, expected_children) in expected.children {
            let actual_children = &actual.children[&root];
            assert_eq!(actual_children.len(), expected_children.len());
            for (actual, expected) in actual_children.iter().zip(expected_children) {
                assert_eq!(actual.depth, expected.depth);
                assert_eq!(
                    serde_json::to_value(&actual.session).unwrap(),
                    serde_json::to_value(&expected.session).unwrap()
                );
            }
        }
    }

    #[test]
    fn sidebar_swarm_adjacency_matches_legacy_for_all_small_parent_graphs() {
        // Exhaust all four-node single-parent graphs, including missing parents,
        // self cycles, longer cycles and descendants of cycles. Reverse catalog
        // order as well, so child-before-parent inputs and sibling order matter.
        let ids = ["a", "b", "c", "d"];
        let parents = [
            None,
            Some("missing"),
            Some("a"),
            Some("b"),
            Some("c"),
            Some("d"),
        ];
        for mut graph in 0..parents.len().pow(ids.len() as u32) {
            let mut sessions = ids
                .iter()
                .map(|id| {
                    let parent = parents[graph % parents.len()];
                    graph /= parents.len();
                    session(id, parent)
                })
                .collect::<Vec<_>>();
            assert_equivalent(&sessions);
            sessions.reverse();
            assert_equivalent(&sessions);
        }
        assert_equivalent(&[]);
    }

    #[test]
    fn sidebar_swarm_adjacency_preserves_depth_first_sibling_order_and_orphan_roots() {
        let sessions = vec![
            session("grandchild-b", Some("child-b")),
            session("child-b", Some("root")),
            session("orphan-child", Some("orphan")),
            session("grandchild-a", Some("child-a")),
            session("root", None),
            session("child-a", Some("root")),
            session("orphan", Some("missing")),
            session("cycle-a", Some("cycle-b")),
            session("cycle-b", Some("cycle-a")),
            session("cycle-descendant", Some("cycle-a")),
        ];
        assert_equivalent(&sessions);
        let groups = groups(&sessions);
        assert_eq!(
            groups.children["root"]
                .iter()
                .map(|c| (c.session.session_id.as_str(), c.depth))
                .collect::<Vec<_>>(),
            [
                ("child-b", 1),
                ("grandchild-b", 2),
                ("child-a", 1),
                ("grandchild-a", 2)
            ]
        );
        assert_eq!(
            groups.children["orphan"][0].session.session_id,
            "orphan-child"
        );
        assert!(!groups.nested.contains("cycle-descendant"));
    }

    fn scaling_sessions(count: usize, shape: &str) -> Vec<SessionInfo> {
        (0..count)
            .map(|index| {
                let parent = match (shape, index) {
                    ("flat", 1..) => Some("session-0".to_string()),
                    ("chain", 1..) => Some(format!("session-{}", index - 1)),
                    _ => None,
                };
                session(&format!("session-{index}"), parent.as_deref())
            })
            .collect()
    }

    #[test]
    fn sidebar_swarm_adjacency_handles_deep_chains_without_recursive_stack_growth() {
        let sessions = scaling_sessions(10_000, "chain");
        let groups = groups(&sessions);
        assert_eq!(groups.nested.len(), 9_999);
        let last = groups.children["session-0"].last().unwrap();
        assert_eq!(last.session.session_id, "session-9999");
        assert_eq!(last.depth, 9_999);
    }

    #[test]
    fn sidebar_swarm_adjacency_duplicate_ids_cannot_cycle_or_hide_the_root() {
        let sessions = vec![
            session("root", None),
            session("child", Some("root")),
            session("root", Some("child")),
            session("child", Some("child")),
        ];
        let groups = groups(&sessions);
        assert_eq!(groups.nested, HashSet::from(["child".into()]));
        assert_eq!(groups.children["root"].len(), 1);
        assert_eq!(groups.children["root"][0].session.session_id, "child");
        assert_eq!(groups.children["root"][0].depth, 1);
    }

    #[test]
    #[ignore = "manual same-binary component benchmark, not a presentation FPS test"]
    fn sidebar_swarm_grouping_profile() {
        for shape in ["independent", "flat", "chain"] {
            for count in [100, 500, 1_000] {
                let sessions = scaling_sessions(count, shape);
                assert_equivalent(&sessions);
                let mut old = Vec::new();
                let mut new = Vec::new();
                for iteration in 0..23 {
                    let mut sample = |optimized: bool| {
                        let started = std::time::Instant::now();
                        let result = if optimized {
                            groups(std::hint::black_box(&sessions))
                        } else {
                            legacy_groups(std::hint::black_box(&sessions))
                        };
                        std::hint::black_box(result);
                        if iteration >= 3 {
                            if optimized {
                                new.push(started.elapsed().as_micros());
                            } else {
                                old.push(started.elapsed().as_micros());
                            }
                        }
                    };
                    // Alternate order to reduce systematic warm-cache bias.
                    sample(iteration % 2 == 0);
                    sample(iteration % 2 != 0);
                }
                old.sort_unstable();
                new.sort_unstable();
                println!(
                    "SIDEBAR_GROUPS shape={shape} sessions={count} legacy_p50={}us legacy_p95={}us adjacency_p50={}us adjacency_p95={}us",
                    old[9], old[18], new[9], new[18]
                );
            }
        }
    }

    #[test]
    fn sidebar_swarm_groups_owned_agents_recursively_without_hiding_orphans_or_cycles() {
        let sessions = vec![
            session("root", None),
            session("grandchild", Some("child")),
            session("child", Some("root")),
            session("fork", None),
            session("orphan", Some("missing")),
            session("a", Some("b")),
            session("b", Some("a")),
            session("self", Some("self")),
        ];
        let groups = groups(&sessions);
        assert_eq!(
            groups.children["root"]
                .iter()
                .map(|c| (c.session.session_id.as_str(), c.depth))
                .collect::<Vec<_>>(),
            vec![("child", 1), ("grandchild", 2)]
        );
        assert_eq!(
            groups.nested,
            HashSet::from(["child".into(), "grandchild".into()])
        );
    }

    #[test]
    fn headless_workers_are_kept_with_their_coordinator() {
        let sessions = vec![
            session("lead", None),
            session("w1", Some("lead")),
            session("w2", Some("w1")),
            session("stray", Some("other")),
            session("self", Some("self")),
        ];
        let mut kept = HashSet::from(["lead".to_string()]);
        keep_workers_of(&sessions, &mut kept);
        let mut kept = kept.into_iter().collect::<Vec<_>>();
        kept.sort();
        assert_eq!(kept, vec!["lead", "w1", "w2"]);
    }

    /// Live work is not window state: a coordinator is listed because its
    /// workers are still running, and a finished swarm stays history.
    #[test]
    fn live_swarms_are_the_coordinators_that_still_own_work() {
        let live = |id: &str, parent: Option<&str>| {
            let mut worker = session(id, parent);
            worker.swarm_status = Some("running".into());
            worker
        };
        let mut done = session("done-worker", Some("idle-lead"));
        done.swarm_status = Some("ready".into());
        let sessions = vec![
            session("idle-lead", None),
            done,
            session("lead", None),
            // A live grandchild keeps its whole chain, not just its parent.
            live("child", Some("lead")),
            live("grandchild", Some("child")),
            // An orphan worker whose coordinator left the catalog still counts.
            live("orphan", Some("missing")),
            // Cycles must not hang or pick a parent.
            live("cycle-a", Some("cycle-b")),
            live("cycle-b", Some("cycle-a")),
            // Blocked is waiting on the swarm, queued is spawned: both live.
            {
                let mut blocked = session("blocked", Some("blocked-lead"));
                blocked.swarm_status = Some("blocked".into());
                blocked
            },
            session("blocked-lead", None),
        ];
        let roots = live_swarm_roots(&sessions, &HashMap::new(), 0);
        assert!(roots.contains("lead"), "{roots:?}");
        assert!(roots.contains("orphan"), "{roots:?}");
        assert!(roots.contains("blocked-lead"), "{roots:?}");
        assert!(!roots.contains("idle-lead"), "a finished swarm is history");
        assert!(!roots.contains("child"), "the outermost coordinator wins");
        assert!(!roots.contains("grandchild"));
    }

    #[test]
    fn live_swarm_roots_terminate_on_a_cycle() {
        let mut a = session("a", Some("b"));
        a.swarm_status = Some("running".into());
        let mut b = session("b", Some("a"));
        b.swarm_status = Some("running".into());
        let roots = live_swarm_roots(&[a, b], &HashMap::new(), 0);
        // A parent cycle has no reachable coordinator, so neither member nests;
        // both are live and both stay listed on their own.
        assert_eq!(roots, HashSet::from(["a".to_string(), "b".to_string()]));
    }

    #[gpui::test]
    fn sidebar_swarm_metadata_refresh_keeps_showcase_absent_and_rows_compact(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        let sessions = vec![session("root", None), session("child", None)];
        workspace.update(vcx, |w, cx| {
            w.sidebar_closed_sessions
                .extend(["root".into(), "child".into()]);
            w.apply(
                Update::Sessions {
                    sessions: sessions.clone(),
                },
                cx,
            );
            cx.notify();
        });
        vcx.run_until_parked();
        let normal = vcx.debug_bounds("sidebar-session-0").unwrap().size.height;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("swarm.json"), r#"{"members":[{"session_id":"child","report_back_to_session_id":"root","task_label":"Verify nested sidebar","status":"running"}]}"#).unwrap();
        let mut enriched = sessions.clone();
        jcode_sdk::enrich_sessions_from_swarm_state(&mut enriched, directory.path());
        assert_eq!(
            enriched[1].agent_label.as_deref(),
            Some("Verify nested sidebar")
        );
        assert_eq!(enriched[1].swarm_status.as_deref(), Some("running"));
        workspace.update(vcx, |w, cx| {
            w.apply(Update::Sessions { sessions: enriched }, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        for selector in [
            "swarm-child-child",
            "swarm-toggle-root",
            "swarm-lead-root",
            "sidebar-session-1",
        ] {
            assert!(
                vcx.debug_bounds(selector).is_none(),
                "{selector} must not render"
            );
        }
        assert_eq!(
            vcx.debug_bounds("sidebar-session-0").unwrap().size.height,
            normal
        );
        workspace.update(vcx, |w, cx| {
            w.apply(Update::Sessions { sessions }, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-child-child").is_none());
        assert_eq!(
            vcx.debug_bounds("sidebar-session-0").unwrap().size.height,
            normal
        );
    }

    #[gpui::test]
    fn sidebar_swarm_reopening_during_close_preserves_entity_and_restores_watch(
        cx: &mut gpui::TestAppContext,
    ) {
        let (bridge, commands) = harness::spawn_recording();
        let child = session("child", Some("root"));
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.set_test_bridge(bridge);
            w.sessions = vec![session("root", None), child.clone()];
            w.active = w.open_session(child.clone(), cx);
            w
        });
        vcx.run_until_parked();
        while commands.try_recv().is_ok() {}
        vcx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                let panel = w.slots[w.active].panel.clone();
                let input = panel.read(cx).input.clone();
                w.close_sidebar_sessions(vec!["child".into()], window, cx);
                assert!(w.slots[0].closing);
                w.activate_session(child.clone(), window, cx);
                assert_eq!(w.slots.len(), 1);
                assert!(!w.slots[0].closing);
                assert_eq!(w.slots[0].panel.entity_id(), panel.entity_id());
                assert_eq!(
                    w.slots[0].panel.read(cx).input.entity_id(),
                    input.entity_id()
                );
                w.apply(
                    Update::Event {
                        session_id: "child".into(),
                        event: jcode_sdk::ApiEvent::SessionStatus {
                            session_id: "child".into(),
                            status: "running".into(),
                            pending_soft_interrupts: None,
                        },
                    },
                    cx,
                );
                assert_eq!(panel.read(cx).status, "running");
            })
        });
        assert!(
            matches!(commands.try_recv(), Ok(Command::Unwatch { session_id }) if session_id == "child")
        );
        assert!(
            matches!(commands.try_recv(), Ok(Command::Watch { session_id }) if session_id == "child")
        );
        assert!(commands.try_recv().is_err());
    }

    #[gpui::test]
    fn sidebar_swarm_workers_nest_as_compact_rows_under_the_coordinator(
        cx: &mut gpui::TestAppContext,
    ) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            w.sidebar_closed_sessions
                .extend(["root".into(), "other".into()]);
            w.sessions = vec![session("root", None), session("other", None)];
            cx.notify();
        });
        vcx.run_until_parked();
        let root = vcx.debug_bounds("sidebar-session-0").unwrap();
        let other = vcx.debug_bounds("sidebar-session-1").unwrap();
        let header = vcx.debug_bounds("sidebar-navigation-tabs").unwrap();
        let list = vcx.debug_bounds("sidebar-session-list").unwrap();
        workspace.update(vcx, |w, cx| {
            // Live workers: finished ones beyond the latest few fold away.
            let live = |id: &str, parent: &str| {
                let mut worker = session(id, Some(parent));
                worker.swarm_status = Some("running".into());
                worker
            };
            w.sessions.extend([
                live("child1", "root"),
                live("child2", "root"),
                live("child3", "root"),
                live("grandchild", "child3"),
            ]);
            cx.notify();
        });
        vcx.run_until_parked();
        assert_eq!(vcx.debug_bounds("sidebar-session-0").unwrap(), root);
        // Worker rows sit between the coordinator and the next thread.
        let first = vcx.debug_bounds("sidebar-worker-0-0").unwrap();
        let last = vcx.debug_bounds("sidebar-worker-0-3").unwrap();
        assert!(first.origin.y >= root.origin.y + root.size.height);
        assert!(last.origin.x > first.origin.x, "grandchild is indented");
        assert!(vcx.debug_bounds("sidebar-session-1").unwrap().origin.y > other.origin.y);
        assert_eq!(vcx.debug_bounds("sidebar-navigation-tabs").unwrap(), header);
        assert_eq!(vcx.debug_bounds("sidebar-session-list").unwrap(), list);
        for selector in ["sidebar-worker-0-4", "sidebar-session-2"] {
            assert!(
                vcx.debug_bounds(selector).is_none(),
                "{selector} must not render"
            );
        }
    }

    fn sel(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    /// The rung rides beside the model on a nested worker row, the same circle
    /// the composer pill and the composer's worker list draw.
    #[gpui::test]
    fn sidebar_worker_rows_carry_the_effort_circle(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            let mut worker = session("rose", Some("lead"));
            worker.swarm_status = Some("running".into());
            w.sidebar_closed_sessions
                .extend(["lead".into(), "rose".into()]);
            w.sessions = vec![session("lead", None), worker];
            w.worker_meta.insert(
                "rose".into(),
                jcode_sdk::WorkerMeta {
                    model: Some("claude-opus-5-5".into()),
                    provider: Some("claude-oauth".into()),
                    effort: Some("high".into()),
                    ..jcode_sdk::WorkerMeta::default()
                },
            );
            cx.notify();
        });
        vcx.run_until_parked();
        let model = vcx
            .debug_bounds("sidebar-worker-model-0-0")
            .expect("the row shows its model");
        let effort = vcx
            .debug_bounds("sidebar-worker-effort-0-0")
            .expect("the row shows its rung as a circle");
        let state = vcx
            .debug_bounds("sidebar-worker-state-0-0")
            .expect("the row shows its state");
        assert!(
            model.left() < effort.left() && effort.right() <= state.left(),
            "effort follows the model: {model:?} {effort:?} {state:?}"
        );
    }

    /// A worker whose record names no effort keeps the row as it was.
    #[gpui::test]
    fn sidebar_worker_rows_without_effort_show_no_circle(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            let mut worker = session("rose", Some("lead"));
            worker.swarm_status = Some("running".into());
            w.sidebar_closed_sessions
                .extend(["lead".into(), "rose".into()]);
            w.sessions = vec![session("lead", None), worker];
            w.worker_meta.insert(
                "rose".into(),
                jcode_sdk::WorkerMeta {
                    model: Some("claude-opus-5-5".into()),
                    provider: Some("claude-oauth".into()),
                    ..jcode_sdk::WorkerMeta::default()
                },
            );
            cx.notify();
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("sidebar-worker-model-0-0").is_some());
        assert!(vcx.debug_bounds("sidebar-worker-effort-0-0").is_none());
    }

    fn lead_with_workers(w: &mut Workspace, cx: &mut Context<Workspace>, workers: &[(&str, &str)]) {
        let lead = session("lead", None);
        let mut sessions = vec![lead.clone()];
        for (id, status) in workers {
            let mut worker = session(id, Some("lead"));
            worker.swarm_status = Some((*status).into());
            sessions.push(worker);
        }
        w.sessions = sessions;
        w.active = w.open_session(lead, cx);
        w.sync_swarm_strips(cx);
    }

    /// The text strip is gone: no "Waiting for N workers" line, no
    /// "reported · coordinator woke" line, no chips above the composer.
    #[gpui::test]
    fn text_strip_is_gone_and_workers_show_as_a_compact_count(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            lead_with_workers(
                w,
                cx,
                &[
                    ("session_rose_1_a", "running"),
                    ("session_tulip_2_b", "running"),
                    ("session_otter_3_c", "blocked"),
                    ("session_fish_4_d", "failed"),
                ],
            );
        });
        vcx.run_until_parked();
        for gone in [
            "swarm-strip",
            "swarm-strip-waiting",
            "swarm-strip-report",
            "swarm-strip-worker-0",
        ] {
            assert!(vcx.debug_bounds(gone).is_none(), "{gone} must not render");
        }
        // Three live workers (failed does not count), indicator sits in the bar.
        let pill = vcx.debug_bounds("composer-workers").expect("indicator");
        assert!(vcx.debug_bounds("composer-workers-count").is_some());
        let tabs = vcx.debug_bounds("composer-tabs").unwrap();
        assert!(pill.top() >= tabs.top() && pill.bottom() <= tabs.bottom());
        assert!(pill.size.width < gpui::px(64.0), "compact: {pill:?}");
        workspace.read_with(vcx, |w, cx| {
            let panel = w.slots[w.active].panel.read(cx);
            assert_eq!(panel.test_swarm_indicator_count(), 3);
        });
    }

    #[gpui::test]
    fn indicator_is_absent_when_no_worker_is_running(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            lead_with_workers(
                w,
                cx,
                &[
                    ("session_rose_1_a", "ready"),
                    ("session_tulip_2_b", "failed"),
                ],
            );
        });
        vcx.run_until_parked();
        // Nothing runs now: the pill keeps its icon for the hover that lists
        // the finished workers, but counts no live worker.
        assert!(vcx.debug_bounds("composer-workers").is_some());
        assert!(vcx.debug_bounds("composer-workers-icon").is_some());
        assert!(vcx.debug_bounds("composer-workers-count").is_none());
        assert!(vcx.debug_bounds("swarm-list").is_none());
        // Worker rows stay in the sidebar, with their final glyphs.
        assert!(vcx.debug_bounds("sidebar-worker-glyph-0-0-check").is_some());
        assert!(vcx.debug_bounds("sidebar-worker-glyph-0-1-cross").is_some());
    }

    /// User spec: spinner = running, check = done, x = failed, pause = waiting,
    /// nested and indented under the coordinator row.
    #[gpui::test]
    fn sidebar_workers_nest_with_live_status_glyphs(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            lead_with_workers(
                w,
                cx,
                &[
                    ("session_rose_1_a", "running"),
                    ("session_tulip_2_b", "ready"),
                    ("session_otter_3_c", "failed"),
                    ("session_fish_4_d", "blocked"),
                ],
            );
        });
        vcx.run_until_parked();
        let lead = vcx.debug_bounds("sidebar-session-0").unwrap();
        for (index, glyph) in ["spinner", "check", "cross", "pause"]
            .into_iter()
            .enumerate()
        {
            let row = vcx
                .debug_bounds(sel(format!("sidebar-worker-0-{index}")))
                .unwrap();
            assert!(
                vcx.debug_bounds(sel(format!("sidebar-worker-glyph-0-{index}-{glyph}")))
                    .is_some(),
                "{glyph}"
            );
            assert!(
                row.origin.y >= lead.origin.y + lead.size.height,
                "below coordinator"
            );
        }
        let title = vcx.debug_bounds("sidebar-session-title-0").unwrap();
        let row = vcx.debug_bounds("sidebar-worker-0-0").unwrap();
        assert!(
            row.origin.x > title.origin.x - gpui::px(24.0),
            "nested, not flush left"
        );
        assert!(vcx.debug_bounds("sidebar-worker-0-4").is_none());
    }

    #[gpui::test]
    fn sidebar_folds_old_finished_workers_but_never_a_live_one(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            let mut workers = (0..8)
                .map(|i| (format!("session_w{i}_{i}_a"), "ready"))
                .collect::<Vec<_>>();
            workers.insert(1, ("session_live_9_z".into(), "running"));
            let refs = workers
                .iter()
                .map(|(id, s)| (id.as_str(), *s))
                .collect::<Vec<_>>();
            lead_with_workers(w, cx, &refs);
        });
        vcx.run_until_parked();
        // 1 live + 3 latest finished shown, 5 folded.
        assert!(vcx.debug_bounds("sidebar-worker-0-3").is_some());
        assert!(vcx.debug_bounds("sidebar-worker-0-4").is_none());
        assert!(vcx.debug_bounds("sidebar-workers-folded-0").is_some());
        let toggle = vcx
            .debug_bounds("sidebar-workers-folded-0")
            .unwrap()
            .center();
        vcx.simulate_click(toggle, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("sidebar-worker-0-8").is_some(),
            "all finished workers expand"
        );
        workspace.read_with(vcx, |w, _| {
            assert!(w.expanded_lines.contains(&finished_key("lead")));
            assert!(!w.expanded_lines.contains(&finished_key("other")));
        });
        workspace.update(vcx, |_, cx| cx.notify());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("sidebar-worker-0-8").is_some());
        let toggle = vcx
            .debug_bounds("sidebar-workers-folded-0")
            .unwrap()
            .center();
        vcx.simulate_click(toggle, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("sidebar-worker-0-4").is_none(),
            "second click collapses"
        );
        assert!(
            vcx.debug_bounds("sidebar-worker-glyph-0-0-spinner")
                .is_some()
                || vcx
                    .debug_bounds("sidebar-worker-glyph-0-1-spinner")
                    .is_some()
        );
    }

    #[gpui::test]
    fn todo_pill_activates_its_session(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            lead_with_workers(w, cx, &[]);
            w.slots[w.active].panel.update(cx, |panel, cx| panel.test_set_todos(r#"[{"content":"Ship","status":"in_progress","stage":2,"group":"desktop","assigned_to":"@self"}]"#, cx));
            let other = session("other", None);
            w.sessions.push(other.clone());
            w.active = w.open_session(other, cx);
            w.sidebar_todo_progress.insert("lead".into(), (1, 2));
            cx.notify();
        });
        vcx.run_until_parked();
        let row = workspace.read_with(vcx, |w, _| {
            w.sidebar_session_layout
                .iter()
                .position(|row| row.session_id == "lead")
                .unwrap()
        });
        let pill = vcx
            .debug_bounds(format!("sidebar-todos-{row}").leak())
            .unwrap()
            .center();
        vcx.simulate_click(pill, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, "lead")
        });
        assert!(vcx.debug_bounds("pinned-todo-card").is_some());
        let pill = vcx
            .debug_bounds(format!("sidebar-todos-{row}").leak())
            .unwrap()
            .center();
        vcx.simulate_click(pill, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-todo-card").is_none());
        vcx.simulate_click(pill, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("pinned-todo-card").is_some());
    }

    /// All three entry points open the worker: sidebar row, composer count
    /// list, and (see panel tests) the transcript card.
    #[gpui::test]
    fn sidebar_row_and_composer_list_open_the_worker(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            lead_with_workers(
                w,
                cx,
                &[
                    ("session_rose_1_a", "running"),
                    ("session_tulip_2_b", "running"),
                ],
            );
        });
        vcx.run_until_parked();
        // Composer: click the count, then a worker row.
        let pill = vcx.debug_bounds("composer-workers").unwrap().center();
        vcx.simulate_click(pill, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("swarm-list").is_some(),
            "count opens the list"
        );
        let row = vcx.debug_bounds("swarm-list-worker-1").unwrap().center();
        vcx.simulate_click(row, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(
                w.slots[w.active].panel.read(cx).session_id,
                "session_tulip_2_b"
            );
        });
        assert!(
            vcx.debug_bounds("swarm-list").is_none(),
            "opening a worker closes the composer list"
        );
        // Sidebar: click the first worker row.
        let center = vcx.debug_bounds("sidebar-worker-0-0").unwrap().center();
        vcx.simulate_click(center, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(
                w.slots[w.active].panel.read(cx).session_id,
                "session_rose_1_a"
            );
        });
    }

    const SHARK: &str = "session_shark_1794724670890_6b48665715bce735";
    const CALF: &str = "session_calf_1794729858023_e3a95f4d78ff82f0";
    const MICROBE: &str = "session_microbe_1794670947943_2648f3bbec7c7e5e";

    /// The shared daemon's own `/list_sessions` reply, captured read-only with
    /// `hello` + `list_sessions` over the API socket. One coordinator (`shark`)
    /// owns headless workers (`calf`, `ant`), another (`microbe`) owns three the
    /// user sees in the TUI, and the rest is ordinary history.
    fn live_catalog() -> Vec<SessionInfo> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/swarm_view_fixtures/live_session_catalog.json");
        let snapshot: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        serde_json::from_value(snapshot["sessions"].clone()).unwrap()
    }

    /// What the sidebar decided for one session: is it a row at all, and which
    /// workers hang beneath it.
    fn sidebar_row(w: &Workspace, id: &str) -> Option<(bool, String)> {
        w.sidebar_session_layout
            .iter()
            .find(|row| row.session_id == id)
            .map(|row| (row.collapsed, row.workers.clone()))
    }

    /// Workers of a coordinator this window never opened must still nest under
    /// it: the TUI shows them, so Desktop has to.
    #[gpui::test]
    fn live_catalog_keeps_a_coordinator_this_window_did_not_open(cx: &mut gpui::TestAppContext) {
        let mut catalog = live_catalog();
        // The user drives this swarm from the TUI, or from another window. No
        // client is attached here and this window has no panel for it.
        for session in &mut catalog {
            session.user_connected = false;
        }
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            let elsewhere = catalog
                .iter()
                .find(|session| session.session_id == MICROBE)
                .cloned()
                .unwrap();
            w.sessions = catalog.clone();
            w.active = w.open_session(elsewhere, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        let shark = workspace
            .read_with(vcx, |w, _| sidebar_row(w, SHARK))
            .expect("the live coordinator is a sidebar row");
        assert!(
            shark.1.contains("calf"),
            "calf nests under shark: {shark:?}"
        );
        assert!(
            workspace
                .read_with(vcx, |w, _| sidebar_row(w, CALF))
                .is_none(),
            "the worker has no row of its own"
        );
        // The row is not just modelled: it is drawn, with its worker beneath.
        let index = workspace
            .read_with(vcx, |w, _| {
                w.sidebar_session_layout
                    .iter()
                    .position(|row| row.session_id == SHARK)
            })
            .unwrap();
        assert!(
            vcx.debug_bounds(sel(format!("sidebar-worker-{index}-0")))
                .is_some(),
            "the coordinator's first worker row is on screen"
        );
    }

    /// The same catalog with the coordinator attached to this window, which is
    /// what the user sees when they open `shark` in Desktop.
    #[gpui::test]
    fn live_catalog_nests_headless_workers_under_an_open_coordinator(
        cx: &mut gpui::TestAppContext,
    ) {
        let catalog = live_catalog();
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        workspace.update(vcx, |w, cx| {
            let shark = catalog
                .iter()
                .find(|session| session.session_id == SHARK)
                .cloned()
                .unwrap();
            w.sessions = catalog.clone();
            w.active = w.open_session(shark, cx);
            cx.notify();
        });
        vcx.run_until_parked();
        let shark = workspace
            .read_with(vcx, |w, _| sidebar_row(w, SHARK))
            .unwrap();
        assert!(
            shark.1.contains("calf"),
            "calf nests under shark: {shark:?}"
        );
        assert!(shark.1.contains("ant"), "ant nests under shark: {shark:?}");
    }
}
