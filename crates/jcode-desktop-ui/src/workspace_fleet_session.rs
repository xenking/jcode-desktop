//! Opening the exact Jcode session a fleet seat runs, on this machine or over SSH.
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
    if target.is_local_machine() {
        return Some(native_id.into());
    }
    let alias = target.alias();
    let endpoint = target.endpoint();
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
                .find(|host| host.rsplit('@').next() == endpoint)
        })
        .map(String::as_str)
        .or(endpoint)?;
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
        let origin = self
            .slots
            .get(self.active)
            .and_then(|slot| slot.panel.chat())
            .map(Entity::downgrade);
        if !cfg!(test) && !action.target.is_local_machine() {
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
                } else if let Some(origin) = origin {
                    let _ = origin.update(cx, |panel, cx| panel.show_chat_open_error(cx));
                }
            })
            .detach();
        } else if let Some(address) =
            session_address(&action.target, &action.native_id, &self.remotes.hosts)
        {
            self.activate_fleet_address(address, window, cx);
        } else if let Some(origin) = origin {
            let _ = origin.update(cx, |panel, cx| panel.show_chat_open_error(cx));
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
        let remote = HostTarget::primary("https://east.example.test", "east");
        let local = HostTarget::primary("http://127.0.0.1:7433", "local");
        assert_eq!(
            session_address(&local, "session_a", &[]).as_deref(),
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
            session_address(&local, "ssh://elsewhere/session_a", &[]),
            None
        );
        assert_eq!(
            session_address(&HostTarget::primary("", "east"), "session_a", &[]),
            None
        );
    }
}
