use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionKind {
    Topology,
    Specs,
    Scopes,
    Terminals,
    Needs,
    System,
    Config,
    Connections,
    Usage,
    UsageHosts,
    Cost,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub depth: u8,
    pub label: String,
    pub status: Option<String>,
    pub detail: Vec<(String, String)>,
    pub children_count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub title: String,
    pub rows: Vec<Row>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub sections: Vec<Section>,
    pub error: Option<String>,
}

pub fn default_base_url() -> String {
    std::env::var("OPENRIG_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:7433".into())
}

fn token(operator: bool) -> Option<String> {
    let env = |key| {
        std::env::var(key)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    if operator && let Some(t) = env("OPENRIG_AUTH_BEARER_TOKEN") {
        return Some(t);
    }
    env("OPENRIG_TERMINAL_BEARER_TOKEN").or_else(|| {
        let home = env("OPENRIG_HOME")
            .or_else(|| env("RIGGED_HOME"))
            .or_else(|| env("HOME").map(|s| format!("{s}/.openrig")))?;
        std::fs::read_to_string(std::path::Path::new(&home).join("terminal-token"))
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    })
}
fn request(
    base: &str,
    route: &str,
    body: Option<Value>,
    operator: bool,
    timeout: u64,
) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout))
        .build();
    let url = format!("{}{}", base.trim_end_matches('/'), route);
    let mut req = if body.is_some() {
        agent.post(&url)
    } else {
        agent.get(&url)
    };
    if let Some(token) = token(operator) {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let response = match body {
        Some(value) => req.send_json(value),
        None => req.call(),
    };
    match response {
        Ok(response) => response
            .into_json()
            .map_err(|_| format!("{route}: invalid JSON response")),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_json::<Value>().ok().and_then(|v| {
                v.get("error")
                    .or_else(|| v.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
            Err(format!(
                "{route}: HTTP {code}{}",
                detail.map(|s| format!(" ({s})")).unwrap_or_default()
            ))
        }
        Err(_) => Err(format!("{route}: connection failed or timed out")),
    }
}
fn get(base: &str, route: &str) -> Result<Value, String> {
    request(
        base,
        route,
        None,
        false,
        if route.contains("usage/hosts") { 20 } else { 5 },
    )
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
/// Open an existing saved or derived terminal view, optionally enforcing a preview plan.
pub fn open_terminal(base: &str, view: &str, expected_plan: Option<&str>) -> Result<Value, String> {
    let mut body = json!({"view": view});
    if let Some(plan) = expected_plan {
        body["expectedPlan"] = json!(plan);
    }
    let result = request(base, "/api/terminal/open", Some(body), false, 30)?;
    if result["opened"].as_array().is_none_or(Vec::is_empty) {
        return Err(format!(
            "terminal open failed: {}",
            text(&result["error"])
                .or_else(|| text(&result["code"]))
                .unwrap_or_else(|| "no tiles opened".into())
        ));
    }
    Ok(result)
}
pub fn preview_terminal(base: &str, view: &str, provider: &str) -> Result<Value, String> {
    get(
        base,
        &format!(
            "/api/terminal/preview?view={}&provider={}",
            encode(view),
            encode(provider)
        ),
    )
}
pub fn launch_node(base: &str, rig: &str, node: &str) -> Result<Value, String> {
    request(
        base,
        &format!("/api/rigs/{}/nodes/{}/launch", encode(rig), encode(node)),
        Some(json!({})),
        false,
        30,
    )
}
pub fn mission_control_action(base: &str, input: Value) -> Result<Value, String> {
    request(base, "/api/mission-control/action", Some(input), true, 30)
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(v.to_string()),
        _ => None,
    }
}
fn field(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| text(&v[*k]))
}
fn row(v: &Value, depth: u8, fallback: &str) -> Row {
    let label = field(
        v,
        &[
            "name",
            "label",
            "summary",
            "mission",
            "logicalId",
            "hostId",
            "key",
            "seatSession",
            "identity",
            "id",
        ],
    )
    .or_else(|| text(v))
    .unwrap_or_else(|| fallback.into());
    let id = field(
        v,
        &[
            "id",
            "qitemId",
            "nodeId",
            "logicalId",
            "key",
            "hostId",
            "seatSession",
            "mission",
            "name",
        ],
    )
    .unwrap_or_else(|| label.clone());
    let detail = v
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| text(v).map(|s| (k.clone(), s)))
                .collect()
        })
        .unwrap_or_default();
    Row {
        id,
        depth,
        label,
        status: field(v, &["status", "state", "lifecycleState", "visibility"]),
        detail,
        children_count: 0,
    }
}
fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn generic_rows(v: &Value, keys: &[&str]) -> Vec<Row> {
    let values = v.as_array().map(Vec::as_slice).or_else(|| {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(Value::as_array).map(Vec::as_slice))
    });
    match values {
        Some(values) => values.iter().map(|v| row(v, 0, "item")).collect(),
        None if v.is_object() => vec![row(v, 0, "Overview")],
        _ => vec![],
    }
}
fn make_section(kind: SectionKind, title: &str, rows: Vec<Row>) -> Section {
    Section {
        kind,
        title: title.into(),
        rows,
    }
}
fn topology(
    base: &str,
    health: Option<&Value>,
    hosts: Option<&Value>,
    rigs: Option<&Value>,
    errors: &mut Vec<String>,
) -> Vec<Row> {
    let host_id = health
        .and_then(|v| field(v, &["selfHostId"]))
        .unwrap_or_else(|| "local".into());
    let summaries = rigs
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut result = if health.is_none() && hosts.is_none() && rigs.is_none() {
        Vec::new()
    } else {
        vec![Row {
            id: host_id.clone(),
            depth: 0,
            label: hosts
                .and_then(|v| field(v, &["ownName"]))
                .unwrap_or_else(|| host_id.clone()),
            status: None,
            detail: vec![],
            children_count: summaries.len(),
        }]
    };
    for rig in summaries {
        let mut rr = row(rig, 1, "rig");
        let rig_id = rr.id.clone();
        match get(base, &format!("/api/rigs/{}/nodes", encode(&rig_id))) {
            Ok(nodes) => {
                let mut pods: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
                for node in nodes
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|n| n["nodeKind"] == "agent")
                {
                    pods.entry(field(node, &["podNamespace"]).unwrap_or_else(|| "(no pod)".into()))
                        .or_default()
                        .push(node);
                }
                rr.children_count = pods.len();
                result.push(rr);
                for (name, agents) in pods {
                    result.push(Row {
                        id: format!("{rig_id}/{name}"),
                        depth: 2,
                        label: name,
                        status: None,
                        detail: vec![],
                        children_count: agents.len(),
                    });
                    for agent in agents {
                        let mut r = row(agent, 3, "agent");
                        r.id = format!("{rig_id}/{}", r.id);
                        result.push(r);
                    }
                }
            }
            Err(e) => {
                errors.push(e);
                result.push(rr);
            }
        }
    }
    for host in hosts.map(|h| array(h, "hosts")).unwrap_or(&[]) {
        if let Some(id) = field(host, &["id"])
            && id != "local"
            && id != host_id
        {
            let mut host_row = row(host, 0, "host");
            match get(base, &format!("/api/rigs/summary?host={}", encode(&id))) {
                Ok(remote) => {
                    let remote_rigs = remote.as_array().map(Vec::as_slice).unwrap_or(&[]);
                    host_row.children_count = remote_rigs.len();
                    result.push(host_row);
                    for rig in remote_rigs {
                        let mut remote_row = row(rig, 1, "rig");
                        remote_row.id = format!("{id}/{}", remote_row.id);
                        result.push(remote_row);
                    }
                }
                Err(e) => {
                    errors.push(e);
                    result.push(host_row);
                }
            }
        }
    }
    result
}

