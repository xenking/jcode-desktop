pub mod connections;
pub mod cost;
pub mod sections;
pub mod usage;
pub mod util;

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveEvent {
    pub ts: String,
    pub kind: String,
    pub summary: String,
    pub rig: Option<String>,
}

fn live_event(v: &Value) -> Option<LiveEvent> {
    let kind = field(v, &["kind", "type"]).unwrap_or_else(|| "activity".into());
    let summary = field(v, &["summary", "body", "seatNodeId"]).unwrap_or_else(|| kind.clone());
    v.is_object().then(|| LiveEvent {
        ts: field(v, &["ts", "tsEmitted", "timestamp"]).unwrap_or_default(),
        kind,
        summary,
        rig: field(v, &["rig", "rigId"]),
    })
}

/// Read one activity SSE connection. The caller decides whether and when to reconnect.
/// `should_stop` is asked once per frame the stream delivers, keepalive comments
/// included: a pane that only receives keepalives never parses an event, so an
/// event-only cancellation check would let its thread and connection outlive the
/// pane for as long as the daemon keeps the stream open.
pub fn subscribe_events(
    base_url: &str,
    should_stop: impl Fn() -> bool + Send,
    mut on_event: impl FnMut(LiveEvent) -> bool + Send,
) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        // Bounds each idle read so a caller can notice cancellation between keepalives.
        .timeout_read(Duration::from_secs(15))
        .build();
    let mut req = agent
        .get(&format!(
            "{}/api/activity/events",
            base_url.trim_end_matches('/')
        ))
        .set("Accept", "text/event-stream");
    if let Some(token) = token(false) {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let response = req.call().map_err(|e| format!("activity events: {e}"))?;
    if !response
        .header("content-type")
        .unwrap_or("")
        .contains("text/event-stream")
    {
        return Err("activity events: expected event stream".into());
    }
    let mut reader = BufReader::new(response.into_reader());
    let mut line = String::new();
    let mut data = Vec::new();
    let mut size = 0usize;
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|e| format!("activity events: {e}"))?
            == 0
        {
            return Err("activity events: stream ended".into());
        }
        if should_stop() {
            return Ok(());
        }
        size += line.len();
        if size > 1_048_576 {
            return Err("activity events: frame too large".into());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&data.join("\n"))
                && let Some(event) = live_event(&value)
                && !on_event(event)
            {
                return Ok(());
            }
            data.clear();
            size = 0;
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value).to_owned());
        } else if line == "data" {
            data.push(String::new());
        }
    }
}

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

/// Semantic color of a status or meter. The UI maps it onto the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Neutral,
    Ok,
    Warn,
    Error,
    Dim,
}

