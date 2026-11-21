//! Typed rows for the remaining sections, mirroring what `packages/tui/src/hydrate.ts` carries:
//! seats (status, context, work, activity), Needs-You items, Scopes slices, Specs, System health
//! findings and Config entries. Served words are carried as served.
use crate::{
    Row, Tone, push_group,
    util::{Items, Lax, Text, clip, join, js_number},
};
use serde::Deserialize;
use serde_json::Value;

const SECTION_HOST_LIMIT: usize = 16;
pub(crate) const SECTION_LIST_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadWindow<T> {
    pub items: Vec<T>,
    pub truncated: bool,
}

impl<T> From<Vec<T>> for ReadWindow<T> {
    fn from(mut items: Vec<T>) -> Self {
        let truncated = items.len() > SECTION_LIST_LIMIT;
        items.truncate(SECTION_LIST_LIMIT);
        Self { items, truncated }
    }
}

fn registry(base: &str) -> Result<crate::host::HostRegistry, String> {
    section_get(&crate::HostTarget::primary(base, "local"), "/api/hosts")
}

pub(crate) fn host_target(base: &str, alias: &str) -> Result<crate::HostTarget, String> {
    if alias == "local" {
        return Ok(crate::HostTarget::primary(base, alias));
    }
    registry(base)?
        .into_targets()
        .find_map(|(target, _)| (target.alias() == alias).then_some(target))
        .ok_or_else(|| "Host is not in the OpenRig registry".into())
}

pub(crate) fn read_hosts<T: Send>(
    base: &str,
    read: impl Fn(&crate::HostTarget) -> T + Sync,
) -> (Vec<T>, Option<String>) {
    let mut hosts = vec![crate::HostTarget::primary(base, "local")];
    let issue = match registry(base) {
        Ok(catalog) => {
            for (target, _) in catalog.into_targets() {
                if !hosts.iter().any(|host| host.alias() == target.alias()) {
                    hosts.push(target);
                    if hosts.len() > SECTION_HOST_LIMIT {
                        break;
                    }
                }
            }
            (hosts.len() > SECTION_HOST_LIMIT).then(|| {
                hosts.truncate(SECTION_HOST_LIMIT);
                format!("Host list truncated to {SECTION_HOST_LIMIT} hosts")
            })
        }
        Err(error) => Some(format!("Other hosts unavailable: {error}")),
    };
    let results = std::thread::scope(|scope| {
        let read = &read;
        hosts
            .iter()
            .map(|host| scope.spawn(move || read(host)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|task| task.join().expect("section reader panicked"))
            .collect()
    });
    (results, issue)
}

pub(crate) fn section_get<T: serde::de::DeserializeOwned>(
    target: &crate::HostTarget,
    route: &str,
) -> Result<T, String> {
    use std::io::Read;

    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let bearer = target.bearer(false)?;
    let response = target
        .request("GET", route, 8, bearer.as_deref())?
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            ureq::Error::Transport(_) => "Connection failed or timed out".into(),
        })?;
    if response.status() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read response".to_owned())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Response exceeds the 8 MiB limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Unexpected response format".into())
}

