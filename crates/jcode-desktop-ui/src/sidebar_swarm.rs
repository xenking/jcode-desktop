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

/// Compact rows nested under a coordinator: status glyph, name, task and age.
/// Each row opens the worker's chat.
pub(super) fn render_workers(
    sidebar_index: usize,
    workers: &[(usize, crate::swarm_view::Worker)],
    folded: usize,
    under_checkout: bool,
    marks: &mut crate::swarm_glyph::Marks,
    expanded_lines: &HashSet<String>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let theme = Theme::global();
    let base = if under_checkout { 34.0 } else { 26.0 };
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
                .gap(px(5.0))
                .rounded_md()
                .cursor_pointer()
                .hover(|el| el.bg(theme.PANEL_BG))
                .text_size(px(10.5))
                .text_color(theme.TEXT_DIM)
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
                .child(div().flex_none().child(worker.name.clone()))
                .children(worker.label.clone().map(|label| {
                    div()
                        .min_w_0()
                        .when(!expanded, |el| el.truncate())
                        .when(expanded, |el| el.max_w_full())
                        .text_color(theme.TEXT_FAINT)
                        .child(label)
                }))
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
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.TEXT_FAINT)
                        .child(worker.status_text()),
                )
        }))
        .when(folded > 0, |el| {
            el.child(
                div()
                    .debug_selector(move || format!("sidebar-workers-folded-{sidebar_index}"))
                    .ml(px(base + 5.0))
                    .h(px(16.0))
                    .flex()
                    .items_center()
                    .text_size(px(10.0))
                    .text_color(theme.TEXT_FAINT)
                    .child(format!("+{folded} finished")),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn lead_with_workers(
        w: &mut Workspace,
        cx: &mut Context<Workspace>,
        workers: &[(&str, &str)],
    ) {
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
                &[("session_rose_1_a", "ready"), ("session_tulip_2_b", "failed")],
            );
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("composer-workers").is_none());
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
        for (index, glyph) in ["spinner", "check", "cross", "pause"].into_iter().enumerate() {
            let row = vcx.debug_bounds(sel(format!("sidebar-worker-0-{index}"))).unwrap();
            assert!(vcx.debug_bounds(sel(format!("sidebar-worker-glyph-0-{index}-{glyph}"))).is_some(), "{glyph}");
            assert!(row.origin.y >= lead.origin.y + lead.size.height, "below coordinator");
        }
        let title = vcx.debug_bounds("sidebar-session-title-0").unwrap();
        let row = vcx.debug_bounds("sidebar-worker-0-0").unwrap();
        assert!(row.origin.x > title.origin.x - gpui::px(24.0), "nested, not flush left");
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
            let refs = workers.iter().map(|(id, s)| (id.as_str(), *s)).collect::<Vec<_>>();
            lead_with_workers(w, cx, &refs);
        });
        vcx.run_until_parked();
        // 1 live + 3 latest finished shown, 5 folded.
        assert!(vcx.debug_bounds("sidebar-worker-0-3").is_some());
        assert!(vcx.debug_bounds("sidebar-worker-0-4").is_none());
        assert!(vcx.debug_bounds("sidebar-workers-folded-0").is_some());
        assert!(vcx.debug_bounds("sidebar-worker-glyph-0-0-spinner").is_some()
            || vcx.debug_bounds("sidebar-worker-glyph-0-1-spinner").is_some());
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
                &[("session_rose_1_a", "running"), ("session_tulip_2_b", "running")],
            );
        });
        vcx.run_until_parked();
        // Composer: click the count, then a worker row.
        let pill = vcx.debug_bounds("composer-workers").unwrap().center();
        vcx.simulate_click(pill, gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("swarm-list").is_some(), "count opens the list");
        let row = vcx.debug_bounds("swarm-list-worker-1").unwrap().center();
        vcx.simulate_click(row, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, "session_tulip_2_b");
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
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, "session_rose_1_a");
        });
    }
}