pub fn fetch_snapshot(base_url: &str) -> Snapshot {
    let routes = [
        "/healthz",
        "/api/hosts",
        "/api/rigs/summary",
        "/api/specs/library",
        "/api/scopes?detail=1",
        "/api/terminal/views",
        "/api/review/fleet",
        "/api/health?limit=200",
        "/api/config?view=browser",
        "/api/gateway/connections",
        "/api/provider/status",
        "/api/provider/usage/hosts",
        "/api/telemetry/usage/cost",
    ];
    let results: Vec<_> = std::thread::scope(|scope| {
        routes
            .iter()
            .map(|route| scope.spawn(move || get(base_url, route)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("read worker failed".into()))
            })
            .collect()
    });
    let mut errors: Vec<String> = results
        .iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect();
    let val = |i: usize| results[i].as_ref().ok();
    let mut sections = vec![make_section(
        SectionKind::Topology,
        "Topology",
        topology(base_url, val(0), val(1), val(2), &mut errors),
    )];
    for (kind, title, index, keys) in [
        (SectionKind::Specs, "Specs", 3, vec![]),
        (SectionKind::Scopes, "Scopes", 4, vec!["missions"]),
        (SectionKind::Needs, "Needs attention", 6, vec![]),
        (SectionKind::System, "System", 7, vec!["records"]),
        (SectionKind::Config, "Config", 8, vec!["entries"]),
        (SectionKind::Connections, "Connections", 9, vec!["humans"]),
        (SectionKind::Usage, "Usage", 10, vec!["usage"]),
        (SectionKind::UsageHosts, "Usage hosts", 11, vec!["hosts"]),
        (SectionKind::Cost, "Cost", 12, vec!["seats"]),
    ] {
        let mut rows = Vec::new();
        if let Some(v) = val(index) {
            if kind == SectionKind::Config
                && !(v["readOnly"] == true
                    && v["entries"].is_array()
                    && v["sources"].is_array()
                    && v["exclusions"].is_array())
            {
                errors.push("Config: invalid browser contract".into());
            } else if kind == SectionKind::Needs {
                rows = generic_rows(&v["needsYou"]["items"], &[]);
            } else if kind == SectionKind::Scopes {
                for mission in array(v, "missions") {
                    let slices = array(mission, "slices");
                    let mut r = row(mission, 0, "mission");
                    r.children_count = slices.len();
                    rows.push(r);
                    rows.extend(slices.iter().map(|s| row(s, 1, "slice")));
                }
            } else {
                rows = generic_rows(v, &keys);
            }
        }
        sections.push(make_section(kind, title, rows));
    }
    let mut terminals = Vec::new();
    if let Some(v) = val(5) {
        for saved in array(v, "saved") {
            let mut r = row(saved, 0, "saved view");
            r.id = format!("saved:{}", r.id);
            r.children_count = array(saved, "members").len();
            terminals.push(r);
        }
        for rig in array(v, "rigs") {
            let mut r = row(rig, 0, "rig");
            r.id = format!("rig:{}", r.id);
            terminals.push(r);
        }
    }
    sections.insert(
        3,
        make_section(SectionKind::Terminals, "Terminals", terminals),
    );
    Snapshot {
        sections,
        error: (!errors.is_empty()).then(|| errors.join("\n")),
    }
}
impl Snapshot {
    pub fn fixture() -> Self {
        let kinds = [
            (SectionKind::Topology, "Topology"),
            (SectionKind::Specs, "Specs"),
            (SectionKind::Scopes, "Scopes"),
            (SectionKind::Terminals, "Terminals"),
            (SectionKind::Needs, "Needs attention"),
            (SectionKind::System, "System"),
            (SectionKind::Config, "Config"),
            (SectionKind::Connections, "Connections"),
            (SectionKind::Usage, "Usage"),
            (SectionKind::UsageHosts, "Usage hosts"),
            (SectionKind::Cost, "Cost"),
        ];
        Self { sections: kinds.into_iter().map(|(kind,title)| { let mut rows = vec![row(&json!({"id":format!("demo-{title}"),"name":format!("Demo {title}"),"status":"ready","source":"Demo data"}),0,"demo")]; if kind == SectionKind::Topology { rows[0].label="local host".into(); rows[0].children_count=1; for (depth,name) in [(1,"demo rig"),(2,"engineering"),(3,"builder"),(3,"reviewer")] { let mut r=row(&json!({"id":name,"name":name,"status":"running","runtime":"jcode"}),depth,name); r.children_count=usize::from(depth<3); rows.push(r); } } make_section(kind,title,rows) }).collect(), error: None }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_has_all_sections_and_topology() {
        let s = Snapshot::fixture();
        assert_eq!(s.sections.len(), 11);
        assert!(s.sections[0].rows.iter().any(|r| r.depth == 3));
    }
    #[test]
    fn encodes_route_components() {
        assert_eq!(encode("rig/a b"), "rig%2Fa%20b");
    }
    #[test]
    fn failed_reads_are_not_success() {
        let s = fetch_snapshot("http://127.0.0.1:1");
        assert!(s.error.is_some());
        assert_eq!(s.sections.len(), 11);
    }
}