#[cfg(test)]
pub(crate) fn serve_fixture(
    count: usize,
    response: impl Fn(&str) -> (u16, String) + Send + 'static,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        for _ in 0..count {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing fixture request");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            assert_eq!(parts.next(), Some("GET"));
            let (code, body) = response(parts.next().unwrap());
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            write!(stream, "HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (base, handle)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextUsage {
    pub availability: Text,
    pub used_percentage: Lax<f64>,
    pub context_window_size: Lax<f64>,
    pub total_input_tokens: Lax<f64>,
    pub total_output_tokens: Lax<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct NeedsInput {
    pub count: Lax<f64>,
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityState {
    pub activity: Text,
    pub display: Text,
    pub needs_input: Lax<NeedsInput>,
    pub decided_by: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentActivity {
    pub state: Text,
    pub reason: Text,
    pub evidence_source: Text,
    pub event_at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct IdentityVerdict {
    pub verdict: Text,
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct NeedsInputEvidence {
    pub rung: Text,
    pub observed_at: Text,
}

/// One node of `GET /api/rigs/:id/nodes`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Node {
    pub node_id: Text,
    pub logical_id: Text,
    pub pod_namespace: Text,
    pub node_kind: Text,
    pub runtime: Text,
    pub model: Text,
    pub effort: Text,
    pub effort_source: Text,
    pub lifecycle_state: Text,
    pub session_status: Text,
    pub startup_status: Text,
    pub terminal_active: Lax<bool>,
    pub last_activity_at: Text,
    pub needs_attention: Lax<bool>,
    pub arbitrated_needs_input: Lax<bool>,
    pub needs_input_evidence: Lax<NeedsInputEvidence>,
    pub agent_activity: Lax<AgentActivity>,
    pub activity_state: Lax<ActivityState>,
    pub identity_verdict: Lax<IdentityVerdict>,
    pub community_harness: Lax<Value>,
    pub canonical_session_name: Text,
    pub tmux_attach_command: Text,
    pub cwd: Text,
    pub resolved_spec_name: Text,
    pub profile: Text,
    pub resolved_spec_version: Text,
    pub resolved_spec_hash: Text,
    pub context_usage: Lax<ContextUsage>,
    pub assigned_work_count: Lax<f64>,
    pub pending_work_count: Lax<f64>,
    pub in_progress_work_count: Lax<f64>,
    pub blocked_work_count: Lax<f64>,
}

impl Node {
    /// The visible status label, derived exactly as `toAgentRow` in hydrate.ts derives it:
    /// lifecycle truth drives actions, session and terminal activity drive the label.
    pub fn display_status(&self) -> String {
        let verdict = self.identity_verdict.get();
        let verdict_is = |v: &str| verdict.is_some_and(|x| x.verdict.is(v));
        let reason_is = |r: &str| verdict.is_some_and(|x| x.reason.is(r));
        let identity_downranked =
            verdict_is("mismatch") || (verdict_is("pane_missing") && !reason_is("session_missing"));
        let activity = self.activity_state.get();
        let display = |d: &str| activity.is_some_and(|a| a.display.is(d));
        let signal = |s: &str| self.agent_activity.get().is_some_and(|a| a.state.is(s));
        let needs_attention = self.needs_attention.get().copied();
        let lifecycle_attention = self.lifecycle_state.is("attention_required")
            || self.startup_status.is("attention_required");
        let terminal_active = self.terminal_active.get().copied();
        if reason_is("session_missing")
            && self.session_status.is("detached")
            && !self.lifecycle_state.is("attention_required")
        {
            "detached".into()
        } else if self.startup_status.is("failed") {
            "failed".into()
        } else if identity_downranked || (needs_attention.is_none() && lifecycle_attention) {
            "attention_required".into()
        } else if needs_attention == Some(true) {
            if self
                .arbitrated_needs_input
                .get()
                .copied()
                .unwrap_or_else(|| signal("needs_input"))
            {
                "needs_input".into()
            } else {
                "attention_required".into()
            }
        } else if needs_attention.is_none() && (display("needs-input") || signal("needs_input")) {
            "needs_input".into()
        } else if self.session_status.is("running") || self.session_status.is("ready") {
            if display("working") {
                "active".into()
            } else if display("idle") {
                "idle".into()
            } else if terminal_active == Some(true)
                || (terminal_active.is_none() && signal("running"))
            {
                "active".into()
            } else if self.community_harness.get().is_some() || activity.is_some() {
                "unknown".into()
            } else {
                "idle".into()
            }
        } else {
            self.session_status.or("unknown")
        }
    }
}

fn status_tone(status: &str) -> Tone {
    match status {
        "active" | "running" | "ready" => Tone::Ok,
        "idle" | "detached" | "stopped" => Tone::Dim,
        "needs_input" | "attention_required" | "unknown" => Tone::Warn,
        "failed" => Tone::Error,
        _ => Tone::Neutral,
    }
}

fn count_text(label: &str, n: &Lax<f64>) -> Option<String> {
    n.num().map(|n| format!("{label} {}", js_number(n)))
}

fn tokens_short(input: Option<f64>, output: Option<f64>) -> Option<String> {
    if input.is_none() && output.is_none() {
        return None;
    }
    let total = input.unwrap_or(0.) + output.unwrap_or(0.);
    Some(if total >= 1000. {
        format!("{}k", (total / 1000.).round() as i64)
    } else {
        js_number(total)
    })
}

/// A seat: status, runtime, model, effort, context, work and activity.
pub fn seat_row(node: &Node, depth: u8, id: &str) -> Row {
    let status = node.display_status();
    let ctx = node
        .context_usage
        .get()
        .filter(|c| c.availability.is("known"));
    let used = ctx.and_then(|c| c.used_percentage.num()).map(f64::round);
    let mut row = Row::new(id, depth, node.logical_id.or("agent"))
        .status(status.clone(), status_tone(&status));
    row.default_open = false;
    row = row
        .with("status", status)
        .with("runtime", node.runtime.or("unknown"))
        .with_opt("model", node.model.get())
        .with_opt(
            "effort",
            node.effort.get().map(|e| match node.effort_source.get() {
                Some(src) => format!("{e} ({src})"),
                None => e.to_owned(),
            }),
        )
        .with_opt("spec", node.resolved_spec_name.get())
        .with_opt("profile", node.profile.get())
        .with_opt("spec version", node.resolved_spec_version.get())
        .with_opt("spec hash", node.resolved_spec_hash.get())
        .with_opt("session", node.canonical_session_name.get())
        .with_opt("attach", node.tmux_attach_command.get())
        .with_opt("cwd", node.cwd.get())
        .with_opt("lifecycle", node.lifecycle_state.get())
        .with_opt("last activity", node.last_activity_at.get());
    match ctx {
        Some(c) => {
            row.put(
                "context",
                used.map_or("—".into(), |u| format!("{}%", js_number(u))),
            );
            if let Some(size) = c.context_window_size.num() {
                row.put("context window", js_number(size));
            }
            if let Some(t) = tokens_short(c.total_input_tokens.num(), c.total_output_tokens.num()) {
                row.put("tokens", t);
            }
        }
        None => row.put("context", "—"),
    }
    if let Some(used) = used {
        let tone = if used >= 90. {
            Tone::Error
        } else if used >= 75. {
            Tone::Warn
        } else {
            Tone::Neutral
        };
        row = row.meter(format!("context {}%", js_number(used)), Some(used), tone);
    }
    let work: Vec<String> = [
        count_text("assigned", &node.assigned_work_count),
        count_text("pending", &node.pending_work_count),
        count_text("in progress", &node.in_progress_work_count),
        count_text("blocked", &node.blocked_work_count),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !work.is_empty() {
        row.put("work", work.join(" · "));
    }
    if let Some(a) = node.activity_state.get() {
        row.put("activity", a.activity.or(&a.display.unknown()));
        if let Some(by) = a.decided_by.get() {
            row.put("decided by", by);
        }
        if let Some(n) = a.needs_input.get() {
            row.put(
                "needs input",
                format!(
                    "{}{}",
                    n.count.num().map_or("0".into(), js_number),
                    n.reason
                        .get()
                        .map(|r| format!(" · {r}"))
                        .unwrap_or_default()
                ),
            );
        }
    }
    if let Some(a) = node.agent_activity.get() {
        row.put(
            "signal",
            join(
                [
                    a.state.get().map(str::to_owned),
                    a.reason.get().map(str::to_owned),
                    a.evidence_source.get().map(|s| format!("via {s}")),
                    a.event_at.get().map(|s| format!("at {s}")),
                ]
                .into_iter()
                .flatten(),
                " · ",
            ),
        );
    }
    if let Some(e) = node.needs_input_evidence.get() {
        row.put(
            "needs-input evidence",
            format!("{} at {}", e.rung.unknown(), e.observed_at.unknown()),
        );
    }
    if let Some(v) = node.identity_verdict.get() {
        row.put(
            "identity",
            format!(
                "{}{}",
                v.verdict.unknown(),
                v.reason
                    .get()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            ),
        );
    }
    if let Some(active) = node.terminal_active.get() {
        row.put("pane output", if *active { "active" } else { "quiet" });
    }
    row
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RigSummary {
    pub id: Text,
    pub name: Text,
    pub lifecycle_state: Text,
    pub has_live_agents: Lax<bool>,
}

/// A rig row: lifecycle state beside the name (non-running states surface), live-agent presence.
pub fn rig_row(rig: &RigSummary, depth: u8, id: &str) -> Row {
    let mut row = Row::new(id, depth, rig.name.or(&rig.id.or("rig")));
    if let Some(state) = rig.lifecycle_state.get() {
        row = row.status(
            state,
            if state == "running" {
                Tone::Ok
            } else {
                Tone::Warn
            },
        );
    }
    row = row.with_opt("id", rig.id.get());
    match rig.has_live_agents.get() {
        Some(live) => row.put("live agents", live.to_string()),
        None => row.put("live agents", "unknown"),
    }
    row
}

/// A pod's seats grouped as the TUI groups them: infrastructure nodes are not listed,
/// and a node without a pod namespace sits under "(no pod)".
pub fn group_pods(nodes: &[Node]) -> Vec<(String, Vec<&Node>)> {
    let mut pods: Vec<(String, Vec<&Node>)> = Vec::new();
    for node in nodes.iter().filter(|n| n.node_kind.is("agent")) {
        let pod = node.pod_namespace.or("(no pod)");
        match pods.iter_mut().find(|(p, _)| *p == pod) {
            Some((_, list)) => list.push(node),
            None => pods.push((pod, vec![node])),
        }
    }
    pods.sort_by(|a, b| a.0.cmp(&b.0));
    pods
}

/// A Needs-You item (`NeedsYouItemRead`): the daemon's ordered union, carried verbatim.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct NeedsItem {
    pub host_id: Text,
    pub source: Text,
    pub identity: Text,
    pub summary: Text,
    pub leg: Text,
    #[serde(rename = "where")]
    pub place: Text,
    pub destination_session: Text,
    pub derived: Lax<Derived>,
    pub qitem_id: Text,
    pub evidence_ref: Text,
    pub unblocks: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Derived {
    pub kind: Text,
    pub evidence: Text,
    pub threshold: Text,
}

pub fn needs_row(item: &NeedsItem, index: usize) -> Row {
    let derived = item.derived.get();
    let target = if item.source.is("derived") {
        item.identity
            .get()
            .and_then(|i| i.split('|').next())
            .map(str::to_owned)
            .unwrap_or_else(|| item.place.unknown())
    } else {
        item.destination_session.or(&item.place.unknown())
    };
    let kind = derived
        .and_then(|d| d.kind.get())
        .map_or_else(|| item.leg.unknown(), str::to_owned);
    let detail = match derived {
        Some(d) => format!("{} — {}", item.summary.unknown(), d.evidence.unknown()),
        None => item.summary.unknown(),
    };
    let tone = if item.source.is("derived") {
        Tone::Warn
    } else {
        Tone::Neutral
    };
    let mut row = Row::new(
        format!("needs/{index}"),
        0,
        format!("{target} · {}", clip(&detail, 80)),
    )
    .status(kind.clone(), tone)
    .with("source", item.source.unknown())
    .with("kind", kind)
    .with("target", target)
    .with("detail", detail)
    .with_opt("host", item.host_id.get())
    .with_opt("queue item", item.qitem_id.get())
    .with_opt("evidence", item.evidence_ref.get())
    .with_opt("unblocks", item.unblocks.get())
    .with_opt("where", item.place.get());
    if let Some(d) = derived {
        row.put("threshold", d.threshold.unknown());
    }
    row
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Proof {
    pub paired: Lax<f64>,
    pub total: Lax<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Lock {
    pub by: Text,
    pub at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Locks {
    pub spec: Lax<Lock>,
    pub delivery: Lax<Lock>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReadinessItem {
    pub state: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Readiness {
    pub configured: Lax<bool>,
    pub state: Text,
    pub revision: Text,
    pub issues: Items<Text>,
    pub items: Items<ReadinessItem>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Slice {
    pub error: Text,
    pub dir_name: Text,
    pub id: Text,
    pub display_name: Text,
    pub status: Text,
    pub stage: Text,
    pub locks: Lax<Locks>,
    pub proof: Lax<Proof>,
    pub intent: Text,
    pub mini_requirements: Items<Text>,
    pub readiness: Lax<Readiness>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Mission {
    pub mission: Text,
    pub error: Text,
    pub slices: Items<Slice>,
}

/// `proofBadge` from scopes-model.ts: accepted count when readiness is configured, else paired/total.
pub fn proof_badge(s: &Slice) -> (String, Option<f64>) {
    if let Some(r) = s.readiness.get().filter(|r| r.configured.truthy()) {
        let accepted = r.items.iter().filter(|i| i.state.is("accepted")).count();
        let total = r.items.len();
        return (
            format!("accepted: {accepted}/{total} · {}", r.state.unknown()),
            (total > 0).then(|| accepted as f64 * 100. / total as f64),
        );
    }
    let proof = s.proof.get().cloned().unwrap_or_default();
    let (paired, total) = (
        proof.paired.num().unwrap_or(0.),
        proof.total.num().unwrap_or(0.),
    );
    let base = format!("proof: {} / {} paired", js_number(paired), js_number(total));
    let locked = s.locks.get().is_some_and(|l| l.delivery.get().is_some());
    (
        if locked { format!("{base} 🔒") } else { base },
        (total > 0.).then(|| paired * 100. / total),
    )
}

fn slice_tone(s: &Slice) -> Tone {
    if s.error.is_set() {
        Tone::Error
    } else if s.readiness.get().is_some_and(|r| r.configured.truthy()) {
        if s.readiness.get().is_some_and(|r| r.state.is("ready")) {
            Tone::Ok
        } else {
            Tone::Warn
        }
    } else if s.locks.get().is_some_and(|l| l.delivery.get().is_some()) {
        Tone::Ok
    } else {
        Tone::Neutral
    }
}

pub fn scopes_rows(v: &Value) -> Vec<Row> {
    let missions: Items<Mission> =
        serde_json::from_value(v["missions"].clone()).unwrap_or_default();
    let mut rows = Vec::new();
    for (mi, m) in missions.iter().enumerate() {
        let id = format!("scopes/{mi}");
        let mut row = Row::new(&id, 0, m.mission.or("mission")).open();
        if let Some(e) = m.error.get() {
            row = row.status("unavailable", Tone::Error).with("error", e);
        }
        push_group(&mut rows, row, |out| {
            for (si, s) in m.slices.iter().enumerate() {
                let (badge, percent) = proof_badge(s);
                let stage = s
                    .stage
                    .get()
                    .or(s.status.get())
                    .unwrap_or("unknown")
                    .to_owned();
                let tone = slice_tone(s);
                let mut r = Row::new(
                    format!("{id}/{si}"),
                    1,
                    s.display_name.or(&s.dir_name.or("slice")),
                )
                .status(stage.clone(), tone)
                .with_opt("id", s.id.get())
                .with_opt("directory", s.dir_name.get())
                .with("stage", stage)
                .with_opt("status", s.status.get())
                .with("proof", badge.clone())
                .with_opt("error", s.error.get())
                .with_opt("intent", s.intent.get());
                if let Some(locks) = s.locks.get() {
                    for (name, lock) in [
                        ("spec lock", &locks.spec),
                        ("delivery lock", &locks.delivery),
                    ] {
                        if let Some(l) = lock.get() {
                            r.put(name, format!("{} at {}", l.by.unknown(), l.at.unknown()));
                        }
                    }
                }
                for (ri, req) in s.mini_requirements.iter().enumerate() {
                    r.put(&format!("requirement {}", ri + 1), req.unknown());
                }
                if let Some(rd) = s.readiness.get().filter(|r| r.configured.truthy()) {
                    r.put(
                        "readiness",
                        format!(
                            "{} · revision {}",
                            rd.state.unknown(),
                            rd.revision.unknown()
                        ),
                    );
                    for issue in &rd.issues {
                        r.put("issue", issue.unknown());
                    }
                }
                if let Some(p) = percent {
                    r = r.meter(badge, Some(p), tone);
                }
                out.push(r);
            }
        });
    }
    rows
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Spec {
    pub id: Text,
    pub kind: Text,
    pub name: Text,
    pub version: Text,
    pub source_path: Text,
    pub resolved_source_path: Text,
    pub source_type: Text,
    pub relative_path: Text,
    pub updated_at: Text,
    pub roles_count: Lax<f64>,
    pub steps_count: Lax<f64>,
    pub status: Text,
}

/// Specs grouped by kind (rig, agent, workflow), each spec a row beneath its kind.
pub fn specs_rows(v: &Value) -> Vec<Row> {
    let specs: Items<Spec> = serde_json::from_value(v.clone()).unwrap_or_default();
    let mut rows = Vec::new();
    for kind in ["rig", "agent", "workflow"] {
        let of_kind: Vec<_> = specs.iter().filter(|s| s.kind.is(kind)).collect();
        if of_kind.is_empty() {
            continue;
        }
        push_group(
            &mut rows,
            Row::new(
                format!("specs/{kind}"),
                0,
                format!("{kind} ({})", of_kind.len()),
            )
            .open(),
            |out| {
                for (i, s) in of_kind.iter().enumerate() {
                    let mut r = Row::new(
                        format!("specs/{kind}/{}", s.id.or(&i.to_string())),
                        1,
                        s.name.unknown(),
                    )
                    .with("kind", kind)
                    .with_opt("version", s.version.get())
                    .with_opt("source", s.source_type.get())
                    .with_opt("path", s.resolved_source_path.get().or(s.source_path.get()))
                    .with_opt("relative path", s.relative_path.get())
                    .with_opt("updated", s.updated_at.get());
                    if let Some(status) = s.status.get() {
                        r = r.status(status, Tone::Neutral).with("status", status);
                    }
                    if let Some(n) = s.roles_count.num() {
                        r.put("roles", js_number(n));
                    }
                    if let Some(n) = s.steps_count.num() {
                        r.put("steps", js_number(n));
                    }
                    out.push(r);
                }
            },
        );
    }
    rows
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Freshness {
    pub state: Text,
    pub age_seconds: Lax<f64>,
    pub evaluated_at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Scope {
    #[serde(rename = "type")]
    pub kind: Text,
    #[serde(rename = "instanceId")]
    pub instance_id: Text,
    #[serde(rename = "rigId")]
    pub rig_id: Text,
    #[serde(rename = "seatId")]
    pub seat_id: Text,
    #[serde(rename = "missionId")]
    pub mission_id: Text,
    #[serde(rename = "sliceId")]
    pub slice_id: Text,
}
impl Scope {
    fn name(&self) -> String {
        match self.kind.get() {
            Some("instance") => self.instance_id.unknown(),
            Some("rig") => self.rig_id.unknown(),
            Some("seat") => self.seat_id.unknown(),
            Some("mission") => self.mission_id.unknown(),
            Some("slice") => self.slice_id.unknown(),
            _ => "unknown".into(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Evidence {
    #[serde(rename = "type")]
    pub kind: Text,
    #[serde(rename = "observedAt")]
    pub observed_at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Ceremony {
    pub stage: Text,
    pub basis: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Posture {
    pub posture: Text,
    pub source: Text,
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Health {
    pub id: Text,
    pub detector: Text,
    pub category: Text,
    pub scope: Lax<Scope>,
    pub severity: Text,
    pub confidence: Text,
    pub status: Text,
    pub started_at: Text,
    pub last_observed_at: Text,
    pub freshness: Lax<Freshness>,
    pub summary: Text,
    pub evidence: Items<Evidence>,
    pub threshold: Text,
    pub explanation: Text,
    pub suggested_inspection: Text,
    pub indeterminate_reason: Text,
    pub policy_version: Text,
    pub ceremony: Lax<Ceremony>,
    pub operating_posture: Lax<Posture>,
}

impl Health {
    /// `Unknown` when the finding is indeterminate or its source is not fresh, as the TUI words it.
    fn condition(&self) -> Option<&'static str> {
        let fresh = self
            .freshness
            .get()
            .map(|f| f.state.unknown())
            .unwrap_or_else(|| "unavailable".into());
        match fresh.as_str() {
            "stale" => Some("STALE"),
            _ if self.status.is("indeterminate")
                || fresh == "unavailable"
                || fresh == "contradictory" =>
            {
                Some("Unknown")
            }
            _ if self.status.is("cleared") => Some("CLEARED"),
            _ => None,
        }
    }
    fn tone(&self) -> Tone {
        let fresh = self.freshness.get().is_some_and(|f| f.state.is("fresh"));
        if self.status.is("indeterminate") || !fresh {
            Tone::Warn
        } else if self.status.is("cleared") {
            Tone::Dim
        } else if self.severity.is("critical") {
            Tone::Error
        } else if self.severity.is("warning") {
            Tone::Warn
        } else {
            Tone::Neutral
        }
    }
}

fn age(f: &Freshness) -> String {
    match f.age_seconds.num() {
        None => "—".into(),
        Some(s) if s < 60. => format!("{}s", s.floor()),
        Some(s) if s < 3600. => format!("{}m", (s / 60.).floor()),
        Some(s) if s < 86_400. => format!("{}h", (s / 3600.).floor()),
        Some(s) => format!("{}d", (s / 86_400.).floor()),
    }
}

/// System: the canonical instance health findings, sorted active first, then severity, then recency.
pub fn health_rows(v: &Value) -> Vec<Row> {
    let records: Items<Health> = serde_json::from_value(v["records"].clone()).unwrap_or_default();
    let rank = |s: &Text, order: &[&str]| order.iter().position(|o| s.is(o)).unwrap_or(order.len());
    let mut sorted: Vec<&Health> = records.iter().collect();
    sorted.sort_by(|a, b| {
        rank(&a.status, &["active", "indeterminate", "cleared"])
            .cmp(&rank(&b.status, &["active", "indeterminate", "cleared"]))
            .then(
                rank(&a.severity, &["critical", "warning", "info"])
                    .cmp(&rank(&b.severity, &["critical", "warning", "info"])),
            )
            .then(b.last_observed_at.get().cmp(&a.last_observed_at.get()))
            .then(a.id.get().cmp(&b.id.get()))
    });
    let mut rows = Vec::new();
    let active: Vec<_> = sorted.iter().filter(|r| r.status.is("active")).collect();
    let count = |sev: &str| active.iter().filter(|r| r.severity.is(sev)).count();
    let mut overview = Row::new("system/overview", 0, "Health")
        .with(
            "active",
            format!(
                "CRIT {} · WARN {} · INFO {}",
                count("critical"),
                count("warning"),
                count("info")
            ),
        )
        .with("findings", sorted.len().to_string());
    if v["truncated"] == true {
        overview.put("coverage", "PARTIAL · the bounded read was truncated; omitted findings are unevaluated, not healthy");
    }
    if let Some(total) = v["total"].as_u64() {
        overview.put("total", total.to_string());
    }
    if let Some(at) = v["evaluatedAt"].as_str() {
        overview.put("evaluated", at);
    }
    for c in v["coverage"].as_array().into_iter().flatten() {
        let text = if c["status"] == "unavailable" {
            format!(
                "{} unavailable: {}",
                c["source"].as_str().unwrap_or("source"),
                c["reason"].as_str().unwrap_or("no reason given")
            )
        } else {
            format!(
                "{} evaluated {}/{} {} ({} omitted)",
                c["source"].as_str().unwrap_or("source"),
                c["evaluated"],
                c["total"],
                c["unit"].as_str().unwrap_or("items"),
                c["omitted"]
            )
        };
        overview.put("source coverage", text);
    }
    overview.status = Some(if active.is_empty() {
        "no findings served".into()
    } else {
        "active".into()
    });
    overview.tone = if count("critical") > 0 {
        Tone::Error
    } else if count("warning") > 0 {
        Tone::Warn
    } else {
        Tone::Dim
    };
    if sorted.is_empty() {
        overview.put("note", "no findings served; not a healthy verdict");
    }
    rows.push(overview);
    for (i, h) in sorted.iter().enumerate() {
        let state = format!(
            "{}{}",
            h.severity.or("unknown").to_uppercase(),
            h.condition().map(|c| format!(" {c}")).unwrap_or_default()
        );
        let scope = h.scope.get().cloned().unwrap_or_default();
        let mut r = Row::new(
            format!("system/{i}"),
            0,
            h.summary.or(&h.detector.unknown()),
        )
        .status(state, h.tone())
        .with("detector", h.detector.unknown())
        .with("finding id", h.id.unknown())
        .with(
            "scope",
            format!("{} {}", scope.kind.unknown(), scope.name()),
        )
        .with("category", h.category.unknown())
        .with("severity", h.severity.unknown())
        .with("status", h.status.unknown())
        .with("confidence", h.confidence.unknown())
        .with(
            "freshness",
            h.freshness
                .get()
                .map(|f| format!("{} · source age {}", f.state.unknown(), age(f)))
                .unwrap_or_else(|| "unavailable".into()),
        )
        .with_opt("started", h.started_at.get())
        .with_opt("observed", h.last_observed_at.get())
        .with_opt("why", h.explanation.get())
        .with_opt("threshold", h.threshold.get())
        .with("policy", h.policy_version.or("not reported by source"))
        .with_opt("inspect", h.suggested_inspection.get())
        .with_opt("reason", h.indeterminate_reason.get());
        if let Some(c) = h.ceremony.get() {
            r.put("diagnosis stage", c.stage.unknown());
            r.put("assessment basis", c.basis.unknown());
        }
        if let Some(p) = h.operating_posture.get() {
            r.put(
                "posture",
                format!("{} · {}", p.posture.unknown(), p.source.unknown()),
            );
            r.put("oversight", p.reason.unknown());
        }
        r.put(
            "evidence",
            if h.evidence.is_empty() {
                "none served".to_owned()
            } else {
                join(
                    h.evidence.iter().map(|e| {
                        format!(
                            "{} at {}",
                            e.kind.unknown(),
                            e.observed_at.or("unknown time")
                        )
                    }),
                    "; ",
                )
            },
        );
        rows.push(r);
    }
    rows
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConfigEntry {
    pub key: Text,
    pub group: Text,
    pub value: Value,
    #[serde(rename = "defaultValue")]
    pub default_value: Value,
    #[serde(rename = "defaultKnown")]
    pub default_known: Lax<bool>,
    pub subject: Text,
    pub source: Text,
    pub visibility: Text,
    pub reason: Text,
    pub scope: Text,
    pub application: Text,
}

/// Category by key, as `configCategory` in config-model.ts.
pub fn config_category(e: &ConfigEntry) -> &'static str {
    if matches!(e.group.get(), Some("slack" | "people")) {
        return "Slack & people";
    }
    if e.group.is("hosts") {
        return "Instance & work";
    }
    if e.group.is("health") {
        return "Context & skills";
    }
    let key = e.key.unknown();
    let root = key.split('.').next().unwrap_or_default();
    match root {
        "daemon" | "host" | "db" | "workspace" | "files" | "progress" => "Instance & work",
        "topology" | "context" | "skills" | "onboarding" | "health" => "Context & skills",
        "ui" | "terminal" => "Display & terminals",
        "workflow" | "queue" => "Workflows & waiting",
        "recovery" | "snapshots" => "Recovery & snapshots",
        "transcripts" | "feed" | "retention" => "Activity & retention",
        "agents" | "runtime" => "Agents & runtime",
        "policies" if key.starts_with("policies.idle_gate_qitem.") => "Workflows & waiting",
        "policies" if key.starts_with("policies.claude_compaction.") => "Recovery & snapshots",
        _ => "All settings",
    }
}

/// `configValue` from config-model.ts.
pub fn config_value(e: &ConfigEntry, default_value: bool) -> String {
    if default_value && e.default_known.get() == Some(&false) {
        return "Not reported".into();
    }
    if e.visibility.is("unavailable") && !default_value {
        return "Unavailable".into();
    }
    if e.visibility.is("withheld") {
        return "Contents withheld".into();
    }
    let key = e.key.unknown();
    let v = if default_value {
        &e.default_value
    } else {
        &e.value
    };
    match v {
        Value::Null => "Unset".into(),
        Value::String(s) if s.is_empty() && key == "slack.outboundDestinations" => {
            "All registered humans".into()
        }
        Value::String(s) if s.is_empty() => "Unset".into(),
        Value::Bool(b)
            if [
                "credentialFile",
                "credentialReference",
                "bearer_env",
                "bearer_file",
            ]
            .iter()
            .any(|s| key.ends_with(s)) =>
        {
            (if *b { "Present" } else { "Missing" }).into()
        }
        Value::Bool(b) => (if *b { "On" } else { "Off" }).into(),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if key.ends_with("_seconds") || key.ends_with("Seconds") {
                if f >= 60. && f % 60. == 0. {
                    format!("{} min", js_number(f / 60.))
                } else {
                    format!("{} sec", js_number(f))
                }
            } else if key.ends_with("_minutes") {
                format!("{} min", js_number(f))
            } else if key.ends_with("_days") {
                format!("{} days", js_number(f))
            } else if key.ends_with("_percent") {
                format!("{}%", js_number(f))
            } else {
                js_number(f)
            }
        }
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn config_rows(v: &Value) -> Vec<Row> {
    let entries: Items<ConfigEntry> =
        serde_json::from_value(v["entries"].clone()).unwrap_or_default();
    let categories = [
        "Instance & work",
        "Context & skills",
        "Display & terminals",
        "Workflows & waiting",
        "Recovery & snapshots",
        "Activity & retention",
        "Agents & runtime",
        "Slack & people",
        "All settings",
    ];
    let mut rows = Vec::new();
    for category in categories {
        let of: Vec<_> = entries
            .iter()
            .filter(|e| config_category(e) == category)
            .collect();
        if of.is_empty() {
            continue;
        }
        push_group(
            &mut rows,
            Row::new(
                format!("config/{category}"),
                0,
                format!("{category} ({})", of.len()),
            ),
            |out| {
                for e in of {
                    let key = e.key.unknown();
                    let tone = if e.visibility.is("shown") {
                        Tone::Neutral
                    } else {
                        Tone::Dim
                    };
                    let value = config_value(e, false);
                    let mut r = Row::new(format!("config/{key}"), 1, key.clone())
                        .status(clip(&value, 32), tone)
                        .with("value", value)
                        .with("default", config_value(e, true))
                        .with("source", e.source.unknown())
                        .with("scope", e.scope.unknown())
                        .with("application", e.application.unknown())
                        .with_opt("subject", e.subject.get())
                        .with_opt("visibility", e.reason.get().or(e.visibility.get()));
                    r.put(
                        "override",
                        match e.source.get() {
                            Some("env") => "Environment override (above file and default)",
                            Some("file") => "File setting (above default)",
                            Some("default") => "Default; no override reported",
                            _ => "Not reported by this source",
                        },
                    );
                    out.push(r);
                }
            },
        );
    }
    let sources: Vec<Value> = v["sources"].as_array().cloned().unwrap_or_default();
    if !sources.is_empty() {
        let mut sr = Row::new("config/sources", 0, "Sources & coverage");
        for s in &sources {
            let text = |k: &str| s[k].as_str().unwrap_or("unreported");
            sr.put(
                text("id"),
                format!("{} · {} · {}", text("state"), text("path"), text("detail")),
            );
        }
        for e in v["exclusions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            sr.put("excluded", e);
        }
        rows.push(sr);
    }
    rows
}
