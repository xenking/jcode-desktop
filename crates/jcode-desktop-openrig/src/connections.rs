//! Connections: this daemon's instance, its Slack gateway, and the external humans registry
//! (`packages/tui/src/connections/connections-model.ts`, from `GET /api/gateway/connections`
//! plus `/healthz`). The projection is value-safe: raw config and connector errors never appear.
use crate::{
    Row, Tone, push_group,
    util::{Items, Lax, Text},
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Setting {
    pub key: Text,
    pub value: Text,
    pub source: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub state: Text,
    pub path: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChannelMapEntry {
    #[serde(rename = "match")]
    pub matches: Text,
    pub channel: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Configuration {
    pub enabled: Lax<bool>,
    pub channel: Text,
    pub inbound_destination: Text,
    pub post_level: Text,
    pub interrupt_level: Text,
    pub bot_token: Text,
    pub app_token: Text,
    pub channel_map: Items<ChannelMapEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Running {
    pub state: Text,
    pub activated_at: Text,
    pub outbound_ready: Lax<bool>,
    pub inbound_ready: Lax<bool>,
    pub inbound_state: Text,
    pub applied: Text,
    pub inbound_delivery: Text,
    pub inbound_events_missing_since: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Binding {
    pub kind: Text,
    #[serde(rename = "ref")]
    pub reference: Text,
    pub role: Text,
    pub handle: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Human {
    pub entity_id: Text,
    pub address: Text,
    pub display_name: Text,
    pub delivery_class: Text,
    pub availability: Text,
    pub excluded: Lax<bool>,
    pub bindings: Items<Binding>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Connections {
    pub observed_at: Text,
    pub home: Text,
    pub pid: Text,
    pub settings_source: Text,
    pub settings: Items<Setting>,
    pub config_source: Lax<Source>,
    pub configuration: Lax<Configuration>,
    pub running: Lax<Running>,
    pub state: Text,
    pub next_action: Text,
    pub registry: Lax<Source>,
    pub humans: Items<Human>,
}

/// `GET /healthz`, the running control plane.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ControlPlane {
    pub status: Text,
    pub semver: Text,
    pub commit: Text,
    pub dirty: Lax<bool>,
    pub built_at: Text,
    pub self_host_id: Text,
    pub self_host_id_source: Text,
}

/// An open socket is not proof that events arrive, so the delivery word is named beside the state.
/// A daemon too old to report delivery leaves the bare state.
pub fn inbound_value(r: &Running) -> String {
    let state = r.inbound_state.unknown();
    match r.inbound_delivery.get() {
        Some("events-missing") => format!(
            "{state}; events missing since {}",
            r.inbound_events_missing_since.unknown()
        ),
        Some("no-server-pings") => format!("{state}; no server pings"),
        Some("socket-mode-disabled") => {
            format!("{state}; Socket Mode disabled in the Slack app settings")
        }
        Some("delivering") if r.inbound_state.is("connected") => "connected; delivering".into(),
        Some("unknown") if r.inbound_state.is("connected") => {
            "connected; delivery not yet confirmed".into()
        }
        _ => state,
    }
}

/// No Slack app yet: neither token resolves. Unknown token state is not "not configured".
pub fn slack_not_configured(c: &Connections) -> bool {
    c.configuration
        .get()
        .is_some_and(|c| c.bot_token.is("missing") && c.app_token.is("missing"))
}

fn state_tone(state: &Text) -> Tone {
    match state.get() {
        Some("ready" | "running" | "connected" | "enabled") => Tone::Ok,
        Some("disabled" | "not-configured") => Tone::Dim,
        Some("error" | "failed" | "degraded") => Tone::Error,
        Some(_) => Tone::Warn,
        None => Tone::Dim,
    }
}

fn token_tone(token: &Text) -> Tone {
    match token.get() {
        Some("missing") => Tone::Warn,
        Some(_) => Tone::Neutral,
        None => Tone::Dim,
    }
}

fn instance_row(c: &Connections, h: Option<&ControlPlane>, daemon_target: &str) -> Row {
    let mut row = Row::new("conn/instance", 0, "Running control plane").open();
    row.put(
        "daemon",
        match h {
            Some(h) => format!(
                "{} · {} · {}{}",
                h.status.unknown(),
                h.semver.or("version unstamped"),
                h.commit.or("commit unstamped"),
                if h.dirty.truthy() { " · dirty" } else { "" }
            ),
            None => "unavailable: check rig status".into(),
        },
    );
    row.put(
        "host",
        match h.and_then(|h| h.self_host_id.get().map(|id| (id, &h.self_host_id_source))) {
            Some((id, source)) => format!("{id} ({})", source.or("source unreported")),
            None => "identity unreported".into(),
        },
    );
    row.put("target", daemon_target);
    row.put(
        "process",
        format!("PID {} · home {}", c.pid.unknown(), c.home.or("unreported")),
    );
    row.put("observed", c.observed_at.unknown());
    row
}

fn settings_row(c: &Connections) -> Row {
    let mut row = Row::new("conn/settings", 0, "Instance settings · resolved values")
        .with("settings", c.settings_source.or("source unavailable"));
    for s in &c.settings {
        let key = s.key.unknown();
        let label = match key.as_str() {
            "host.name" => "display name".to_owned(),
            "workspace.root" => "workspace".to_owned(),
            _ => "operator".to_owned(),
        };
        row.put(
            &label,
            format!("{} ({})", s.value.or("unavailable"), s.source.unknown()),
        );
    }
    row.put(
        "precedence",
        "env overrides file overrides default. These are current settings, not proof of runtime adoption.",
    );
    row.put(
        "inspect",
        "rig config --with-source; rig config set <key> <value>",
    );
    row
}

fn slack_row(c: &Connections) -> Row {
    let running = c.running.get().cloned().unwrap_or_default();
    let mut row = Row::new(
        "conn/slack",
        0,
        "Slack · configuration and running services",
    )
    .status(c.state.unknown(), state_tone(&c.state))
    .open()
    .with(
        "delivery",
        format!(
            "{} · current external reach is unverified",
            c.state.unknown()
        ),
    )
    .with(
        "source",
        c.config_source
            .get()
            .map(|s| format!("{} · {}", s.state.unknown(), s.path.or("unreported")))
            .unwrap_or_else(|| "unknown · unreported".into()),
    )
    .with(
        "gateway",
        format!(
            "{} · configuration {}",
            running.state.unknown(),
            running.applied.unknown()
        ),
    );
    if running.applied.is("changed") {
        row.put(
            "warning",
            "Config changed since the wire was built. Inspect before a supported restart; do not assume applied.",
        );
        row.tone = Tone::Warn;
    }
    if slack_not_configured(c) {
        row.put("setup", "not configured · no Slack app tokens yet");
        row.put(
            "create app",
            "Create your own private Slack app from OpenRig's manifest: rig slack manifest --url. Then: rig slack setup, rig slack verify, rig slack enable.",
        );
    }
    if let Some(cfg) = c.configuration.get() {
        row.put(
            "enabled",
            cfg.enabled
                .get()
                .map_or("unknown".into(), |b| b.to_string()),
        );
        row.put("channel", cfg.channel.or("missing"));
        for e in &cfg.channel_map {
            row.put(
                "channel map",
                format!(
                    "{} → {}",
                    e.matches.or("withheld"),
                    e.channel.or("withheld")
                ),
            );
        }
        row.put(
            "credentials",
            format!(
                "bot {}; Socket Mode app {} (values hidden)",
                cfg.bot_token.unknown(),
                cfg.app_token.unknown()
            ),
        );
        row.put(
            "outbound",
            match running.outbound_ready.get() {
                None => "unreported".to_owned(),
                Some(ready) => format!(
                    "{}; posting >= {}, interrupting >= {} (current config)",
                    if *ready {
                        "configured at activation"
                    } else {
                        "not configured at activation"
                    },
                    cfg.post_level.unknown(),
                    cfg.interrupt_level.unknown()
                ),
            },
        );
        row.put("inbound", inbound_value(&running));
        row.put("new inbound", cfg.inbound_destination.or("missing"));
        row.put(
            "replies",
            "Replies follow their existing conversation; new/unmapped inbound uses the configured seat above.",
        );
        if cfg.bot_token.is("missing") || cfg.app_token.is("missing") {
            row.tone = Tone::Warn;
        } else if token_tone(&cfg.bot_token) == Tone::Dim {
            row.tone = Tone::Dim;
        }
    }
    row.put("next", c.next_action.unknown());
    row
}

fn human_row(c: &Connections, h: &Human, index: usize) -> Row {
    let route = match h.excluded.get() {
        Some(true) => "excluded by outbound policy".to_owned(),
        None => "route eligibility unknown".to_owned(),
        Some(false) => format!("route uses instance Slack · {}", c.state.unknown()),
    };
    let mut row = Row::new(
        format!("conn/human/{index}"),
        1,
        format!(
            "{} · {}",
            h.display_name.or(&h.entity_id.unknown()),
            h.address.unknown()
        ),
    )
    .status(
        h.availability.unknown(),
        match h.excluded.get() {
            Some(true) => Tone::Dim,
            None => Tone::Warn,
            Some(false) => Tone::Neutral,
        },
    )
    .with("route", route)
    .with("class", h.delivery_class.unknown())
    .with("availability", h.availability.unknown());
    for b in &h.bindings {
        row.put(
            &b.role.unknown(),
            format!(
                "{} {} · handle {}",
                b.kind.unknown(),
                b.reference.or("unreported"),
                b.handle.or("absent (outbound only)")
            ),
        );
    }
    row.put(
        "inspect",
        format!(
            "rig gateway human show {} --json (contacts Slack)",
            h.entity_id.unknown()
        ),
    );
    row
}

pub fn connections_rows(
    connections: Result<&Value, &str>,
    health: Option<&Value>,
    daemon_target: &str,
) -> Vec<Row> {
    let health: Option<ControlPlane> = health.and_then(|h| serde_json::from_value(h.clone()).ok());
    let c: Connections = match connections {
        Ok(v) if v.is_object() => serde_json::from_value(v.clone()).unwrap_or_default(),
        other => {
            let mut row = Row::new("conn/unavailable", 0, "Connections unavailable")
                .status("unavailable", Tone::Warn)
                .with("reason", "the daemon could not serve this view")
                .with("next", "rig status; rig --version; rig daemon logs")
                .with(
                    "note",
                    "An older daemon may not support Connections. No readiness inferred.",
                );
            if let Err(e) = other {
                row.put("error", e);
            }
            return vec![row];
        }
    };
    let registry_available = c.registry.get().is_some_and(|r| r.state.is("available"));
    let mut rows = vec![
        Row::new(
            "conn/summary",
            0,
            format!(
                "Slack: {} · humans: {}",
                c.state.unknown(),
                if registry_available {
                    c.humans.len().to_string()
                } else {
                    "unknown".into()
                }
            ),
        )
        .status(c.state.unknown(), state_tone(&c.state))
        .with("next", c.next_action.unknown())
        .with("view", "Passive view · refresh sends nothing"),
        instance_row(&c, health.as_ref(), daemon_target),
        settings_row(&c),
        slack_row(&c),
    ];
    push_group(
        &mut rows,
        Row::new("conn/humans", 0, "External humans · instance-wide registry")
            .open()
            .with(
                "note",
                "Primary is the declared default binding; registration alone assigns no rig and proves no reachability.",
            ),
        |out| {
            if !registry_available {
                out.push(
                    Row::new("conn/humans/unavailable", 1, "Registry unavailable; recipients unknown")
                        .status("unavailable", Tone::Warn)
                        .with("next", "rig gateway human list --json"),
                );
            } else if c.humans.is_empty() {
                out.push(
                    Row::new("conn/humans/none", 1, "No registered humans")
                        .with("next", "rig gateway human add --help"),
                );
            }
            for (i, h) in c.humans.iter().enumerate() {
                out.push(human_row(&c, h, i));
            }
        },
    );
    rows
}
