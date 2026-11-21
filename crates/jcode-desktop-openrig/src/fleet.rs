use serde::Deserialize;

use crate::{encode, get};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fleet {
    pub hosts: Vec<Host>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostTarget {
    Local,
    Remote {
        alias: String,
        endpoint: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub target: HostTarget,
    pub status: Option<String>,
    pub rigs: Option<Vec<Rig>>,
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
    pub runtime: Option<Runtime>,
    pub status: Option<String>,
    pub jcode_session_id: Option<String>,
    pub context_percent: Option<f64>,
    pub current_work_count: Option<u64>,
}

#[derive(Deserialize)]
struct HostList {
    #[serde(rename = "ownName")]
    own_name: Option<String>,
    hosts: Vec<HostEntry>,
}

#[derive(Deserialize)]
struct HostEntry {
    id: String,
    status: Option<String>,
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    node_id: String,
    logical_id: String,
    pod_id: Option<String>,
    pod_namespace: Option<String>,
    runtime: Option<String>,
    lifecycle_state: Option<String>,
    session_status: Option<String>,
    native_ref: Option<String>,
    context_usage: Option<ContextUsage>,
    in_progress_work_count: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextUsage {
    availability: Option<String>,
    used_percentage: Option<f64>,
}

impl From<Node> for Seat {
    fn from(node: Node) -> Self {
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
            status: node.lifecycle_state.or(node.session_status),
            jcode_session_id,
            context_percent: node
                .context_usage
                .filter(|context| context.availability.as_deref() != Some("unknown"))
                .and_then(|context| context.used_percentage)
                .filter(|percent| (0.0..=100.0).contains(percent)),
            current_work_count: node.in_progress_work_count,
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

fn endpoint_host(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.host_str().map(str::to_owned)
}

fn daemon_target(base: &str, host_id: &str) -> HostTarget {
    let endpoint = endpoint_host(base);
    if matches!(
        endpoint.as_deref(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ) {
        HostTarget::Local
    } else {
        HostTarget::Remote {
            alias: host_id.into(),
            endpoint,
        }
    }
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
    pub(crate) fn fetch(
        base: &str,
        health: Option<&serde_json::Value>,
        hosts: Option<&serde_json::Value>,
        rigs: Option<&serde_json::Value>,
        errors: &mut Vec<String>,
    ) -> Self {
        Self::load(base, health, hosts, rigs, errors, |route| get(base, route))
    }

    fn load(
        base: &str,
        health: Option<&serde_json::Value>,
        hosts: Option<&serde_json::Value>,
        rigs: Option<&serde_json::Value>,
        errors: &mut Vec<String>,
        get: impl Fn(&str) -> Result<serde_json::Value, String>,
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
                |value| match serde_json::from_value::<HostList>(value.clone()) {
                    Ok(hosts) => Some(hosts),
                    Err(error) => {
                        errors.push(format!("Fleet hosts: {error}"));
                        None
                    }
                },
            );
        let mut local = Host {
            id: host_id.into(),
            name: hosts
                .as_ref()
                .and_then(|hosts| hosts.own_name.clone())
                .unwrap_or_else(|| host_id.into()),
            target: daemon_target(base, host_id),
            status: health
                .and_then(|value| value["status"].as_str())
                .map(str::to_owned),
            rigs: rigs.and_then(|value| record(inventory(value.clone()), "Fleet rigs", errors)),
        };
        populate_nodes(&mut local, "", &get, errors);
        let mut fleet = Self { hosts: vec![local] };
        for entry in hosts.into_iter().flat_map(|hosts| hosts.hosts) {
            if entry.id.is_empty() || entry.id == "local" || entry.id == host_id {
                continue;
            }
            let query = format!("?host={}", encode(&entry.id));
            let mut host = Host {
                name: entry.id.clone(),
                target: HostTarget::Remote {
                    alias: entry.id.clone(),
                    endpoint: entry.url.as_deref().and_then(endpoint_host),
                },
                status: entry.status,
                rigs: record(
                    get(&format!("/api/rigs/summary{query}")).and_then(inventory),
                    &format!("Fleet rigs on {}", entry.id),
                    errors,
                ),
                id: entry.id,
            };
            populate_nodes(&mut host, &query, &get, errors);
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
            |route| match route {
                "/api/rigs/local-kernel/nodes" => Ok(data["localNodes"].clone()),
                "/api/rigs/local-verteam/nodes" => Ok(serde_json::json!([])),
                "/api/rigs/summary?host=worker-east" => Ok(data["remote"].clone()),
                "/api/rigs/remote-kernel/nodes?host=worker-east" => Ok(data["remoteNodes"].clone()),
                route => Err(format!("unexpected fixture route {route}")),
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
    query: &str,
    get: &impl Fn(&str) -> Result<serde_json::Value, String>,
    errors: &mut Vec<String>,
) {
    for rig in host.rigs.as_mut().into_iter().flatten() {
        rig.pods = record(
            get(&format!("/api/rigs/{}/nodes{query}", encode(&rig.id)))
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
    use serde_json::json;

    #[test]
    fn captured_legacy_daemon_groups_local_and_remote_seats_without_inventing_sessions() {
        let data =
            serde_json::from_str(include_str!("../tests/fixtures/fleet-legacy.json")).unwrap();
        let mut errors = Vec::new();
        let fleet = Fleet::from_fixture(&data, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(fleet.hosts[0].id, "host-fixture-local");
        assert_eq!(fleet.hosts[0].target, HostTarget::Local);
        assert_eq!(fleet.hosts[1].id, "worker-east");
        assert_eq!(
            fleet.hosts[1].target,
            HostTarget::Remote {
                alias: "worker-east".into(),
                endpoint: Some("worker-east.example.test".into())
            }
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
        let fetch = |route: &str| get(&base, route).ok();
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
            |_| Err("offline".into()),
        );
        assert_eq!(
            fleet.hosts[0].target,
            HostTarget::Remote {
                alias: "remote-daemon".into(),
                endpoint: Some("rig.example.test".into())
            }
        );
        assert!(fleet.hosts[0].rigs.as_ref().unwrap()[0].pods.is_none());
        assert_eq!(errors, ["Fleet seats on remote-daemon/Rig: offline"]);
    }
}
