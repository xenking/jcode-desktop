//! TUI parity for the OpenRig sections, against fixture JSON shaped like the daemon's read models
//! (`packages/tui/test/usage-journey.test.ts` and the models under `packages/tui/src`).
use jcode_desktop_openrig::{
    ReadFailure, Row, Tone, connections, cost,
    sections::{self, Node},
    usage::{self, SwitchSetting},
    util::Items,
};
use serde_json::{Value, json};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("valid fixture")
}
fn detail<'a>(row: &'a Row, key: &str) -> Option<&'a str> {
    row.detail
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}
fn find<'a>(rows: &'a [Row], label: &str) -> &'a Row {
    rows.iter().find(|r| r.label == label).unwrap_or_else(|| {
        panic!(
            "no row {label:?} in {:?}",
            rows.iter().map(|r| &r.label).collect::<Vec<_>>()
        )
    })
}
fn failure(status: Option<u16>, error: &str) -> ReadFailure {
    ReadFailure {
        status,
        error: error.into(),
        message: None,
    }
}

#[test]
fn usage_is_provider_allowance_window() {
    let rows = usage::usage_rows(&Ok(fixture("usage")), &SwitchSetting::Absent);
    let codex = find(&rows, "codex");
    let claude = find(&rows, "claude");
    assert_eq!((codex.depth, claude.depth), (1, 1));
    // codex has three allowances (two identified, one with no identity yet), claude one.
    assert_eq!(codex.children_count, 3);
    assert_eq!(claude.children_count, 1);
    let allowance = find(&rows, "codex · account acct-1");
    assert_eq!(allowance.depth, 2);
    assert_eq!(allowance.children_count, 2);
    assert!(allowance.default_open);
    assert_eq!(detail(allowance, "bucket"), Some("codex"));
    assert_eq!(detail(allowance, "email"), Some("a@x.example"));
    assert_eq!(detail(allowance, "plan"), Some("plus"));
    assert_eq!(detail(allowance, "seats"), Some("unknown"));
    assert_eq!(
        detail(allowance, "refresh failed"),
        Some(
            "at 2026-10-13T17:58:00.000Z: timeout (no answer within 35 s); last reading kept, marked old"
        )
    );
    assert_eq!(
        detail(allowance, "last observed exhausted"),
        Some("at 2026-10-12T08:00:00.000Z (secondary: provider rejected the window)")
    );
    assert_eq!(
        detail(allowance, "sign-in"),
        Some("needs sign-in (401 on refresh; as of 2026-10-13T17:58:00.000Z)")
    );
    assert_eq!(
        detail(allowance, "saved rig auth profiles: not read"),
        Some("work-a, work-b")
    );
}

#[test]
fn usage_windows_carry_progress_and_served_words() {
    let rows = usage::usage_rows(&Ok(fixture("usage")), &SwitchSetting::Absent);
    let five_hours = rows
        .iter()
        .find(|r| r.label == "5h (primary)")
        .expect("5h window");
    assert_eq!(five_hours.depth, 3);
    let meter = five_hours.meter.as_ref().expect("meter");
    assert_eq!(meter.label, "8% left");
    assert_eq!(meter.fraction(), Some(0.08));
    assert_eq!(meter.tone, Tone::Warn);
    assert_eq!(five_hours.status.as_deref(), Some("nearing"));
    assert_eq!(
        detail(five_hours, "level"),
        Some("nearing (80% or more used)")
    );
    assert_eq!(
        detail(five_hours, "freshness"),
        Some("stale: refresh failed at 2026-10-13T17:58:00.000Z; this reading is older")
    );
    assert_eq!(
        detail(five_hours, "resets"),
        Some("2026-10-13T19:00:00.000Z")
    );
    let weekly = find(&rows, "weekly (secondary)");
    assert_eq!(weekly.meter.as_ref().and_then(|m| m.fraction()), Some(0.0));
    assert_eq!(weekly.meter.as_ref().map(|m| m.tone), Some(Tone::Error));
    assert_eq!(weekly.status.as_deref(), Some("exhausted"));
}

