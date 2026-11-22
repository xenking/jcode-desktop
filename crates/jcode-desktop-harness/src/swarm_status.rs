//! Ordered live swarm facts, shared by the sidebar and composer through the
//! workspace session catalog. Polling durable metadata cannot roll these back.

use jcode_sdk::{SessionInfo, SwarmMemberInfo};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct LiveSwarmStatus {
    snapshots: HashMap<String, (u64, HashSet<String>)>,
    members: HashMap<String, (u64, SwarmMemberInfo)>,
}

impl LiveSwarmStatus {
    /// Apply the complete snapshot in daemon publication order. A member
    /// missing from a newer snapshot was removed, not left running forever.
    pub fn apply(&mut self, source: &str, updated_at: u64, members: &[SwarmMemberInfo]) -> bool {
        if self
            .snapshots
            .get(source)
            .is_some_and(|(seen, _)| *seen >= updated_at)
        {
            return false;
        }
        let ids = members
            .iter()
            .map(|member| member.session_id.clone())
            .collect::<HashSet<_>>();
        let mut changed = false;
        if let Some((_, previous)) = self
            .snapshots
            .insert(source.to_owned(), (updated_at, ids.clone()))
        {
            for id in previous.difference(&ids) {
                if let Some((seen, member)) = self.members.get_mut(id)
                    && *seen < updated_at
                {
                    *seen = updated_at;
                    if member.status != "stopped" {
                        member.status = "stopped".into();
                        changed = true;
                    }
                }
            }
        }
        for member in members {
            if let Some((seen, previous)) = self.members.get_mut(&member.session_id) {
                if *seen < updated_at {
                    *seen = updated_at;
                    if previous != member {
                        previous.clone_from(member);
                        changed = true;
                    }
                }
            } else {
                self.members
                    .insert(member.session_id.clone(), (updated_at, member.clone()));
                changed = true;
            }
        }
        changed
    }

    /// Overlay after each poll as well as each live event. Both UI projections
    /// then consume this one reconciled catalog, never a separate status cache.
    pub fn merge(&self, sessions: &mut [SessionInfo]) {
        for session in sessions {
            let Some((_, member)) = self.members.get(&session.session_id) else {
                continue;
            };
            session.swarm_status = Some(member.status.clone());
            session
                .parent_session_id
                .clone_from(&member.parent_session_id);
            if member.agent_label.is_some() {
                session.agent_label.clone_from(&member.agent_label);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(status: &str) -> SwarmMemberInfo {
        SwarmMemberInfo {
            session_id: "worker".into(),
            status: status.into(),
            parent_session_id: Some("lead".into()),
            agent_label: Some("task".into()),
        }
    }

    fn catalog() -> Vec<SessionInfo> {
        serde_json::from_str(
            r#"[{"session_id":"worker","status":"Active","swarm_status":"ready"}]"#,
        )
        .unwrap()
    }

    #[test]
    fn rapid_status_updates_win_over_older_events_duplicate_connections_and_slow_polls() {
        let mut live = LiveSwarmStatus::default();
        assert!(live.apply("lead", 1_000, &[member("ready")]));
        assert!(live.apply("lead", 1_065, &[member("running")]));
        assert!(!live.apply("lead", 1_000, &[member("ready")]));
        live.apply("other-attachment", 1_000, &[member("ready")]);
        let mut sessions = catalog();
        live.merge(&mut sessions);
        assert_eq!(sessions[0].swarm_status.as_deref(), Some("running"));
        live.apply("lead", 1_100, &[member("ready")]);
        live.merge(&mut sessions);
        assert_eq!(sessions[0].swarm_status.as_deref(), Some("ready"));
    }

    #[test]
    fn newer_duplicate_advances_the_watermark_without_a_visible_change() {
        let mut live = LiveSwarmStatus::default();
        assert!(live.apply("lead", 10, &[member("running")]));
        assert!(!live.apply("lead", 30, &[member("running")]));
        assert!(!live.apply("other", 20, &[member("ready")]));
        assert!(live.apply("lead", 40, &[member("ready")]));
    }

    #[test]
    fn removed_worker_cannot_be_resurrected_by_a_stale_catalog_or_snapshot() {
        let mut live = LiveSwarmStatus::default();
        live.apply("lead", 100, &[member("running")]);
        live.apply("lead", 200, &[]);
        live.apply("lead", 150, &[member("running")]);
        let mut sessions = catalog();
        live.merge(&mut sessions);
        assert_eq!(sessions[0].swarm_status.as_deref(), Some("stopped"));
    }

    #[test]
    fn live_event_before_catalog_is_retained() {
        let mut live = LiveSwarmStatus::default();
        live.apply("lead", 100, &[member("running")]);
        live.merge(&mut []);
        let mut sessions = catalog();
        live.merge(&mut sessions);
        assert_eq!(sessions[0].swarm_status.as_deref(), Some("running"));
    }
}
