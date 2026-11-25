use super::*;

impl Workspace {
    pub(super) fn inspect_fleet_seat(
        &mut self,
        action: &crate::ui_actions::InspectFleetSeat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.slots.iter().position(|slot| {
            !slot.closing && slot.panel.session_id(cx) == crate::panel::openrig::SESSION_ID
        }) {
            self.slots[index]
                .panel
                .update_chat(cx, |panel, cx| panel.inspect_fleet_seat(action, cx));
            self.set_active(index, cx);
            self.focus_active(window, cx);
        } else {
            let current = window.window_handle();
            for handle in cx.windows().into_iter().filter(|handle| *handle != current) {
                if let Some(handle) = handle.downcast::<Workspace>() {
                    if handle.read(cx).is_ok_and(|other| {
                        other.slots.iter().any(|slot| {
                            !slot.closing
                                && slot.panel.session_id(cx) == crate::panel::openrig::SESSION_ID
                        })
                    }) {
                        let _ = handle.update(cx, |other, window, cx| {
                            other.inspect_fleet_seat(action, window, cx);
                            window.activate_window();
                        });
                        return;
                    }
                } else if let Some(handle) = handle.downcast::<panel_window::PanelWindow>() {
                    if handle.read(cx).is_ok_and(|other| {
                        other.item.read_chat(cx).is_some_and(|panel| {
                            panel.session_id == crate::panel::openrig::SESSION_ID
                        })
                    }) {
                        let _ = handle.update(cx, |other, window, cx| {
                            other
                                .item
                                .update_chat(cx, |panel, cx| panel.inspect_fleet_seat(action, cx));
                            window.activate_window();
                        });
                        return;
                    }
                }
            }
            let panel = cx.new(|cx| Panel::new_openrig(self.bridge.clone(), cx));
            if self.single_panel {
                if let Ok(hosted) = panel_window::open_panel_window(panel, None, window, cx) {
                    hosted.update(cx, |panel, cx| panel.inspect_fleet_seat(action, cx));
                }
            } else {
                panel.update(cx, |panel, cx| panel.inspect_fleet_seat(action, cx));
                self.insert_spawned_panel(panel, window, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn fleet_updates_only_notify_panels_whose_seat_hud_changed(cx: &mut gpui::TestAppContext) {
        let seat_id = "session_fox_1700000000000_0123456789abcdef";
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel(seat_id, cx);
            workspace.push_test_panel("ordinary-session", cx);
            for slot in &workspace.slots {
                slot.panel.update_chat(cx, |panel, cx| {
                    panel.set_rig_host_id(Some("host-fixture-local".into()), cx);
                });
            }
            workspace
        });
        vcx.run_until_parked();
        let notifications = workspace.update_in(vcx, |workspace, _, cx| {
            workspace
                .slots
                .iter()
                .map(|slot| {
                    let count = std::rc::Rc::new(std::cell::Cell::new(0));
                    let observed = count.clone();
                    cx.observe(slot.panel.chat().unwrap(), move |_, _, _| {
                        observed.set(observed.get() + 1)
                    })
                    .detach();
                    count
                })
                .collect::<Vec<_>>()
        });
        let publish = |vcx: &mut gpui::VisualTestContext, snapshot| {
            vcx.update(|_, cx| {
                crate::openrig_feed::get(cx).update(cx, |feed, cx| {
                    feed.snapshot = Some(snapshot);
                    cx.notify();
                });
            });
            vcx.run_until_parked();
        };
        let mut snapshot = jcode_desktop_openrig::Snapshot::fixture();
        publish(vcx, snapshot.clone());
        assert_eq!(notifications[0].get(), 1, "the linked HUD must appear");
        assert_eq!(notifications[1].get(), 0, "the ordinary panel has no seat");

        snapshot.error = Some("Unrelated browser section unavailable".into());
        publish(vcx, snapshot);
        assert_eq!(
            notifications[0].get(),
            1,
            "browser-only changes do not repaint the HUD"
        );
        assert_eq!(notifications[1].get(), 0);
    }

    #[gpui::test]
    fn hud_opens_exact_seat_and_reuses_openrig(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let id = "session_fox_1700000000000_0123456789abcdef";
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let feed = crate::openrig_feed::get(cx);
            feed.update(cx, |feed, cx| {
                feed.snapshot = Some(jcode_desktop_openrig::Snapshot::fixture());
                cx.notify();
            });
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel(id, cx);
            workspace.apply(
                harness::Update::OpenrigHostIdentity {
                    session_id: id.into(),
                    self_host_id: Some("host-fixture-local".into()),
                },
                cx,
            );
            workspace.slots[0].panel.update_chat(cx, |panel, cx| {
                panel.input.update(cx, |input, cx| {
                    input.set_content("keep this draft".into(), cx)
                });
            });
            workspace
        });
        vcx.run_until_parked();
        let icon = vcx
            .debug_bounds("panel-openrig:session_fox_1700000000000_0123456789abcdef")
            .expect("seat HUD");
        vcx.simulate_mouse_move(icon.center(), None, Default::default());
        vcx.run_until_parked();
        vcx.executor().advance_clock(Duration::from_secs(1));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-openrig-tooltip").is_some());
        let click_hud = |vcx: &mut gpui::VisualTestContext, selector: &'static str| {
            let bounds = vcx.debug_bounds(selector).expect("seat HUD");
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        click_hud(
            vcx,
            "panel-openrig:session_fox_1700000000000_0123456789abcdef",
        );
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(
                w.slots[0].panel.read(cx).input.read(cx).content.as_ref(),
                "keep this draft"
            );
            assert_eq!(w.slots.len(), 2);
            assert_eq!(
                w.slots[w.active].panel.read(cx).inspected_fleet_key(),
                Some("seat:\"host-fixture-local\"/\"local-kernel\"/\"local-operator.agent\"")
            );
        });
        workspace.update_in(vcx, |w, window, cx| {
            w.push_test_panel(&format!("ssh://worker-east.example.test/{id}"), cx);
            let index = w.slots.len() - 1;
            w.apply(
                harness::Update::OpenrigHostIdentity {
                    session_id: format!("ssh://worker-east.example.test/{id}"),
                    self_host_id: Some("host-fixture-remote".into()),
                },
                cx,
            );
            w.set_active(index, cx);
            w.focus_active(window, cx);
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        click_hud(
            vcx,
            "panel-openrig:ssh://worker-east.example.test/session_fox_1700000000000_0123456789abcdef",
        );
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots.len(), 3);
            assert_eq!(
                w.slots[w.active].panel.read(cx).inspected_fleet_key(),
                Some("seat:\"worker-east\"/\"remote-kernel\"/\"remote-operator.agent\"")
            );
        });
        workspace.update_in(vcx, |w, window, cx| {
            w.apply(
                harness::Update::OpenrigHostIdentity {
                    session_id: format!("ssh://worker-east.example.test/{id}"),
                    self_host_id: None,
                },
                cx,
            );
            w.set_active(2, cx);
            w.focus_active(window, cx);
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("panel-openrig:ssh://worker-east.example.test/session_fox_1700000000000_0123456789abcdef").is_none());
    }

    #[gpui::test]
    fn hud_reuses_openrig_in_another_window(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let id = "session_fox_1700000000000_0123456789abcdef";
        let (source, vcx) = cx.add_window_view(|_, cx| {
            let feed = crate::openrig_feed::get(cx);
            feed.update(cx, |feed, cx| {
                feed.snapshot = Some(jcode_desktop_openrig::Snapshot::fixture());
                cx.notify();
            });
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.push_test_panel(id, cx);
            workspace.slots[0].panel.update_chat(cx, |panel, cx| {
                panel.set_rig_host_id(Some("host-fixture-local".into()), cx);
            });
            workspace
        });
        let target = vcx.update(|_, cx| {
            let handle = cx
                .open_window(gpui::WindowOptions::default(), |_, cx| {
                    cx.new(|cx| {
                        let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
                        workspace.push_test_panel("other-chat", cx);
                        workspace
                    })
                })
                .unwrap();
            handle
                .update(cx, |workspace, window, cx| {
                    workspace.open_openrig(&OpenOpenrig, window, cx)
                })
                .unwrap();
            handle
        });
        vcx.run_until_parked();
        let bounds = vcx
            .debug_bounds("panel-openrig:session_fox_1700000000000_0123456789abcdef")
            .expect("seat HUD");
        vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        source.read_with(vcx, |workspace, _| assert_eq!(workspace.slots.len(), 1));
        vcx.update(|_, cx| {
            let workspace = target.read(cx).unwrap();
            assert_eq!(workspace.slots.len(), 2);
            assert_eq!(
                workspace.slots[workspace.active]
                    .panel
                    .read(cx)
                    .inspected_fleet_key(),
                Some("seat:\"host-fixture-local\"/\"local-kernel\"/\"local-operator.agent\"")
            );
        });
    }

    #[gpui::test]
    fn chat_lists_each_host_and_opens_or_focuses_the_exact_seat_session(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("current-chat", cx);
            workspace
        });
        let click = |vcx: &mut gpui::VisualTestContext, selector| {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"));
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        let open_chat = |vcx: &mut gpui::VisualTestContext| {
            workspace.update_in(vcx, |workspace, window, cx| {
                workspace.open_openrig(&OpenOpenrig, window, cx)
            });
            vcx.executor().advance_clock(Duration::from_secs(2));
            vcx.run_until_parked();
            if vcx.debug_bounds("openrig-section:Chat").is_none() {
                click(vcx, "openrig-section-menu");
            }
            click(vcx, "openrig-section:Chat");
        };
        open_chat(vcx);
        for selector in [
            "openrig-chat:host-fixture-local:Advisor",
            "openrig-chat:host-fixture-local:Operator",
            "openrig-chat:worker-east:Advisor",
            "openrig-chat:worker-east:Operator",
        ] {
            assert!(vcx.debug_bounds(selector).is_some(), "missing {selector}");
        }
        let native = "session_fox_1700000000000_0123456789abcdef";
        let local_button = "openrig-chat:host-fixture-local:Operator:open";
        click(vcx, local_button);
        let panel = workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots[w.active].panel.session_id(cx), native);
            w.slots[w.active].panel.entity_id()
        });
        open_chat(vcx);
        click(vcx, local_button);
        workspace.read_with(vcx, |w, _| {
            assert_eq!(w.slots.len(), 3);
            assert_eq!(w.slots[w.active].panel.entity_id(), panel);
        });
        open_chat(vcx);
        click(vcx, "openrig-chat:worker-east:Operator:open");
        let remote = format!("ssh://worker-east.example.test/{native}");
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots.len(), 4);
            assert_eq!(w.slots[w.active].panel.session_id(cx), remote);
        });
        let commands: Vec<_> = commands.try_iter().collect();
        for session in [native, &remote] {
            assert!(commands.iter().any(
                |command| matches!(command, Command::Watch { session_id } if session_id == session)
            ));
        }
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, Command::CreateSession { .. }))
        );
    }

    #[gpui::test]
    fn inbox_and_attention_confirm_cancel_and_render_failure(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("current-chat", cx);
            workspace
        });
        workspace.update_in(vcx, |workspace, window, cx| {
            workspace.open_openrig(&OpenOpenrig, window, cx)
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        let click = |vcx: &mut gpui::VisualTestContext, selector| {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"));
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        if vcx.debug_bounds("openrig-section-menu").is_some() {
            click(vcx, "openrig-section-menu");
        }
        click(vcx, "openrig-section:Inbox");
        let local = "inbox-item:Queue { host: \"local\", item: \"q1\" }";
        let remote = "inbox-item:Queue { host: \"worker-east\", item: \"q1\" }";
        assert!(vcx.debug_bounds(remote).is_some());
        click(vcx, "inbox-filter:Approval");
        assert!(vcx.debug_bounds(remote).is_none());
        click(vcx, local);
        assert!(vcx.debug_bounds("inbox-inspector").is_some());
        click(vcx, "openrig-attention-action:0");
        assert!(vcx.debug_bounds("openrig-attention-confirmation").is_some());
        assert!(vcx.debug_bounds("openrig-attention-action-error").is_none());
        click(vcx, "openrig-attention-cancel");
        assert!(vcx.debug_bounds("openrig-attention-confirmation").is_none());
        assert!(vcx.debug_bounds("openrig-attention-action-error").is_none());
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inbox-inspector").is_none());
        if vcx.debug_bounds("openrig-section-menu").is_some() {
            click(vcx, "openrig-section-menu");
        }
        click(vcx, "openrig-section:Needs attention");
        click(vcx, local);
        click(vcx, "openrig-attention-action:1");
        assert!(vcx.debug_bounds("openrig-attention-confirmation").is_some());
        click(vcx, "openrig-attention-confirm");
        assert!(vcx.debug_bounds("openrig-attention-confirmation").is_none());
        assert!(vcx.debug_bounds("openrig-attention-action-error").is_some());
        assert!(vcx.debug_bounds("inbox-inspector").is_some());
        assert!(!commands.try_iter().any(|command| matches!(
            command,
            Command::CreateSession { .. } | Command::Watch { .. }
        )));
    }

    #[gpui::test]
    fn work_filters_switches_to_workflows_and_opens_the_owner_session(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("current-chat", cx);
            workspace
        });
        workspace.update_in(vcx, |workspace, window, cx| {
            workspace.open_openrig(&OpenOpenrig, window, cx)
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        let click = |vcx: &mut gpui::VisualTestContext, selector| {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"));
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        if vcx.debug_bounds("openrig-section-menu").is_some() {
            click(vcx, "openrig-section-menu");
        }
        click(vcx, "openrig-section:Work");
        click(vcx, "work-filter:Owner");
        click(vcx, "work-option:operator@kernel");
        assert!(
            vcx.debug_bounds("work-row:local:qitem-example-done")
                .is_none()
        );
        assert!(
            vcx.debug_bounds("work-row:local:qitem-example-held")
                .is_some()
        );
        click(vcx, "work-filter:State");
        click(vcx, "work-option:in-progress");
        assert!(
            vcx.debug_bounds("work-row:local:qitem-example-held")
                .is_none()
        );
        click(vcx, "work-row:local:qitem-example-open");
        assert!(vcx.debug_bounds("work-inspector").is_some());
        click(vcx, "work-back");
        assert!(vcx.debug_bounds("work-inspector").is_none());
        click(vcx, "work-tab:Workflows");
        click(vcx, "work-row:local:workflow-example");
        assert!(vcx.debug_bounds("work-inspector").is_some());
        click(vcx, "work-open-session");
        workspace.read_with(vcx, |workspace, cx| {
            assert_eq!(
                workspace.slots[workspace.active].panel.session_id(cx),
                "session_fox_1700000000000_0123456789abcdef"
            );
        });
        assert!(
            !commands
                .try_iter()
                .any(|command| matches!(command, Command::CreateSession { .. }))
        );
    }

    #[gpui::test]
    fn build_category_filter_and_preview_are_usable_at_split_width(cx: &mut gpui::TestAppContext) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut workspace = Workspace::for_test(learning::Coach::new(), cx);
            workspace.set_test_bridge(bridge);
            workspace.push_test_panel("current-chat", cx);
            workspace
        });
        workspace.update_in(vcx, |workspace, window, cx| {
            workspace.open_openrig(&OpenOpenrig, window, cx)
        });
        vcx.executor().advance_clock(Duration::from_secs(2));
        vcx.run_until_parked();
        let click = |vcx: &mut gpui::VisualTestContext, selector| {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"));
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        if vcx.debug_bounds("openrig-section-menu").is_some() {
            click(vcx, "openrig-section-menu");
        }
        click(vcx, "openrig-section:Build");
        assert!(
            vcx.debug_bounds("build-row:local:0a25241dadbcaf2d")
                .is_some()
        );
        click(vcx, "build-category:Workflow specs");
        assert!(
            vcx.debug_bounds("build-row:local:0a25241dadbcaf2d")
                .is_none()
        );
        click(vcx, "build-row:local:workflow:basic-loop:1");
        assert!(vcx.debug_bounds("build-inspector").is_some());
        click(vcx, "build-back");
        assert!(vcx.debug_bounds("build-inspector").is_none());
        assert!(
            vcx.debug_bounds("build-row:local:workflow:basic-loop:1")
                .is_some()
        );
        assert!(!commands.try_iter().any(|command| matches!(
            command,
            Command::CreateSession { .. } | Command::Watch { .. }
        )));
    }

    #[gpui::test]
    fn fleet_row_inspects_then_opens_local_and_remote_panels_without_creating_a_session(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::bind_workspace_keys);
        let (bridge, commands) = harness::spawn_recording();
        let (workspace, vcx) = cx.add_window_view(|_, cx| {
            let mut w = Workspace::for_test(learning::Coach::new(), cx);
            w.set_test_bridge(bridge);
            w.push_test_panel("current-chat", cx);
            w
        });
        let open = |workspace: &Entity<Workspace>, vcx: &mut gpui::VisualTestContext| {
            workspace.update_in(vcx, |workspace, window, cx| {
                workspace.open_openrig(&OpenOpenrig, window, cx)
            });
            vcx.executor().advance_clock(Duration::from_secs(2));
            vcx.run_until_parked();
        };
        open(&workspace, vcx);
        let click = |vcx: &mut gpui::VisualTestContext, selector| {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing selector {selector}"));
            vcx.simulate_click(bounds.center(), gpui::Modifiers::default());
            vcx.run_until_parked();
        };
        for seat in [
            "seat:\"host-fixture-local\"/\"local-kernel\"/\"local-operator.human\"",
            "seat:\"host-fixture-local\"/\"local-kernel\"/\"local-advisor.lead\"",
        ] {
            click(vcx, seat);
            assert!(vcx.debug_bounds("fleet-properties").is_some());
            assert!(vcx.debug_bounds("fleet-open-session").is_none());
            vcx.simulate_keystrokes("enter escape");
            vcx.run_until_parked();
            workspace.read_with(vcx, |w, _| assert_eq!(w.slots.len(), 2));
        }
        let local = "session_fox_1700000000000_0123456789abcdef";
        let linked = "seat:\"host-fixture-local\"/\"local-kernel\"/\"local-operator.agent\"";
        click(vcx, linked);
        workspace.read_with(vcx, |w, _| assert_eq!(w.slots.len(), 2));
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        let entity = workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots[w.active].panel.session_id(cx), local);
            w.slots[w.active].panel.entity_id()
        });
        open(&workspace, vcx);
        click(vcx, "fleet-open-session");
        workspace.read_with(vcx, |w, _| {
            assert_eq!(w.slots.len(), 3);
            assert_eq!(w.slots[w.active].panel.entity_id(), entity);
        });
        open(&workspace, vcx);
        if vcx.debug_bounds("fleet-back").is_some() {
            click(vcx, "fleet-back");
        }
        click(vcx, "host:\"host-fixture-local\"");
        click(
            vcx,
            "seat:\"worker-east\"/\"remote-kernel\"/\"remote-operator.agent\"",
        );
        click(vcx, "fleet-open-session");
        let remote = format!("ssh://worker-east.example.test/{local}");
        workspace.read_with(vcx, |w, cx| {
            assert_eq!(w.slots[w.active].panel.session_id(cx), remote);
            assert_eq!(w.slots.len(), 4);
        });
        let commands: Vec<_> = commands.try_iter().collect();
        assert!(commands.iter().any(
            |command| matches!(command, Command::Watch { session_id } if session_id == local)
        ));
        assert!(commands.iter().any(
            |command| matches!(command, Command::Watch { session_id } if session_id == &remote)
        ));
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, Command::CreateSession { .. }))
        );
    }
}
