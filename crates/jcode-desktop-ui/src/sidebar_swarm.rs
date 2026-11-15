//! Swarm ownership is distinct from transcript ancestry: user forks stay roots.
use super::*;
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
pub(super) fn layout_key(workers: &[(usize, crate::swarm_view::Worker)]) -> String {
    workers
        .iter()
        .map(|(depth, w)| format!("{depth}:{}:{}", w.name, w.state.label()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Compact rows under a coordinator: dot, name, task, state and age.
pub(super) fn render_workers(
    sidebar_index: usize,
    workers: &[(usize, crate::swarm_view::Worker)],
    under_checkout: bool,
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
            div()
                .id(gpui::SharedString::from(format!(
                    "sidebar-worker-{sidebar_index}-{index}"
                )))
                .debug_selector(move || format!("sidebar-worker-{sidebar_index}-{index}"))
                .ml(px(base + (*depth as f32 - 1.0) * 10.0))
                .mr(px(crate::scrollbar::GUTTER + 8.0))
                .h(px(18.0))
                .px_1()
                .flex()
                .items_center()
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
                        .size(px(6.0))
                        .rounded_full()
                        .bg(crate::panel::swarm_state_color(worker.state)),
                )
                .child(div().flex_none().child(worker.name.clone()))
                .children(worker.label.clone().map(|label| {
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.TEXT_FAINT)
                        .child(label)
                }))
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.TEXT_FAINT)
                        .child(worker.status_text()),
                )
        }))
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
            w.sessions.extend([
                session("child1", Some("root")),
                session("child2", Some("root")),
                session("child3", Some("root")),
                session("grandchild", Some("child3")),
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

    #[gpui::test]
    fn coordinator_strip_shows_real_wait_and_opens_workers(cx: &mut gpui::TestAppContext) {
        let (workspace, vcx) =
            cx.add_window_view(|_, cx| Workspace::for_test(learning::Coach::new(), cx));
        let mut worker = session("session_rose_1_a", Some("lead"));
        worker.swarm_status = Some("running".into());
        let mut failed = session("session_tulip_2_b", Some("lead"));
        failed.swarm_status = Some("failed".into());
        let lead = session("lead", None);
        workspace.update(vcx, |w, cx| {
            w.sessions = vec![lead.clone(), worker, failed];
            w.active = w.open_session(lead, cx);
            w.swarm_awaits.insert(
                "lead".into(),
                crate::swarm_view::AwaitState {
                    waiting_on: vec!["session_rose_1_a".into()],
                    reports: vec![crate::swarm_view::Report {
                        name: "otter".into(),
                        at_ms: swarm_now_ms(),
                        woke: true,
                    }],
                },
            );
            w.sync_swarm_strips(cx);
        });
        vcx.run_until_parked();
        for selector in [
            "swarm-strip",
            "swarm-strip-waiting",
            "swarm-strip-report",
            "swarm-strip-worker-0",
            "swarm-strip-worker-1",
            "sidebar-worker-0-0",
            "sidebar-worker-0-1",
        ] {
            assert!(vcx.debug_bounds(selector).is_some(), "{selector} renders");
        }
        let center = vcx.debug_bounds("swarm-strip-worker-1").unwrap().center();
        vcx.simulate_click(center, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots.len(), 2);
            assert_eq!(
                w.slots[w.active].panel.read(cx).session_id,
                "session_tulip_2_b"
            );
        });
        let center = vcx.debug_bounds("sidebar-worker-0-0").unwrap().center();
        vcx.simulate_click(center, gpui::Modifiers::default());
        vcx.run_until_parked();
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots.len(), 3);
            assert_eq!(
                w.slots[w.active].panel.read(cx).session_id,
                "session_rose_1_a"
            );
        });
    }
}