#[test]
fn usage_claude_windows_name_seats_and_disagreement() {
    let rows = usage::usage_rows(&Ok(fixture("usage")), &SwitchSetting::Absent);
    let claude = find(&rows, "claude · identity unresolved");
    assert_eq!(detail(claude, "earlier read"), None);
    assert_eq!(
        detail(claude, "identity note"),
        Some("identity not read yet")
    );
    assert_eq!(
        detail(claude, "kept reads"),
        Some("kept from the 3 most recent unresolved reads (1 older dropped)")
    );
    let window = find(&rows, "5h (five_hour)");
    assert_eq!(detail(window, "by seat"), Some("dev-build@r"));
    assert_eq!(
        detail(window, "seat dev-r1@r disagrees"),
        Some(
            "reports 10% used · resets 2026-10-13T21:00:00.000Z · observed 2026-10-13T17:59:00.000Z"
        )
    );
    assert_eq!(
        detail(window, "expected"),
        Some("full since 2026-10-13T17:00:00.000Z, not yet observed (planning, not a reading)")
    );
}

#[test]
fn usage_marks_rows_kept_from_earlier_reads() {
    let rows = usage::usage_rows(&Ok(fixture("usage")), &SwitchSetting::Absent);
    let old = find(&rows, "codex · account acct-0");
    assert_eq!(
        detail(old, "earlier read"),
        Some("from an earlier read; the latest read didn't match this row")
    );
    assert_eq!(old.tone, Tone::Dim);
    let unread = find(&rows, "codex");
    assert_eq!(unread.depth, 1);
    let nothing = rows
        .iter()
        .find(|r| detail(r, "unavailable").is_some_and(|d| d.starts_with("no reading yet")))
        .expect("identity-less row");
    assert_eq!(nothing.status.as_deref(), Some("unavailable"));
    assert_eq!(nothing.children_count, 0);
}

#[test]
fn usage_seats_show_configuration_and_observation() {
    let rows = usage::usage_rows(&Ok(fixture("usage")), &SwitchSetting::Absent);
    let qa = find(&rows, "dev-qa@r");
    assert_eq!(
        detail(qa, "configured"),
        Some(
            "profile work-a (registry, recorded 2026-10-12T10:00:00Z; whether the profile is saved: unknown)"
        )
    );
    assert_eq!(
        detail(qa, "registry note"),
        Some("can't tell whether profile work-a is saved on this host")
    );
    assert_eq!(
        detail(qa, "observed"),
        Some("no per-seat observation for Codex")
    );
    assert_eq!(detail(qa, "allowance"), Some("not linked"));
    let build = find(&rows, "dev-build@r");
    assert_eq!(
        detail(build, "configured"),
        Some("default login (/h/.claude)")
    );
    assert_eq!(
        detail(build, "home"),
        Some("the login in the daemon's Claude config dir")
    );
    assert_eq!(
        detail(build, "observed"),
        Some("this seat's status line last reported windows at 2026-10-13T17:59:30.000Z")
    );
    assert_eq!(
        detail(build, "differs"),
        Some("differs from dev-r1@r (possibly a different login)")
    );
    assert_eq!(build.tone, Tone::Warn);
    assert_eq!(
        detail(find(&rows, "t@r"), "configured"),
        Some("unknown (no account configuration is known for runtime terminal)")
    );
}

#[test]
fn usage_absent_model_says_why() {
    let no_usage = Ok(json!({"asOf": "x"}));
    let off = usage::usage_rows(
        &no_usage,
        &SwitchSetting::Value {
            value: serde_json::from_value(json!(false)).unwrap(),
            source: serde_json::from_value(json!("default")).unwrap(),
        },
    );
    let row = find(&off, "No usage windows served");
    assert!(row.detail.iter().any(|(_, v)| v
        == "Usage windows are off on this daemon (provider.usage_windows = false, default)."));
    assert!(
        row.detail
            .iter()
            .any(|(_, v)| v == "Turn them on with: rig config set provider.usage_windows true")
    );
    assert_eq!(
        find(
            &off,
            "Seat accounts are served only while usage windows are"
        )
        .depth,
        1
    );

    let older = usage::usage_rows(&no_usage, &SwitchSetting::Absent);
    assert!(
        find(&older, "No usage windows served")
            .detail
            .iter()
            .any(|(_, v)| v.contains("doesn't know the provider.usage_windows setting"))
    );
    let unread = usage::usage_rows(&no_usage, &SwitchSetting::Unread);
    assert!(
        find(&unread, "No usage windows served")
            .detail
            .iter()
            .any(|(_, v)| v.contains("couldn't be read"))
    );
}