/// A progress pill: `percent` is the share to fill (0 to 100), `None` when the daemon did not serve one.
#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub label: String,
    pub percent: Option<f64>,
    pub tone: Tone,
}
impl Meter {
    /// Fill fraction in `0.0..=1.0`, `None` for an unknown or non-finite percentage.
    pub fn fraction(&self) -> Option<f32> {
        self.percent
            .filter(|p| p.is_finite())
            .map(|p| (p.clamp(0., 100.) / 100.) as f32)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub id: String,
    pub depth: u8,
    pub label: String,
    pub status: Option<String>,
    pub tone: Tone,
    pub detail: Vec<(String, String)>,
    pub children_count: usize,
    /// Show the row's children before the user toggles it.
    pub default_open: bool,
    pub meter: Option<Meter>,
}
impl Row {
    pub(crate) fn new(id: impl Into<String>, depth: u8, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            depth,
            label: label.into(),
            ..Self::default()
        }
    }
    pub(crate) fn status(mut self, status: impl Into<String>, tone: Tone) -> Self {
        self.status = Some(status.into());
        self.tone = tone;
        self
    }
    pub(crate) fn open(mut self) -> Self {
        self.default_open = true;
        self
    }
    pub(crate) fn meter(
        mut self,
        label: impl Into<String>,
        percent: Option<f64>,
        tone: Tone,
    ) -> Self {
        self.meter = Some(Meter {
            label: label.into(),
            percent,
            tone,
        });
        self
    }
    pub(crate) fn put(&mut self, key: &str, value: impl Into<String>) {
        self.detail.push((key.into(), value.into()));
    }
    pub(crate) fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.put(key, value);
        self
    }
    pub(crate) fn with_opt(mut self, key: &str, value: Option<impl Into<String>>) -> Self {
        if let Some(value) = value {
            self.put(key, value);
        }
        self
    }
}
/// Push `row`, let `build` append its descendants, then record how many direct children it got.
pub(crate) fn push_group(out: &mut Vec<Row>, row: Row, build: impl FnOnce(&mut Vec<Row>)) {
    let at = out.len();
    let child_depth = row.depth.saturating_add(1);
    out.push(row);
    build(out);
    out[at].children_count = out[at + 1..]
        .iter()
        .filter(|r| r.depth == child_depth)
        .count();
}
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub kind: SectionKind,
    pub title: String,
    pub rows: Vec<Row>,
}
#[derive(Clone, Debug, Default, PartialEq)]
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
/// A failed read: the HTTP status when the daemon answered (`None`: no answer), and its own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadFailure {
    pub status: Option<u16>,
    pub error: String,
    /// The daemon's own `message` (409 on usage across hosts: usage windows off here).
    pub message: Option<String>,
}
impl std::fmt::Display for ReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}
fn request_typed(
    base: &str,
    route: &str,
    body: Option<Value>,
    operator: bool,
    timeout: u64,
) -> Result<Value, ReadFailure> {
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
    let fail = |status, error: String| ReadFailure {
        status,
        error,
        message: None,
    };
    match response {
        Ok(response) => response
            .into_json()
            .map_err(|_| fail(None, format!("{route}: invalid JSON response"))),
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_json::<Value>().ok();
            let text = |key: &str| {
                body.as_ref()
                    .and_then(|v| v.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            let message = text("message");
            let detail = text("error").or_else(|| message.clone());
            Err(ReadFailure {
                status: Some(code),
                error: format!(
                    "{route}: HTTP {code}{}",
                    detail.map(|s| format!(" ({s})")).unwrap_or_default()
                ),
                message,
            })
        }
        Err(_) => Err(fail(
            None,
            format!("{route}: connection failed or timed out"),
        )),
    }
}
fn request(
    base: &str,
    route: &str,
    body: Option<Value>,
    operator: bool,
    timeout: u64,
) -> Result<Value, String> {
    request_typed(base, route, body, operator, timeout).map_err(|f| f.error)
}
fn get_typed(base: &str, route: &str) -> Result<Value, ReadFailure> {
    request_typed(
        base,
        route,
        None,
        false,
        if route.contains("usage/hosts") { 20 } else { 5 },
    )
}
fn get(base: &str, route: &str) -> Result<Value, String> {
    get_typed(base, route).map_err(|f| f.error)
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
    let mut row = Row::new(id, depth, label);
    row.status = field(v, &["status", "state", "lifecycleState", "visibility"]);
    row.detail = detail;
    row
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
fn reachability_tone(status: &str) -> Tone {
    match status {
        "reachable" | "ok" => Tone::Ok,
        "unreachable" => Tone::Error,
        _ => Tone::Warn,
    }
}
fn topology(
    base: &str,
    health: Option<&Value>,
    hosts: Option<&Value>,
    rigs: Option<&Value>,
    errors: &mut Vec<String>,
) -> Vec<Row> {
    let control_plane: connections::ControlPlane = health
        .and_then(|h| serde_json::from_value(h.clone()).ok())
        .unwrap_or_default();
    let host_id = control_plane
        .self_host_id
        .get()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".into());
    let summaries: util::Items<sections::RigSummary> = rigs
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .unwrap_or_default();
    let mut result = if health.is_none() && hosts.is_none() && rigs.is_none() {
        Vec::new()
    } else {
        let mut host = Row::new(
            host_id.clone(),
            0,
            hosts
                .and_then(|v| field(v, &["ownName"]))
                .unwrap_or_else(|| host_id.clone()),
        )
        .status("this host", Tone::Ok)
        .with("host id", host_id.clone())
        .with_opt("host id source", control_plane.self_host_id_source.get())
        .with_opt("daemon", control_plane.status.get())
        .with_opt("version", control_plane.semver.get())
        .with_opt("commit", control_plane.commit.get())
        .with_opt("built", control_plane.built_at.get());
        if control_plane.dirty.truthy() {
            host.put("build", "dirty");
        }
        host.children_count = summaries.len();
        vec![host]
    };
    for rig in &summaries {
        let rig_id = rig.id.or(&rig.name.unknown());
        let mut rr = sections::rig_row(rig, 1, &rig_id);
        match get(base, &format!("/api/rigs/{}/nodes", encode(&rig_id))) {
            Ok(nodes) => {
                let nodes: util::Items<sections::Node> =
                    serde_json::from_value(nodes).unwrap_or_default();
                let pods = sections::group_pods(nodes.as_slice());
                let seats: usize = pods.iter().map(|(_, a)| a.len()).sum();
                rr.put("seats", seats.to_string());
                rr.children_count = pods.len();
                result.push(rr);
                for (name, agents) in pods {
                    let mut pod = Row::new(format!("{rig_id}/{name}"), 2, name)
                        .with("seats", agents.len().to_string());
                    pod.children_count = agents.len();
                    result.push(pod);
                    for agent in agents {
                        let id = agent.node_id.or(&agent.logical_id.unknown());
                        result.push(sections::seat_row(agent, 3, &format!("{rig_id}/{id}")));
                    }
                }
            }
            Err(e) => {
                errors.push(e);
                rr.put("seat inventory", "unavailable");
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
            if let Some(status) = host_row.status.clone() {
                host_row.tone = reachability_tone(&status);
            }
            match get(base, &format!("/api/rigs/summary?host={}", encode(&id))) {
                Ok(remote) => {
                    let remote: util::Items<sections::RigSummary> =
                        serde_json::from_value(remote).unwrap_or_default();
                    host_row.children_count = remote.len();
                    result.push(host_row);
                    for rig in &remote {
                        let rig_id = rig.id.or(&rig.name.unknown());
                        result.push(sections::rig_row(rig, 1, &format!("{id}/{rig_id}")));
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

/// The daemon address without credentials or query, as the TUI shows its target.
fn daemon_target(base: &str) -> String {
    let base = base.trim_end_matches('/');
    match base.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
            let host = host.rsplit('@').next().unwrap_or(host);
            format!("{scheme}://{host}")
        }
        None => "unreported".into(),
    }
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
            .map(|route| scope.spawn(move || get_typed(base_url, route)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(ReadFailure {
                        status: None,
                        error: "read worker failed".into(),
                        message: None,
                    })
                })
            })
            .collect()
    });
    let mut errors: Vec<String> = results
        .iter()
        .filter_map(|r| r.as_ref().err().map(|f| f.error.clone()))
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
        if kind == SectionKind::Usage {
            let setting = match &results[10] {
                Ok(model) if model["usage"].is_array() => usage::SwitchSetting::Absent,
                _ => usage::switch_setting(val(8)),
            };
            rows = usage::usage_rows(&results[10], &setting);
        } else if kind == SectionKind::UsageHosts {
            rows = usage::usage_hosts_rows(&results[11]);
        } else if kind == SectionKind::Cost {
            rows = cost::cost_rows(&results[12]);
        } else if kind == SectionKind::Connections {
            rows = connections::connections_rows(
                results[9].as_ref().map_err(|f| f.error.as_str()),
                val(0),
                &daemon_target(base_url),
            );
        } else if let Some(v) = val(index) {
            if kind == SectionKind::Config
                && !(v["readOnly"] == true
                    && v["entries"].is_array()
                    && v["sources"].is_array()
                    && v["exclusions"].is_array())
            {
                errors.push("Config: invalid browser contract".into());
            } else if kind == SectionKind::Config {
                rows = sections::config_rows(v);
            } else if kind == SectionKind::Needs {
                let items: util::Items<sections::NeedsItem> =
                    serde_json::from_value(v["needsYou"]["items"].clone()).unwrap_or_default();
                rows = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| sections::needs_row(item, i))
                    .collect();
            } else if kind == SectionKind::Scopes {
                rows = sections::scopes_rows(v);
            } else if kind == SectionKind::Specs {
                rows = sections::specs_rows(v);
            } else if kind == SectionKind::System {
                rows = sections::health_rows(v);
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
/// Offline demo rows for the sections whose shape matters most visually.
fn demo_rows(kind: SectionKind) -> Option<Vec<Row>> {
    let parse = |text: &str| serde_json::from_str::<Value>(text).ok();
    match kind {
        SectionKind::Usage => Some(usage::usage_rows(
            &Ok(parse(include_str!("../tests/fixtures/usage.json"))?),
            &usage::SwitchSetting::Absent,
        )),
        SectionKind::UsageHosts => Some(usage::usage_hosts_rows(&Ok(parse(include_str!(
            "../tests/fixtures/usage_hosts.json"
        ))?))),
        SectionKind::Cost => Some(cost::cost_rows(&Ok(parse(include_str!(
            "../tests/fixtures/cost.json"
        ))?))),
        SectionKind::Connections => Some(connections::connections_rows(
            Ok(&parse(include_str!("../tests/fixtures/connections.json"))?),
            parse(include_str!("../tests/fixtures/healthz.json")).as_ref(),
            "http://127.0.0.1:7433",
        )),
        _ => None,
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
        Self { sections: kinds.into_iter().map(|(kind,title)| { let mut rows = vec![row(&json!({"id":format!("demo-{title}"),"name":format!("Demo {title}"),"status":"ready","source":"Demo data"}),0,"demo")]; if kind == SectionKind::Topology { rows[0].label="local host".into(); rows[0].children_count=1; for (depth,name) in [(1,"demo rig"),(2,"engineering"),(3,"builder"),(3,"reviewer")] { let mut r=row(&json!({"id":name,"name":name,"status":"running","runtime":"jcode"}),depth,name); r.children_count=usize::from(depth<3); rows.push(r); } } if let Some(real) = demo_rows(kind) { rows = real; } make_section(kind,title,rows) }).collect(), error: None }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_activity_notification() {
        let e = live_event(&json!({"type":"seat_changed","seatNodeId":"node-1","seq":3})).unwrap();
        assert_eq!(e.kind, "seat_changed");
        assert_eq!(e.summary, "node-1");
        assert!(live_event(&json!(null)).is_none());
    }
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

    /// A closed mission-control pane must stop its subscription even when the
    /// daemon only sends SSE comment keepalives: no JSON event ever arrives, so
    /// an event-only cancellation check never runs and the thread lives on.
    #[test]
    fn keepalive_only_stream_observes_cancellation() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            // Read the request head: a blank line ends the headers, and the GET
            // carries no body.
            loop {
                request.clear();
                match reader.read_line(&mut request) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                if request == "\r\n" || request == "\n" {
                    break;
                }
            }
            let mut socket = stream;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
                .unwrap();
            let _ = socket.flush();
            // Keepalive comments only, then a clean close so the pre-fix code
            // fails fast instead of waiting out the 15-second read timeout.
            for _ in 0..6 {
                if socket.write_all(b": keepalive\n").is_err() {
                    return;
                }
                let _ = socket.flush();
                std::thread::sleep(Duration::from_millis(40));
            }
        });

        let frames = AtomicUsize::new(0);
        let started = Instant::now();
        let result = subscribe_events(
            &format!("http://{addr}"),
            || frames.fetch_add(1, Ordering::SeqCst) >= 2,
            |_| true,
        );
        let elapsed = started.elapsed();
        server.join().unwrap();
        assert!(
            result.is_ok(),
            "a cancelled subscription ends cleanly, not with {:?}",
            result.err()
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the subscription stopped on a keepalive frame after {elapsed:?}"
        );
    }
}

/// Newest page of the daemon's activity stream, oldest to newest, as the TUI footer shows it.
pub fn fetch_stream_tail(base_url: &str, limit: usize) -> Result<Vec<LiveEvent>, String> {
    let value = get(
        base_url,
        &format!("/api/stream/list?limit={limit}&direction=latest"),
    )?;
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            Some(LiveEvent {
                ts: field(item, &["tsEmitted", "ts"]).unwrap_or_default(),
                kind: field(item, &["sourceSession"]).unwrap_or_else(|| "stream".into()),
                summary: field(item, &["body"])?
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                rig: None,
            })
        })
        .collect())
}

#[cfg(test)]
mod stream_tests {
    #[test]
    fn stream_tail_parses_items() {
        let v = serde_json::json!([{"tsEmitted":"2026-11-15T01:00:00Z","sourceSession":"dev@rig","body":"hello\nmore","streamSortKey":"1"}]);
        let items = v.as_array().unwrap();
        let e = super::field(&items[0], &["body"]).unwrap();
        assert!(e.starts_with("hello"));
    }
}
