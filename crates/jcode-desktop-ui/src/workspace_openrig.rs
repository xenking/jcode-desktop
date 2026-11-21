use super::*;
use crate::ui_actions::OpenFleetSession;
use jcode_desktop_openrig::fleet::HostTarget;

fn session_address(target: &HostTarget, native_id: &str, known_hosts: &[String]) -> Option<String> {
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
    let host = known_hosts
        .iter()
        .find(|host| *host == alias)
        .or_else(|| {
            known_hosts.iter().find(|host| {
                let hostname = host.rsplit('@').next().unwrap_or(host);
                endpoint.as_deref() == Some(hostname) || hostname == alias
            })
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
        let Some(address) = session_address(&action.target, &action.native_id, &self.remotes.hosts)
        else {
            return;
        };
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
            session_address(&remote, "session_a", &["alice@east".into()]).as_deref(),
            Some("ssh://alice%40east/session_a")
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
