use serde::Deserialize;

use crate::{encode, get_target, host::HostRegistry, inbox};

pub use crate::HostTarget;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fleet {
    pub hosts: Vec<Host>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub id: String,
    pub self_host_id: Option<String>,
    pub name: String,
    pub target: HostTarget,
    pub status: Option<String>,
    pub rigs: Option<Vec<Rig>>,
    pub error: Option<String>,
}

impl Host {
    pub(crate) fn queue_host_id(&self) -> &str {
        if self.target.is_primary() {
            "local"
        } else {
            &self.id
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rig {
    pub id: String,
    pub name: String,
    #[serde(rename = "lifecycleState")]
    pub status: Option<String>,
    #[serde(skip)]
    pub pods: Option<Vec<Pod>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pod {
    pub id: Option<String>,
    pub name: Option<String>,
    pub seats: Vec<Seat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runtime {
    Jcode,
    Other(String),
}

impl Runtime {
    pub fn label(&self) -> &str {
        match self {
            Self::Jcode => "Jcode",
            Self::Other(label) => label,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Seat {
    pub id: String,
    pub logical_id: String,
    pub canonical_session_name: Option<String>,
    pub runtime: Option<Runtime>,
    pub status: Option<String>,
    pub session_status: Option<String>,
    /// A non-stale activity reported by OpenRig.
    pub activity: Option<String>,
    pub(crate) last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
    pub jcode_session_id: Option<String>,
    pub context_percent: Option<f64>,
    pub current_work_count: Option<u64>,
    pub(crate) attention: Option<SeatAttention>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeatAttention {
    pub kind: inbox::Kind,
    pub reason: Option<String>,
    pub evidence: Option<String>,
    pub observed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub clear: Option<inbox::ClearEvidence>,
    pub class: inbox::SeatReasonClass,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    node_id: String,
    logical_id: String,
    canonical_session_name: Option<String>,
    pod_id: Option<String>,
    pod_namespace: Option<String>,
    runtime: Option<String>,
    lifecycle_state: Option<String>,
    session_status: Option<String>,
    native_ref: Option<String>,
    context_usage: Option<ContextUsage>,
    in_progress_work_count: Option<u64>,
    arbitrated_needs_input: Option<bool>,
    activity_state: Option<ActivityState>,
    agent_activity: Option<AgentActivity>,
    last_activity_at: Option<String>,
    needs_attention: Option<bool>,
    held_reason: Option<String>,
    startup_status: Option<String>,
    latest_error: Option<String>,
    latest_error_stage: Option<String>,
    restore_outcome: Option<String>,
    identity_verdict: Option<IdentityVerdict>,
}

#[derive(Deserialize)]
struct IdentityVerdict {
    verdict: Option<String>,
    reason: Option<String>,
    #[serde(rename = "observedAt")]
    observed_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityState {
    needs_input: Option<NeedsInput>,
}

#[derive(Deserialize)]
struct NeedsInput {
    count: Option<u64>,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentActivity {
    state: Option<String>,
    reason: Option<String>,
    evidence: Option<String>,
    event_at: Option<String>,
    stale: Option<bool>,
    evidence_source: Option<String>,
}

impl Node {
    fn attention(&self) -> Option<SeatAttention> {
        let observed = self
            .agent_activity
            .as_ref()
            .filter(|activity| activity.stale != Some(true));
        let arbitration = self
            .activity_state
            .as_ref()
            .and_then(|state| state.needs_input.as_ref());
        let needs_input = self
            .arbitrated_needs_input
            .or_else(|| arbitration.and_then(|signal| signal.count.map(|count| count > 0)))
            .unwrap_or_else(|| observed.and_then(|a| a.state.as_deref()) == Some("needs_input"));
        let held = self
            .held_reason
            .as_ref()
            .filter(|reason| !reason.trim().is_empty());
        let input_reason = arbitration
            .and_then(|signal| signal.reason.as_deref())
            .or_else(|| {
                observed
                    .filter(|a| a.state.as_deref() == Some("needs_input"))
                    .and_then(|a| a.reason.as_deref())
            });
        let startup = matches!(
            self.startup_status.as_deref(),
            Some("attention_required" | "failed")
        );
        let identity = self.identity_verdict.as_ref().filter(|verdict| {
            matches!(
                verdict.verdict.as_deref(),
                Some("pane_missing" | "mismatch")
            )
        });
        let restore = matches!(
            self.restore_outcome.as_deref(),
            Some("attention_required" | "failed" | "awaiting-decision")
        );
        let hook_wait = observed.is_some_and(|a| {
            a.evidence_source.as_deref() == Some("runtime_hook")
                && (a.state.as_deref() == Some("needs_input")
                    || self.runtime.as_deref() == Some("codex")
                        && a.reason.as_deref() == Some("permission_request_pending"))
        });
        use inbox::SeatReasonClass as Class;
        let (kind, reason, clear, class) = if startup {
            (
                inbox::Kind::Startup,
                self.latest_error
                    .as_deref()
                    .or(self.latest_error_stage.as_deref()),
                Some(inbox::ClearEvidence::Reconcile),
                Class::Startup,
            )
        } else if let Some(identity) = identity {
            (
                inbox::Kind::SeatAttention,
                identity.reason.as_deref().or(identity.verdict.as_deref()),
                Some(inbox::ClearEvidence::Reconcile),
                Class::Identity,
            )
        } else if restore {
            (
                inbox::Kind::SeatAttention,
                self.latest_error
                    .as_deref()
                    .or(self.restore_outcome.as_deref()),
                (self.restore_outcome.as_deref() == Some("attention_required")
                    || self.restore_outcome.as_deref() == Some("failed")
                        && self.session_status.as_deref() == Some("running"))
                .then_some(inbox::ClearEvidence::Reconcile),
                Class::Restore,
            )
        } else if needs_input || hook_wait {
            (
                if matches!(
                    input_reason,
                    Some("permission_prompt" | "permission_request_pending")
                ) {
                    inbox::Kind::Permission
                } else {
                    inbox::Kind::SeatAttention
                },
                input_reason,
                hook_wait.then_some(inbox::ClearEvidence::AttestPromptEnded),
                Class::Input,
            )
        } else if let Some(reason) = held.filter(|_| self.needs_attention != Some(false)) {
            (
                inbox::Kind::SeatAttention,
                Some(reason.as_str()),
                None,
                Class::LaunchHold,
            )
        } else if self.needs_attention == Some(true)
            || self.lifecycle_state.as_deref() == Some("attention_required")
        {
            (
                inbox::Kind::SeatAttention,
                self.latest_error.as_deref(),
                None,
                Class::Reported,
            )
        } else {
            return None;
        };
        let observed =
            observed.filter(|a| needs_input && a.state.as_deref() == Some("needs_input"));
        Some(SeatAttention {
            kind,
            reason: reason
                .filter(|reason| !reason.trim().is_empty())
                .map(str::to_owned),
            evidence: observed
                .and_then(|a| a.evidence.clone())
                .filter(|text| !text.trim().is_empty()),
            observed_at: inbox::timestamp(match class {
                Class::Input => observed.and_then(|a| a.event_at.as_deref()),
                Class::Identity => identity.and_then(|value| value.observed_at.as_deref()),
                _ => None,
            }),
            clear,
            class,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextUsage {
    availability: Option<String>,
    used_percentage: Option<f64>,
}

impl From<Node> for Seat {
    fn from(node: Node) -> Self {
        let attention = node.attention();
        let runtime = node.runtime.filter(|value| !value.is_empty()).map(|value| {
            if value == "jcode" {
                Runtime::Jcode
            } else {
                Runtime::Other(value)
            }
        });
        let jcode_session_id = node.native_ref.filter(|id| {
            runtime == Some(Runtime::Jcode)
                && !id.is_empty()
                && id.len() <= 200
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        });
        Self {
            id: node.node_id,
            logical_id: node.logical_id,
            runtime,
            status: node.lifecycle_state.or_else(|| node.session_status.clone()),
            session_status: node.session_status,
            activity: node
                .agent_activity
                .as_ref()
                .filter(|activity| activity.stale != Some(true))
                .and_then(|activity| activity.state.clone()),
            last_activity_at: [
                node.last_activity_at.as_deref(),
                node.agent_activity
                    .as_ref()
                    .and_then(|activity| activity.event_at.as_deref()),
            ]
            .into_iter()
            .filter_map(inbox::timestamp)
            .max(),
            jcode_session_id,
            context_percent: node
                .context_usage
                .filter(|context| context.availability.as_deref() != Some("unknown"))
                .and_then(|context| context.used_percentage)
                .filter(|percent| (0.0..=100.0).contains(percent)),
            current_work_count: node.in_progress_work_count,
            canonical_session_name: node
                .canonical_session_name
                .filter(|id| !id.trim().is_empty()),
            attention,
        }
    }
}

fn group_pods(nodes: Vec<Node>) -> Vec<Pod> {
    let mut pods: Vec<Pod> = Vec::new();
    for node in nodes {
        if let Some(pod) = pods.iter_mut().find(|pod| {
            pod.id == node.pod_id && (pod.id.is_some() || pod.name == node.pod_namespace)
        }) {
            pod.seats.push(node.into());
        } else {
            pods.push(Pod {
                id: node.pod_id.clone(),
                name: node.pod_namespace.clone(),
                seats: vec![node.into()],
            });
        }
    }
    pods
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Inventory<T> {
    Array(Vec<T>),
    Items { items: Vec<T> },
}

fn inventory<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<Vec<T>, String> {
    serde_json::from_value(value)
        .map(|inventory| match inventory {
            Inventory::Array(items) | Inventory::Items { items } => items,
        })
        .map_err(|error| error.to_string())
}

impl Fleet {
    /// Resolve only an unambiguous native Jcode session on the given host.
    pub fn jcode_seat<'a>(
        &'a self,
        self_host_id: &str,
        native_id: &str,
    ) -> Option<(&'a Host, &'a Rig, &'a Pod, &'a Seat)> {
        if native_id.is_empty() || self_host_id.is_empty() {
            return None;
        }
        let mut hosts = self
            .hosts
            .iter()
            .filter(|host| host.self_host_id.as_deref() == Some(self_host_id));
        let host = hosts.next()?;
        if hosts.next().is_some() {
            return None;
        }
        let mut matches = std::iter::once(host).flat_map(|host| {
            host.rigs.iter().flatten().flat_map(move |rig| {
                rig.pods.iter().flatten().flat_map(move |pod| {
                    pod.seats.iter().filter_map(move |seat| {
                        (seat.runtime == Some(Runtime::Jcode)
                            && seat.jcode_session_id.as_deref() == Some(native_id))
                        .then_some((host, rig, pod, seat))
                    })
                })
            })
        });
        let result = matches.next()?;
        matches.next().is_none().then_some(result)
    }

    pub(crate) fn fetch(
        base: &str,
        health: Option<&serde_json::Value>,
        hosts: Option<&serde_json::Value>,
        rigs: Option<&serde_json::Value>,
        errors: &mut Vec<String>,
    ) -> Self {
        Self::load(base, health, hosts, rigs, errors, get_target)
    }

    pub(crate) fn load(
        base: &str,
        health: Option<&serde_json::Value>,
        hosts: Option<&serde_json::Value>,
        rigs: Option<&serde_json::Value>,
        errors: &mut Vec<String>,
        get: impl Fn(&HostTarget, &str) -> Result<serde_json::Value, String>,
    ) -> Self {
        if health.is_none() && hosts.is_none() && rigs.is_none() {
            return Self::default();
        }
        let host_id = health
            .and_then(|value| value["selfHostId"].as_str())
            .filter(|id| !id.is_empty())
            .unwrap_or("local");
        let hosts =
            hosts.and_then(
                |value| match serde_json::from_value::<HostRegistry>(value.clone()) {
                    Ok(hosts) => Some(hosts),
                    Err(error) => {
                        errors.push(format!("Fleet hosts: {error}"));
                        None
                    }
                },
            );
        let mut local = Host {
            error: None,
            id: host_id.into(),
            self_host_id: health
                .and_then(|value| value["selfHostId"].as_str())
                .filter(|id| !id.is_empty())
                .map(str::to_owned),
            name: hosts
                .as_ref()
                .and_then(|hosts| hosts.own_name.clone())
                .unwrap_or_else(|| host_id.into()),
            target: HostTarget::primary(base, host_id),
            status: health
                .and_then(|value| value["status"].as_str())
                .map(str::to_owned),
            rigs: rigs.and_then(|value| record(inventory(value.clone()), "Fleet rigs", errors)),
        };
        populate_nodes(&mut local, &get, errors);
        let mut fleet = Self { hosts: vec![local] };
        for (target, status) in hosts.into_iter().flat_map(HostRegistry::into_targets) {
            let id = target.alias().to_owned();
            if fleet.hosts.iter().any(|host| host.id == id) {
                continue;
            }
            let identity_before = get(&target, "/healthz").ok().and_then(|value| {
                value["selfHostId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
            });
            let (rigs, error) = match get(&target, "/api/rigs/summary").and_then(inventory) {
                Ok(rigs) => (Some(rigs), None),
                Err(error) => {
                    errors.push(format!("Fleet rigs on {id}: {error}"));
                    (None, Some(error))
                }
            };
            let mut host = Host {
                name: id.clone(),
                target,
                self_host_id: None,
                status: if error.is_some() {
                    Some("Unavailable".into())
                } else {
                    status
                },
                rigs,
                id,
                error,
            };
            populate_nodes(&mut host, &get, errors);
            let identity_after = get(&host.target, "/healthz").ok().and_then(|value| {
                value["selfHostId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
            });
            if identity_before.is_some() && identity_before == identity_after {
                host.self_host_id = identity_before;
            }
            fleet.hosts.push(host);
        }
        fleet
    }

    pub fn fixture() -> Self {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/fleet-legacy.json"))
                .expect("fleet fixture");
        let mut errors = Vec::new();
        let mut fleet = Self::from_fixture(&data, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        // Synthetic new-daemon fields. The captured daemon predates nativeRef.
        for seat in fleet
            .hosts
            .iter_mut()
            .flat_map(|host| host.rigs.iter_mut().flatten())
            .flat_map(|rig| rig.pods.iter_mut().flatten())
            .flat_map(|pod| &mut pod.seats)
            .filter(|seat| seat.logical_id == "operator.agent")
        {
            seat.jcode_session_id = Some("session_fox_1700000000000_0123456789abcdef".into());
            seat.canonical_session_name = Some("operator@kernel".into());
            seat.context_percent = Some(41.);
            seat.current_work_count = Some(1);
        }
        fleet
    }

    fn from_fixture(data: &serde_json::Value, errors: &mut Vec<String>) -> Self {
        Self::load(
            "http://127.0.0.1:7433",
            Some(&data["health"]),
            Some(&data["hosts"]),
            Some(&data["rigs"]),
            errors,
            |target, route| match (target.is_primary(), route) {
                (true, "/api/rigs/local-kernel/nodes") => Ok(data["localNodes"].clone()),
                (true, "/api/rigs/local-verteam/nodes") => Ok(serde_json::json!([])),
                (false, "/api/rigs/summary") => Ok(data["remote"].clone()),
                (false, "/healthz") => Ok(serde_json::json!({"selfHostId":"host-fixture-remote"})),
                (false, "/api/rigs/remote-kernel/nodes") => Ok(data["remoteNodes"].clone()),
                _ => Err(format!("unexpected fixture route {route}")),
            },
        )
    }
}

fn record<T>(result: Result<T, String>, what: &str, errors: &mut Vec<String>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(format!("{what}: {error}"));
            None
        }
    }
}

fn populate_nodes(
    host: &mut Host,
    get: &impl Fn(&HostTarget, &str) -> Result<serde_json::Value, String>,
    errors: &mut Vec<String>,
) {
    for rig in host.rigs.as_mut().into_iter().flatten() {
        rig.pods = record(
            get(
                &host.target,
                &format!("/api/rigs/{}/nodes", encode(&rig.id)),
            )
            .and_then(inventory)
            .map(group_pods),
            &format!("Fleet seats on {}/{}", host.name, rig.name),
            errors,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_jcode_seat_requires_exact_host_and_unambiguous_session() {
        let mut fleet = Fleet::fixture();
        let local = fleet.hosts[0].self_host_id.clone().unwrap();
        let remote = fleet.hosts[1].self_host_id.clone().unwrap();
        let id = "session_fox_1700000000000_0123456789abcdef";
        assert_eq!(
            fleet.jcode_seat(&local, id).unwrap().3.logical_id,
            "operator.agent"
        );
        assert_eq!(
            fleet
                .jcode_seat(&remote, id)
                .unwrap()
                .0
                .self_host_id
                .as_deref(),
            Some(remote.as_str())
        );
        assert!(fleet.jcode_seat(&local, "ordinary-session").is_none());
        assert!(fleet.jcode_seat(&local, "").is_none());

        let seat = fleet.jcode_seat(&local, id).unwrap().3.clone();
        let pod = fleet.hosts[0].rigs.as_mut().unwrap()[0]
            .pods
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|pod| pod.seats.iter().any(|s| s.id == seat.id))
            .unwrap();
        pod.seats.push(seat);
        assert!(fleet.jcode_seat(&local, id).is_none());
    }
    use serde_json::json;

    #[test]
    fn inbox_seat_attention_uses_arbitration_and_never_guesses_an_age() {
        let attention = |activity| {
            serde_json::from_value::<Node>(activity)
                .unwrap()
                .attention()
        };
        let observed = json!({"nodeId":"n1", "logicalId":"dev.one", "agentActivity": {
            "state":"needs_input", "reason":"permission_prompt", "eventAt":"2026-11-21T01:00:00Z"
        }});
        let signal = attention(observed.clone()).unwrap();
        assert_eq!(signal.kind, inbox::Kind::Permission);
        assert!(signal.observed_at.is_some());
        let mut vetoed = observed.clone();
        vetoed["arbitratedNeedsInput"] = json!(false);
        assert_eq!(attention(vetoed), None);
        let mut stale = observed;
        stale["agentActivity"]["stale"] = json!(true);
        assert_eq!(attention(stale), None);
        let held = attention(
            json!({"nodeId":"n1", "logicalId":"dev.one", "heldReason":"waiting for owner"}),
        )
        .unwrap();
        assert_eq!(held.kind, inbox::Kind::SeatAttention);
        assert_eq!(held.observed_at, None);
    }

    #[test]
    fn captured_legacy_daemon_groups_local_and_remote_seats_without_inventing_sessions() {
        let data =
            serde_json::from_str(include_str!("../tests/fixtures/fleet-legacy.json")).unwrap();
        let mut errors = Vec::new();
        let fleet = Fleet::from_fixture(&data, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(fleet.hosts[0].id, "host-fixture-local");
        assert!(fleet.hosts[0].target.is_local_machine());
        assert_eq!(fleet.hosts[1].id, "worker-east");
        assert_eq!(
            fleet.hosts[1].target.endpoint(),
            Some("worker-east.example.test")
        );
        for (index, prefix) in [(0, "local"), (1, "remote")] {
            let rig = &fleet.hosts[index].rigs.as_ref().unwrap()[0];
            assert_eq!(rig.id, format!("{prefix}-kernel"));
            let pods = rig.pods.as_ref().unwrap();
            let pod = pods
                .iter()
                .find(|pod| pod.name.as_deref() == Some("operator"))
                .unwrap();
            assert_eq!(
                pod.id.as_deref(),
                Some(format!("{prefix}-operator").as_str())
            );
            assert_eq!(pod.seats.len(), 2);
            let seat = pod
                .seats
                .iter()
                .find(|seat| seat.runtime == Some(Runtime::Jcode))
                .unwrap();
            assert_eq!(seat.id, format!("{prefix}-operator.agent"));
            assert_eq!(seat.logical_id, "operator.agent");
            assert_eq!(seat.jcode_session_id, None);
            assert_eq!(seat.context_percent, None);
            assert_eq!(seat.current_work_count, Some(0));
        }
    }

    #[test]
    fn only_jcode_native_refs_are_routes_and_only_measured_zero_is_zero() {
        let seat = |runtime: &str, ref_id: &str, availability: &str| {
            Seat::from(
                serde_json::from_value::<Node>(json!({
                    "nodeId": "n1", "logicalId": "dev.one", "runtime": runtime,
                    "nativeRef": ref_id, "contextUsage": { "availability": availability, "usedPercentage": 0 },
                }))
                .unwrap(),
            )
        };
        let linked = seat("jcode", "session_one", "known");
        assert_eq!(linked.jcode_session_id.as_deref(), Some("session_one"));
        assert_eq!(linked.context_percent, Some(0.));
        assert_eq!(linked.current_work_count, None);
        assert_eq!(
            seat("terminal", "session_one", "unknown").jcode_session_id,
            None
        );
        assert_eq!(
            seat("jcode", "session_one", "unknown").context_percent,
            None
        );
        assert_eq!(
            seat("jcode", "ssh://foreign/session_one", "known").jcode_session_id,
            None
        );
    }

    /// Contract check against a running daemon:
    /// `OPENRIG_LIVE_BASE=http://127.0.0.1:7433 cargo test -p jcode-desktop-openrig live_daemon -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a running OpenRig daemon"]
    fn live_daemon_links_jcode_seats_to_sessions() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("set OPENRIG_LIVE_BASE");
        let fetch = |route: &str| crate::get(&base, route).ok();
        let (health, hosts, rigs) = (
            fetch("/healthz"),
            fetch("/api/hosts"),
            fetch("/api/rigs/summary"),
        );
        let mut errors = Vec::new();
        let fleet = Fleet::fetch(
            &base,
            health.as_ref(),
            hosts.as_ref(),
            rigs.as_ref(),
            &mut errors,
        );
        let mut linked = 0;
        for host in &fleet.hosts {
            for rig in host.rigs.iter().flatten() {
                for seat in rig.pods.iter().flatten().flat_map(|pod| &pod.seats) {
                    println!(
                        "{} {:?} {} {} {:?} session={:?}",
                        host.name,
                        host.target,
                        rig.name,
                        seat.logical_id,
                        seat.status,
                        seat.jcode_session_id
                    );
                    linked += usize::from(seat.jcode_session_id.is_some());
                }
            }
        }
        println!("errors={errors:?}");
        assert!(linked > 0, "no jcode seat reported a native session");
    }

    #[test]
    fn an_unavailable_inventory_is_not_an_empty_inventory() {
        let health = json!({"selfHostId":"remote-daemon"});
        let rigs = json!([{ "id": "rig", "name": "Rig" }]);
        let mut errors = Vec::new();
        let fleet = Fleet::load(
            "https://rig.example.test",
            Some(&health),
            None,
            Some(&rigs),
            &mut errors,
            |_, _| Err("offline".into()),
        );
        assert_eq!(fleet.hosts[0].target.endpoint(), Some("rig.example.test"));
        assert!(fleet.hosts[0].rigs.as_ref().unwrap()[0].pods.is_none());
        assert_eq!(errors, ["Fleet seats on remote-daemon/Rig: offline"]);
    }
}