#[test]
fn usage_failures_are_unavailable_not_empty() {
    let not_found = usage::usage_rows(&Err(failure(Some(404), "x")), &SwitchSetting::Unread);
    assert_eq!(
        detail(&not_found[0], "reason"),
        Some("This daemon doesn't serve Usage windows (no read model: HTTP 404).")
    );
    let down = usage::usage_rows(
        &Err(failure(None, "connection failed")),
        &SwitchSetting::Unread,
    );
    assert_eq!(down[0].status.as_deref(), Some("unavailable"));
    assert!(
        detail(&down[0], "reason")
            .unwrap()
            .contains("Nothing is inferred from the missing read.")
    );
}

#[test]
fn switch_setting_reads_the_config_entry() {
    let config =
        json!({"entries": [{"key": "provider.usage_windows", "value": true, "source": "file"}]});
    match usage::switch_setting(Some(&config)) {
        SwitchSetting::Value { value, source } => {
            assert!(value.is("true"));
            assert!(source.is("file"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        usage::switch_setting(Some(&json!({"entries": []}))),
        SwitchSetting::Absent
    );
    assert_eq!(usage::switch_setting(None), SwitchSetting::Unread);
}

#[test]
fn identity_labels_follow_the_tui() {
    let label = |v: Value| usage::identity_label(&serde_json::from_value(v).unwrap());
    assert_eq!(
        label(json!({"source": "configured credential label", "credentialLabel": "ops"})),
        "credential ops (operator label)"
    );
    assert_eq!(
        label(json!({"workspaceId": "w1"})),
        "workspace w1 (from account/read; no usage account id)"
    );
    assert_eq!(
        label(json!({"orgId": "o1", "orgName": "Acme"})),
        "organisation Acme (o1)"
    );
    assert_eq!(
        label(json!({"orgId": "o1", "source": "unresolved"})),
        "identity unresolved"
    );
}

#[test]
fn usage_hosts_group_allowances_per_host() {
    let rows = usage::usage_hosts_rows(&Ok(fixture("usage_hosts")));
    let local = find(&rows, "local");
    assert_eq!(local.status.as_deref(), Some("answered"));
    assert_eq!(detail(local, "usage"), Some("usage windows served"));
    assert_eq!(
        detail(local, "refresh from here (codex)"),
        Some(
            "failed at 2026-10-13T17:00:00.000Z: timeout; this host's earlier observations are kept"
        )
    );
    let remote = find(&rows, "mac-2");
    assert_eq!(
        remote.status.as_deref(),
        Some("unreachable (connect refused)")
    );
    assert_eq!(
        detail(remote, "usage"),
        Some("usage unavailable: host did not answer")
    );
    assert_eq!(
        detail(find(&rows, "old-box"), "skipped"),
        Some("no pairing")
    );
    assert_eq!(
        find(&rows, "Host registry").status.as_deref(),
        Some("unreadable")
    );

    let group = find(&rows, "codex · account acct-1 · bucket codex");
    assert_eq!(group.tone, Tone::Error);
    assert_eq!(
        group.status.as_deref(),
        Some("the same allowance on 2 hosts, disagreeing: local, mac-2")
    );
    assert_eq!(
        detail(group, "note"),
        Some("hosts disagree on the weekly window")
    );
    assert_eq!(group.children_count, 2);
    let on_local = find(&rows, "on local (read 2026-10-13T18:00:00.000Z)");
    assert_eq!(on_local.depth, 2);
    // Each host's observation keeps its own window: 70% and 36% left are never merged.
    let lefts: Vec<_> = rows
        .iter()
        .filter(|r| r.label == "weekly (secondary)")
        .filter_map(|r| r.meter.as_ref().and_then(|m| m.fraction()))
        .collect();
    assert_eq!(lefts, vec![0.7, 0.36]);
}

#[test]
fn usage_hosts_seats_prefer_the_difference() {
    let rows = usage::usage_hosts_rows(&Ok(fixture("usage_hosts")));
    let section = find(&rows, "Seats on local");
    assert_eq!(section.children_count, 1);
    let seat = find(&rows, "dev-build@r");
    // On the all-hosts page the served difference wins over the withheld note.
    assert_eq!(
        detail(seat, "observed"),
        Some("differs from dev-r1@r (different login)")
    );
    assert_eq!(detail(seat, "configured"), Some("default login"));
    assert_eq!(find(&rows, "No seats served").depth, 1);
}

#[test]
fn usage_hosts_failures_use_the_daemons_words() {
    let conflict = ReadFailure {
        status: Some(409),
        error: "e".into(),
        message: Some("usage windows are off here".into()),
    };
    let rows = usage::usage_hosts_rows(&Err(conflict));
    assert_eq!(
        detail(&rows[0], "reason"),
        Some("usage windows are off here")
    );
    let missing = usage::usage_hosts_rows(&Err(failure(Some(404), "x")));
    assert_eq!(
        detail(&missing[0], "reason"),
        Some("This daemon doesn't serve usage across hosts (HTTP 404).")
    );
}

#[test]
fn cost_lines_match_rig_usage_cost() {
    let rows = cost::cost_rows(&Ok(fixture("cost")));
    assert_eq!(rows[0].label, "Seats · 5 seats");
    assert_eq!(detail(&rows[0], "note"), Some("estimates only"));
    let claude = find(&rows, "dev-claude@r");
    assert_eq!(claude.status.as_deref(), Some("reported"));
    assert_eq!(claude.tone, Tone::Warn);
    assert_eq!(
        detail(claude, "summary"),
        Some(
            "$1.23 client-side estimate (Claude statusline) for session sess-1 since 2026-10-13T10:00:00Z, as of 2026-10-13T17:00:00Z, 2 resets, STALE (900s old; kept as history)  tokens in 1200 / out 340"
        )
    );
    assert_eq!(
        detail(claude, "resets in this session"),
        Some("2 · stale: 900s old, kept as history")
    );
    assert_eq!(
        detail(claude, "window"),
        Some("native session sess-1 · since 2026-10-13T10:00:00Z · as of 2026-10-13T17:00:00Z")
    );
}

#[test]
fn cost_pi_seat_keeps_estimate_and_charge_apart() {
    let rows = cost::cost_rows(&Ok(fixture("cost")));
    let pi = find(&rows, "dev-pi@r");
    let summary = detail(pi, "summary").unwrap();
    assert!(summary.starts_with("<$0.01 model-priced estimate (Pi persisted entries, 4 own entries) for Pi session pi-9 since 2026-10-13T09:00:00Z, last 2026-10-13T12:00:00Z; 1 entry without a cost; 1 error/aborted turn included; 2 inherited from the fork, not counted ($0.50); "));
    assert!(summary.contains("OpenRouter charge <$0.01 USD for 3 of 4 requests via env, Pi estimate for them <$0.01; 1 pending, 2 unavailable (timeout), 1 not covered (provider-reported, not added to the estimate)"));
    assert!(summary.ends_with("tokens in 50 / out 20"));
    assert_eq!(
        detail(pi, "own entries by kind"),
        Some("assistant 3 · tool 1")
    );
    assert_eq!(
        detail(pi, "entries"),
        Some(
            "own 4 (counted; 1 without a cost, 1 error/aborted) · inherited 2 ($0.50) and unknown origin 0: shown apart, not counted"
        )
    );
    assert_eq!(
        detail(pi, "tokens"),
        Some("in 50 · out 20 · cache read 5 · cache write 0")
    );
    assert_eq!(
        detail(pi, "provider charge"),
        Some(
            "OpenRouter charge, beside the estimate and never added to it: partial · <$0.01 USD (Pi's estimate for the same requests: <$0.01)"
        )
    );
    assert_eq!(
        detail(pi, "requests"),
        Some("5 OpenRouter · 4 covered · 3 charged · 1 pending · 2 unavailable · 1 not covered")
    );
    assert_eq!(detail(pi, "unavailable: timeout"), Some("2"));
    assert_eq!(detail(pi, "not covered: byok"), Some("1"));
    assert_eq!(
        detail(pi, "account context"),
        Some("credential env · workspace not reported · BYOK requests 1")
    );
    assert_eq!(detail(pi, "charge coverage"), Some("3 of 4 covered"));
}

#[test]
fn cost_missing_figures_are_never_zero() {
    let rows = cost::cost_rows(&Ok(fixture("cost")));
    assert_eq!(
        detail(find(&rows, "dev-codex@r"), "summary"),
        Some("cost: not reported (no cost reported)  tokens in 9 / out ?")
    );
    assert_eq!(
        detail(find(&rows, "term@r"), "summary"),
        Some("tokens in ? / out ?  cost: no cost method for this runtime")
    );
    assert_eq!(
        detail(find(&rows, "odd@r"), "summary"),
        Some("cost: unknown (not understood)")
    );
    assert_eq!(cost::format_usd(0.004), "<$0.01");
    assert_eq!(cost::format_usd(0.), "$0.00");
    assert_eq!(cost::format_usd(2.5), "$2.50");
}

#[test]
fn cost_malformed_body_is_unavailable() {
    for body in [
        json!({"seats": [{"seatSession": "a"}]}),
        json!({"seats": "x"}),
        json!([]),
    ] {
        let rows = cost::cost_rows(&Ok(body));
        assert_eq!(rows[0].label, "Cost unavailable");
        assert!(
            detail(&rows[0], "reason")
                .unwrap()
                .contains("the response was not a cost view")
        );
    }
    let empty = cost::cost_rows(&Ok(json!({"seats": []})));
    assert!(
        empty
            .iter()
            .any(|r| r.label.starts_with("no usage samples yet"))
    );
    let missing = cost::cost_rows(&Err(failure(Some(404), "x")));
    assert!(detail(&missing[0], "reason").unwrap().contains("HTTP 404"));
}

fn conn_rows(connections: &Value) -> Vec<Row> {
    connections::connections_rows(
        Ok(connections),
        Some(&fixture("healthz")),
        "http://127.0.0.1:7433",
    )
}

#[test]
fn connections_cover_instance_slack_and_humans() {
    let rows = conn_rows(&fixture("connections"));
    assert_eq!(rows[0].label, "Slack: degraded · humans: 2");
    assert_eq!(detail(&rows[0], "next"), Some("rig slack verify"));
    let instance = find(&rows, "Running control plane");
    assert_eq!(
        detail(instance, "daemon"),
        Some("ok · 0.4.2 · abc1234 · dirty")
    );
    assert_eq!(detail(instance, "host"), Some("studio (settings)"));
    assert_eq!(detail(instance, "target"), Some("http://127.0.0.1:7433"));
    assert_eq!(
        detail(instance, "process"),
        Some("PID 4242 · home /h/.openrig")
    );
    let settings = find(&rows, "Instance settings · resolved values");
    assert_eq!(detail(settings, "display name"), Some("studio (file)"));
    assert_eq!(detail(settings, "workspace"), Some("unavailable (default)"));
    let slack = find(&rows, "Slack · configuration and running services");
    assert_eq!(
        detail(slack, "gateway"),
        Some("running · configuration changed")
    );
    assert_eq!(
        detail(slack, "source"),
        Some("readable · /h/.openrig/slack.json")
    );
    assert_eq!(
        detail(slack, "credentials"),
        Some("bot present; Socket Mode app missing (values hidden)")
    );
    assert_eq!(
        detail(slack, "inbound"),
        Some("connected; events missing since 2026-10-13T17:00:00Z")
    );
    assert_eq!(
        detail(slack, "outbound"),
        Some(
            "configured at activation; posting >= info, interrupting >= critical (current config)"
        )
    );
    assert_eq!(detail(slack, "new inbound"), Some("orch@r"));
    assert_eq!(slack.tone, Tone::Warn);
    let maps: Vec<_> = slack
        .detail
        .iter()
        .filter(|(k, _)| k == "channel map")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(maps, vec!["rig-a → #rig-a", "withheld → withheld"]);
}

#[test]
fn connections_humans_show_routes_and_bindings() {
    let rows = conn_rows(&fixture("connections"));
    assert_eq!(
        find(&rows, "External humans · instance-wide registry").children_count,
        2
    );
    let ada = find(&rows, "Ada · human-ada@host");
    assert_eq!(
        detail(ada, "route"),
        Some("route uses instance Slack · degraded")
    );
    assert_eq!(detail(ada, "class"), Some("interactive"));
    assert_eq!(detail(ada, "primary"), Some("slack U123 · handle @ada"));
    assert_eq!(
        detail(ada, "secondary"),
        Some("email unreported · handle absent (outbound only)")
    );
    let bo = find(&rows, "h-2 · human-bo@host");
    assert_eq!(detail(bo, "route"), Some("route eligibility unknown"));
    assert_eq!(bo.tone, Tone::Warn);
}

#[test]
fn connections_unconfigured_slack_and_unavailable_registry() {
    let mut v = fixture("connections");
    v["configuration"]["botToken"] = json!("missing");
    v["registry"] = json!({"state": "unavailable", "path": null});
    let rows = conn_rows(&v);
    assert_eq!(rows[0].label, "Slack: degraded · humans: unknown");
    let slack = find(&rows, "Slack · configuration and running services");
    assert_eq!(
        detail(slack, "setup"),
        Some("not configured · no Slack app tokens yet")
    );
    assert!(
        find(&rows, "Registry unavailable; recipients unknown")
            .detail
            .iter()
            .any(|(_, v)| v.contains("rig gateway human list"))
    );
    assert!(connections::slack_not_configured(
        &serde_json::from_value(v).unwrap()
    ));
}

#[test]
fn connections_unavailable_does_not_infer_readiness() {
    let rows = connections::connections_rows(Err("HTTP 404"), None, "unreported");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "Connections unavailable");
    assert!(
        detail(&rows[0], "note")
            .unwrap()
            .contains("No readiness inferred")
    );
    assert_eq!(detail(&rows[0], "error"), Some("HTTP 404"));
}

#[test]
fn inbound_delivery_words() {
    let run = |v: Value| connections::inbound_value(&serde_json::from_value(v).unwrap());
    assert_eq!(
        run(json!({"inboundState": "connected", "inboundDelivery": "delivering"})),
        "connected; delivering"
    );
    assert_eq!(
        run(json!({"inboundState": "connected", "inboundDelivery": "unknown"})),
        "connected; delivery not yet confirmed"
    );
    assert_eq!(
        run(json!({"inboundState": "connected", "inboundDelivery": "no-server-pings"})),
        "connected; no server pings"
    );
    assert_eq!(run(json!({"inboundState": "closed"})), "closed");
}

#[test]
fn seats_derive_status_context_and_work() {
    let nodes: Items<Node> = serde_json::from_value(fixture("nodes")).unwrap();
    let pods = Node::by_pod(nodes.as_slice());
    // Infrastructure nodes are not listed; a seat with no pod sits under "(no pod)".
    assert_eq!(
        pods.iter()
            .map(|(p, a)| (p.as_str(), a.len()))
            .collect::<Vec<_>>(),
        vec![("(no pod)", 1), ("eng", 2)]
    );
    let seats: Vec<Row> = nodes.iter().map(|n| n.row(3, "x")).collect();
    let builder = &seats[0];
    assert_eq!(builder.status.as_deref(), Some("active"));
    assert_eq!(detail(builder, "effort"), Some("high (authored)"));
    assert_eq!(detail(builder, "context"), Some("82%"));
    assert_eq!(detail(builder, "tokens"), Some("162k"));
    assert_eq!(
        detail(builder, "work"),
        Some("assigned 3 · pending 2 · blocked 1")
    );
    assert_eq!(detail(builder, "session"), Some("builder@eng"));
    let meter = builder.meter.as_ref().expect("context meter");
    assert_eq!(meter.label, "context 82%");
    assert_eq!(meter.tone, Tone::Warn);
    let reviewer = &seats[1];
    assert_eq!(reviewer.status.as_deref(), Some("needs_input"));
    assert_eq!(detail(reviewer, "context"), Some("—"));
    assert!(reviewer.meter.is_none());
    assert_eq!(seats[2].status.as_deref(), Some("detached"));
}

#[test]
fn needs_items_carry_derived_evidence() {
    let items: Items<sections::NeedsItem> = serde_json::from_value(json!([
        {"source": "agent", "identity": "q1", "summary": "approve the plan", "leg": "approval", "where": "r", "destinationSession": "human-ada@host", "qitemId": "qitem-1", "unblocks": "dev@r"},
        {"source": "derived", "identity": "dev@r|idle", "summary": "idle with work", "leg": "stuck", "where": "r", "destinationSession": null,
         "derived": {"kind": "idle-with-work", "evidence": "12m idle", "threshold": "10m"}, "hostId": "mac-2"}
    ])).unwrap();
    let agent = sections::needs_row(&items.as_slice()[0], 0);
    assert_eq!(detail(&agent, "target"), Some("human-ada@host"));
    assert_eq!(detail(&agent, "queue item"), Some("qitem-1"));
    assert_eq!(detail(&agent, "unblocks"), Some("dev@r"));
    let derived = sections::needs_row(&items.as_slice()[1], 1);
    assert_eq!(derived.status.as_deref(), Some("idle-with-work"));
    assert_eq!(detail(&derived, "target"), Some("dev@r"));
    assert_eq!(
        detail(&derived, "detail"),
        Some("idle with work — 12m idle")
    );
    assert_eq!(detail(&derived, "threshold"), Some("10m"));
    assert_eq!(detail(&derived, "host"), Some("mac-2"));
}

#[test]
fn scopes_slices_show_proof_and_locks() {
    let rows = sections::scopes_rows(&json!({"missions": [
        {"mission": "m1", "slices": [
            {"dirName": "01-a", "id": "s1", "displayName": "Slice A", "status": "building", "stage": "building", "locks": {"spec": {"by": "ada", "at": "t1"}, "delivery": null},
             "proof": {"paired": 4, "total": 6}, "intent": "do a", "miniRequirements": ["r1", "r2"]},
            {"dirName": "02-b", "displayName": "Slice B", "status": "done", "locks": {"spec": null, "delivery": {"by": "bo", "at": "t2"}}, "proof": {"paired": 2, "total": 2}},
            {"dirName": "03-c", "error": "unreadable"}
        ]},
        {"mission": "m2", "error": "missing", "slices": []}
    ]}));
    assert_eq!(rows[0].label, "m1");
    assert_eq!(rows[0].children_count, 3);
    let a = find(&rows, "Slice A");
    assert_eq!(detail(a, "proof"), Some("proof: 4 / 6 paired"));
    assert_eq!(detail(a, "spec lock"), Some("ada at t1"));
    assert_eq!(detail(a, "requirement 2"), Some("r2"));
    assert!((a.meter.as_ref().unwrap().fraction().unwrap() - 4. / 6.).abs() < 1e-6);
    let b = find(&rows, "Slice B");
    assert_eq!(detail(b, "proof"), Some("proof: 2 / 2 paired 🔒"));
    assert_eq!(b.tone, Tone::Ok);
    assert_eq!(find(&rows, "03-c").tone, Tone::Error);
    assert_eq!(find(&rows, "m2").status.as_deref(), Some("unavailable"));
}

#[test]
fn health_findings_sort_and_word_unknown_honestly() {
    let rows = sections::health_rows(
        &json!({"evaluatedAt": "t", "total": 3, "truncated": true, "records": [
        {"id": "f-info", "detector": "d1", "category": "process", "scope": {"type": "instance", "instanceId": "studio"}, "severity": "info", "status": "active",
         "freshness": {"state": "fresh", "ageSeconds": 30}, "summary": "all quiet"},
        {"id": "f-crit", "detector": "d2", "category": "behavioral", "scope": {"type": "seat", "rigId": "r", "seatId": "n1"}, "severity": "critical", "status": "active",
         "freshness": {"state": "stale", "ageSeconds": 7300}, "summary": "stuck seat", "explanation": "no output", "evidence": [{"type": "watchdog-history", "observedAt": "t0"}]},
        {"id": "f-done", "detector": "d3", "category": "context", "scope": {"type": "rig", "rigId": "r"}, "severity": "warning", "status": "cleared",
         "freshness": {"state": "fresh", "ageSeconds": 5}, "summary": "context ok"}
    ], "coverage": [{"source": "queue", "unit": "items", "limit": 10, "total": 20, "evaluated": 10, "omitted": 10, "partial": true}]}),
    );
    assert_eq!(rows[0].label, "Health");
    assert_eq!(detail(&rows[0], "active"), Some("CRIT 1 · WARN 0 · INFO 1"));
    assert!(detail(&rows[0], "coverage").unwrap().starts_with("PARTIAL"));
    assert_eq!(
        detail(&rows[0], "source coverage"),
        Some("queue evaluated 10/20 items (10 omitted)")
    );
    assert_eq!(rows[0].tone, Tone::Error);
    // Active critical first, then active info, then cleared.
    let order: Vec<_> = rows[1..].iter().map(|r| r.label.as_str()).collect();
    assert_eq!(order, vec!["stuck seat", "all quiet", "context ok"]);
    let crit = &rows[1];
    assert_eq!(crit.status.as_deref(), Some("CRITICAL STALE"));
    assert_eq!(crit.tone, Tone::Warn);
    assert_eq!(detail(crit, "freshness"), Some("stale · source age 2h"));
    assert_eq!(detail(crit, "scope"), Some("seat n1"));
    assert_eq!(detail(crit, "policy"), Some("not reported by source"));
    assert_eq!(detail(crit, "evidence"), Some("watchdog-history at t0"));
    assert_eq!(rows[3].status.as_deref(), Some("WARNING CLEARED"));
}

#[test]
fn config_entries_group_by_category_and_format_values() {
    let rows = sections::config_rows(&json!({"readOnly": true, "entries": [
        {"key": "queue.wake_retry_interval_seconds", "group": "general", "value": 120, "defaultValue": 60, "source": "file", "visibility": "shown", "scope": "daemon", "application": "next read"},
        {"key": "ui.enabled", "group": "general", "value": true, "defaultValue": false, "source": "env", "visibility": "shown", "scope": "daemon", "application": "restart"},
        {"key": "slack.botToken", "group": "slack", "value": null, "defaultValue": null, "defaultKnown": false, "source": "default", "visibility": "withheld", "reason": "credential", "scope": "daemon", "application": "restart"},
        {"key": "health.context_pressure.warning_percent", "group": "health", "value": 80, "defaultValue": 70, "source": "default", "visibility": "shown", "scope": "daemon", "application": "live"}
    ], "sources": [{"id": "general", "state": "readable", "path": null, "detail": "config.json"}], "exclusions": ["secrets are never read"]}));
    let wake = find(&rows, "queue.wake_retry_interval_seconds");
    assert_eq!(wake.status.as_deref(), Some("2 min"));
    assert_eq!(detail(wake, "default"), Some("1 min"));
    assert_eq!(
        detail(wake, "override"),
        Some("File setting (above default)")
    );
    assert_eq!(detail(find(&rows, "ui.enabled"), "value"), Some("On"));
    let token = find(&rows, "slack.botToken");
    assert_eq!(detail(token, "value"), Some("Contents withheld"));
    assert_eq!(detail(token, "default"), Some("Not reported"));
    assert_eq!(detail(token, "visibility"), Some("credential"));
    assert_eq!(
        detail(
            find(&rows, "health.context_pressure.warning_percent"),
            "value"
        ),
        Some("80%")
    );
    assert_eq!(find(&rows, "Display & terminals (1)").children_count, 1);
    assert!(rows.iter().any(|r| r.label.starts_with("Slack & people")));
    let sources = find(&rows, "Sources & coverage");
    assert_eq!(detail(sources, "excluded"), Some("secrets are never read"));
}

#[test]
fn specs_group_by_kind() {
    let rows = sections::specs_rows(&json!([
        {"id": "1", "kind": "rig", "name": "demo", "version": "1", "sourceType": "builtin", "sourcePath": "/s/demo.yaml"},
        {"id": "2", "kind": "agent", "name": "builder"},
        {"id": "3", "kind": "workflow", "name": "ship", "rolesCount": 2, "stepsCount": 5, "status": "valid"}
    ]));
    assert_eq!(
        rows.iter()
            .filter(|r| r.depth == 0)
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        vec!["rig (1)", "agent (1)", "workflow (1)"]
    );
    let ship = find(&rows, "ship");
    assert_eq!(detail(ship, "roles"), Some("2"));
    assert_eq!(detail(ship, "steps"), Some("5"));
    assert_eq!(detail(find(&rows, "demo"), "path"), Some("/s/demo.yaml"));
}

#[test]
fn malformed_members_read_as_absent_not_as_failures() {
    // A field of the wrong shape must not take the whole view down.
    let rows = usage::usage_rows(
        &Ok(
            json!({"usage": [{"provider": 7, "identity": "oops", "windows": [{"window": "w", "remainingPercent": "NaN?"}, 5]}], "seatAccounts": {}}),
        ),
        &SwitchSetting::Unread,
    );
    assert!(rows.iter().any(|r| r.label == "7"));
    assert!(rows.iter().any(|r| r.label
        == "Seat accounts are served only while usage windows are"
        || r.label == "This daemon doesn't serve seat accounts"));
}
