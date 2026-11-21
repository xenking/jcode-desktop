use super::*;
use crate::ui_actions::OpenFleetSession;
use jcode_desktop_openrig::fleet::HostTarget;

fn session_address(target: &HostTarget, native_id: &str, ssh_hosts: &[String]) -> Option<String> {
    if native_id.is_empty()
        || native_id.len() > 200
        || !native_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return None;
    }
    let HostTarget::Remote { alias, endpoint } = target else {
        return Some(native_id.into());
    };
    // Prefer the operator's configured alias (including an SSH username) over
    // a DNS name. An OpenRig ID by itself is not necessarily an SSH destination.
    let host = ssh_hosts
        .iter()
        .find(|host| *host == alias)
        .or_else(|| {
            ssh_hosts
                .iter()
                .find(|host| host.rsplit('@').next() == Some(alias))
        })
        .or_else(|| {
            ssh_hosts
                .iter()
                .find(|host| host.rsplit('@').next() == endpoint.as_deref())
        })
        .map(String::as_str)
        .or(endpoint.as_deref())?;
    if remotes::managed_destination(host) {
        return None;
    }
    let host = crate::remote_targets::validate_host(host).ok()?;
    let mut address = String::from("ssh://");
    for byte in host.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            address.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(address, "%{byte:02X}");
        }
    }
    address.push('/');
    address.push_str(native_id);
    (harness::remote_host(&address).as_deref() == Some(&host)).then_some(address)
}

impl Workspace {
    pub(super) fn open_fleet_session(
        &mut self,
        action: &OpenFleetSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !cfg!(test) && matches!(&action.target, HostTarget::Remote { .. }) {
            let target = action.target.clone();
            let native_id = action.native_id.clone();
            let mut ssh_hosts = self.remotes.hosts.clone();
            cx.spawn_in(window, async move |this, cx| {
                let discovered = cx
                    .background_executor()
                    .spawn(async { crate::remote_targets::discover_hosts() })
                    .await;
                for host in discovered {
                    if !ssh_hosts
                        .iter()
                        .any(|known| known.rsplit('@').next() == Some(host.as_str()))
                    {
                        ssh_hosts.push(host);
                    }
                }
                if let Some(address) = session_address(&target, &native_id, &ssh_hosts) {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.activate_fleet_address(address, window, cx)
                    });
                }
            })
            .detach();
        } else if let Some(address) =
            session_address(&action.target, &action.native_id, &self.remotes.hosts)
        {
            self.activate_fleet_address(address, window, cx);
        }
    }

    fn activate_fleet_address(
        &mut self,
        address: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = self
            .sessions
            .iter()
            .find(|session| session.session_id == address)
            .cloned()
            .unwrap_or_else(|| jcode_sdk::SessionInfo {
                session_id: address,
                title: None,
                working_dir: None,
                status: String::new(),
                user_connected: false,
                transcript_bytes: None,
                saved: false,
                save_label: None,
                updated_at_ms: None,
                last_active_at_ms: None,
                archived: false,
                archived_at_ms: None,
                parent_session_id: None,
                agent_label: None,
                swarm_status: None,
                edit_stats: None,
                context: None,
            });
        // activate_session only watches/attaches the exact ID; it never creates a session.
        self.activate_session(session, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_mapping_preserves_the_machine_and_prefers_configured_ssh_targets() {
        let remote = HostTarget::Remote {
            alias: "east".into(),
            endpoint: Some("east.example.test".into()),
        };
        assert_eq!(
            session_address(&HostTarget::Local, "session_a", &[]).as_deref(),
            Some("session_a")
        );
        assert_eq!(
            session_address(&remote, "session_a", &[]).as_deref(),
            Some("ssh://east.example.test/session_a")
        );
        assert_eq!(
            session_address(
                &remote,
                "session_a",
                &["bob@east.example.test".into(), "alice@east".into()]
            )
            .as_deref(),
            Some("ssh://alice%40east/session_a")
        );
        assert_eq!(
            session_address(
                &remote,
                "session_a",
                &["bob@east.example.test".into(), "east".into()]
            )
            .as_deref(),
            Some("ssh://east/session_a")
        );
        assert_eq!(session_address(&remote, "", &[]), None);
        assert_eq!(
            session_address(&HostTarget::Local, "ssh://elsewhere/session_a", &[]),
            None
        );
        assert_eq!(
            session_address(
                &HostTarget::Remote {
                    alias: "east".into(),
                    endpoint: None
                },
                "session_a",
                &[]
            ),
            None
        );
    }

    #[gpui::test]
    fn inbox_filters_and_opens_a_read_only_inspector(cx: &mut gpui::TestAppContext) {
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
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("inbox-inspector").is_none());
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
                workspace.slots[workspace.active].panel.read(cx).session_id,
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
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, local);
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
            assert_eq!(w.slots[w.active].panel.read(cx).session_id, remote);
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
