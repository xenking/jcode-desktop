//! System > Usage and System > Usage hosts, from the daemon's provider read models
//! (`packages/tui/src/usage/usage-model.ts` and `usage-hosts-model.ts`).
//!
//! Everything is shown as served: nothing is matched, summed, thresholded or re-timed here.
//! The shape is provider > allowance > window, then seats.
use crate::{
    ReadFailure, Row, Tone, push_group,
    util::{Items, Lax, Table, Text, join},
};
use serde::Deserialize;
use serde_json::Value;

pub const USAGE_HEADER: &str =
    "this host's daemon only · not fleet-wide · not proof of the login a running session uses";
pub const USAGE_REFRESH_HINT: &str = "This view shows what this daemon already holds and never reads the logins itself. To read them now: rig provider status --refresh [--provider codex|claude]";
pub const USAGE_HOSTS_HEADER: &str =
    "registered hosts, as this daemon reads them · not proof of the login a running session uses";
pub const USAGE_HOSTS_HINT: &str = "This view shows what the hosts already hold. It never starts a refresh or reads the logins. To read them now: rig provider status --all-hosts --refresh [--provider codex|claude]";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Identity {
    pub source: Text,
    pub credential_label: Text,
    pub account_id: Text,
    pub workspace_id: Text,
    pub org_id: Text,
    pub org_name: Text,
    pub limit_id: Text,
    pub email: Text,
    pub plan_type: Text,
    pub subscription_type: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Failure {
    pub at: Text,
    pub outcome: Text,
    pub provider: Text,
    pub reason: Text,
    pub detail: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SignIn {
    pub state: Text,
    pub evidence: Text,
    pub at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Exhausted {
    pub at: Text,
    pub window: Text,
    pub evidence: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Unresolved {
    pub note: Text,
    pub kept_reads: Text,
    pub dropped_reads: Lax<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SavedProfiles {
    pub names: Items<Text>,
    pub note: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Disagreeing {
    pub seat_session: Text,
    pub used_percent: Text,
    pub resets_at: Text,
    pub observed_at: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Expectation {
    pub expected_full_since: Text,
    pub note: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Window {
    pub window: Text,
    pub duration_mins: Lax<f64>,
    pub used_percent: Lax<f64>,
    pub remaining_percent: Lax<f64>,
    pub resets_at: Text,
    pub observed_at: Text,
    pub seat_session: Text,
    pub status: Text,
    pub level: Text,
    pub level_reason: Text,
    pub freshness: Text,
    pub freshness_reason: Text,
    pub disagreeing_seats: Items<Disagreeing>,
    pub expectation: Lax<Expectation>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub kind: Text,
}

/// One served allowance row: a provider login's bucket and its windows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Allowance {
    pub provider: Text,
    pub identity: Lax<Identity>,
    pub source: Lax<Source>,
    pub observed_at: Text,
    pub seats: Text,
    pub windows: Items<Window>,
    pub limit_evidence: Text,
    pub latest_from_source: Lax<bool>,
    pub last_refresh: Lax<Failure>,
    pub current_availability: Text,
    pub unavailable_reason: Text,
    pub retained: Text,
    pub account_history: Text,
    pub last_observed_exhausted: Lax<Exhausted>,
    pub saved_profile: Text,
    pub sign_in: Lax<SignIn>,
    pub identity_note: Text,
    pub unresolved: Lax<Unresolved>,
    pub saved_profiles: Lax<SavedProfiles>,
    pub probe: Lax<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Configured {
    pub source: Text,
    pub profile_ref: Text,
    pub recorded_at: Text,
    pub profile_saved: Lax<bool>,
    pub home: Text,
    pub home_note: Text,
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Observed {
    pub withheld: Lax<bool>,
    pub note: Text,
    pub windows_sampled: Lax<bool>,
    pub last_sampled_at: Text,
    pub differs_from: Items<Text>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct SeatAllowance {
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SeatAccount {
    pub seat_session: Text,
    pub runtime: Text,
    pub configured: Lax<Configured>,
    pub registry_notes: Items<Text>,
    pub observed: Lax<Observed>,
    pub allowance: Lax<SeatAllowance>,
}

/// `GET /api/provider/status`: the model has no `usage` key when usage windows are off or unsupported.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageModel {
    pub usage: Items<Allowance>,
    pub seat_accounts: Items<SeatAccount>,
}

/// The `provider.usage_windows` config entry, read only to say why a model has no usage rows.
#[derive(Clone, Debug, PartialEq)]
pub enum SwitchSetting {
    Value { value: Text, source: Text },
    Absent,
    Unread,
}

/// The daemon's served `provider.usage_windows` entry from a browser config read, if any.
pub fn switch_setting(config: Option<&Value>) -> SwitchSetting {
    let Some(config) = config else {
        return SwitchSetting::Unread;
    };
    let entry = config["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|e| e["key"] == "provider.usage_windows");
    match entry {
        Some(e) => SwitchSetting::Value {
            value: serde_json::from_value(e["value"].clone()).unwrap_or_default(),
            source: serde_json::from_value(e["source"].clone()).unwrap_or_default(),
        },
        None => SwitchSetting::Absent,
    }
}

fn window_name(w: &Window) -> String {
    let label = match w.duration_mins.num().map(crate::util::js_number).as_deref() {
        Some("300") => Some("5h".to_owned()),
        Some("10080") => Some("weekly".to_owned()),
        Some(m) => Some(format!("{m} min")),
        None => None,
    };
    match label {
        Some(label) => format!("{label} ({})", w.window.unknown()),
        None => w.window.unknown(),
    }
}

/// A failure's reason and detail as served, worded as `rig provider status` words them.
pub fn failure_text(f: &Failure) -> String {
    format!(
        "{}{}",
        f.reason.or("no reason given"),
        f.detail
            .get()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default()
    )
}

/// Codex: account, else the workspace account/read reported; Claude: the organisation; else unresolved.
pub fn identity_label(id: &Identity) -> String {
    if id.source.is("configured credential label") {
        format!(
            "credential {} (operator label)",
            id.credential_label.unknown()
        )
    } else if let Some(account) = id.account_id.get() {
        format!("account {account}")
    } else if let Some(workspace) = id.workspace_id.get() {
        format!("workspace {workspace} (from account/read; no usage account id)")
    } else if !id.source.is("unresolved")
        && let Some(org) = id.org_id.get()
    {
        match id.org_name.get() {
            Some(name) => format!("organisation {name} ({org})"),
            None => format!("organisation {org}"),
        }
    } else {
        "identity unresolved".into()
    }
}

/// The row's sign-in fact, as served: "usable (evidence; as of ...)".
pub fn sign_in_value(s: &SignIn) -> String {
    let state = if s.state.is("needs_sign_in") {
        "needs sign-in".to_owned()
    } else {
        s.state.unknown()
    };
    format!(
        "{state} ({}{})",
        s.evidence.unknown(),
        s.at.get()
            .map(|at| format!("; as of {at}"))
            .unwrap_or_default()
    )
}

fn level_tone(level: &Text) -> Tone {
    match level.get() {
        Some("ok") => Tone::Ok,
        Some("nearing") => Tone::Warn,
        Some("exhausted") => Tone::Error,
        _ => Tone::Dim,
    }
}

fn availability_tone(a: &Allowance) -> Tone {
    if a.current_availability.is("known") {
        Tone::Ok
    } else {
        Tone::Warn
    }
}

fn push_history(row: &mut Row, a: &Allowance) {
    if let Some(ex) = a.last_observed_exhausted.get() {
        row.put(
            "last observed exhausted",
            format!(
                "at {} ({}: {})",
                ex.at.unknown(),
                ex.window
                    .get()
                    .map_or("account-level".into(), str::to_owned),
                ex.evidence.unknown()
            ),
        );
    }
    if a.account_history.is("unknown") {
        row.put("account history", "unknown");
    }
    if a.saved_profile.is("not linked") {
        row.put(
            "saved profile",
            "not linked (OpenRig doesn't record which saved profile was active at this read)",
        );
    }
}

fn window_row(w: &Window, depth: u8, id: &str) -> Row {
    let left = w.remaining_percent.num();
    let level = w.level.unknown();
    let tone = level_tone(&w.level);
    let meter_label = left.map_or("?% left".into(), |p| {
        format!("{}% left", crate::util::js_number(p))
    });
    let mut row = Row::new(id, depth, window_name(w))
        .status(level.clone(), tone)
        .meter(meter_label, left, tone);
    row.put("window", w.window.unknown());
    if let Some(mins) = w.duration_mins.num() {
        row.put("duration", format!("{} min", crate::util::js_number(mins)));
    }
    if let Some(used) = w.used_percent.num() {
        row.put("used", format!("{}%", crate::util::js_number(used)));
    }
    row.put(
        "left",
        left.map_or("?% left".into(), |p| {
            format!("{}% left", crate::util::js_number(p))
        }),
    );
    row.put(
        "level",
        match w.level_reason.get() {
            Some(reason) => format!("{level} ({reason})"),
            None => level,
        },
    );
    if let Some(status) = w.status.get() {
        row.put("provider status", status);
    }
    row.put(
        "freshness",
        match w.freshness_reason.get() {
            Some(reason) => format!("{}: {reason}", w.freshness.unknown()),
            None => w.freshness.unknown(),
        },
    );
    row.put("resets", w.resets_at.unknown());
    row.put("observed", w.observed_at.unknown());
    if let Some(seat) = w.seat_session.get() {
        row.put("by seat", seat);
    }
    for d in &w.disagreeing_seats {
        row.put(
            &format!("seat {} disagrees", d.seat_session.unknown()),
            format!(
                "reports {}% used · resets {} · observed {}",
                d.used_percent.unknown(),
                d.resets_at.unknown(),
                d.observed_at.unknown()
            ),
        );
    }
    if let Some(e) = w.expectation.get() {
        row.put(
            "expected",
            format!(
                "full since {}, not yet observed ({})",
                e.expected_full_since.unknown(),
                e.note.unknown()
            ),
        );
    }
    row
}

/// One allowance (one provider login's bucket) and its windows, drawn at `depth`.
/// Also used per host by the usage-across-hosts view.
pub fn allowance_rows(
    a: &Allowance,
    depth: u8,
    id: &str,
    label: Option<String>,
    out: &mut Vec<Row>,
) {
    let provider = a.provider.unknown();
    let last_failed = a.last_refresh.get().filter(|l| l.outcome.is("failed"));
    let Some(identity) = a.identity.get() else {
        // A row with no identity yet can still carry history and a sign-in fact.
        let mut row = Row::new(id, depth, label.unwrap_or_else(|| provider.clone()))
            .status("unavailable", Tone::Warn)
            .with("provider", provider)
            .with("unavailable", a.unavailable_reason.or("no reading"));
        push_history(&mut row, a);
        if let Some(last) = last_failed {
            row.put(
                "refresh failed",
                format!("at {}: {}", last.at.unknown(), failure_text(last)),
            );
        }
        if let Some(s) = a.sign_in.get() {
            row.put("sign-in", sign_in_value(s));
        }
        out.push(row);
        return;
    };
    let earlier = if a.latest_from_source.truthy() {
        None
    } else if a.retained.is("before_restart") {
        Some("kept from before the daemon restarted; not a current reading")
    } else if identity.source.is("unresolved") {
        Some("from an earlier read; possibly this same login")
    } else {
        Some("from an earlier read; the latest read didn't match this row")
    };
    let ident = identity_label(identity);
    let mut row = Row::new(
        id,
        depth,
        label.unwrap_or_else(|| format!("{provider} · {ident}")),
    )
    .status(
        match earlier {
            Some(_) => format!("{} · earlier read", a.current_availability.unknown()),
            None => a.current_availability.unknown(),
        },
        if earlier.is_some() {
            Tone::Dim
        } else {
            availability_tone(a)
        },
    )
    .with("provider", provider)
    .with("identity", ident)
    .with("bucket", identity.limit_id.unknown())
    .with("email", identity.email.or("email not reported"))
    .with_opt("plan", identity.plan_type.get())
    .with_opt("subscription", identity.subscription_type.get())
    .with("seats", a.seats.unknown())
    .with_opt("earlier read", earlier);
    if let Some(probe) = a.probe.get() {
        row.put("token probe", probe.to_string());
    }
    if let Some(note) = a.identity_note.get() {
        row.put("identity note", note);
    }
    if let Some(u) = a.unresolved.get() {
        row.put("unresolved", u.note.unknown());
        let dropped = u.dropped_reads.num().unwrap_or(0.);
        row.put(
            "kept reads",
            format!(
                "kept from the {} most recent unresolved reads{}",
                u.kept_reads.unknown(),
                if dropped > 0. {
                    format!(" ({} older dropped)", crate::util::js_number(dropped))
                } else {
                    String::new()
                }
            ),
        );
    }
    push_history(&mut row, a);
    if (a.windows.is_empty() || a.current_availability.is("unknown"))
        && let Some(reason) = a.unavailable_reason.get()
    {
        row.put("unavailable", reason);
    }
    if let Some(evidence) = a.limit_evidence.get() {
        row.put("limit evidence", evidence);
    }
    if let Some(last) = last_failed {
        let identity_only = a.provider.is("claude")
            && !a
                .source
                .get()
                .is_some_and(|s| s.kind.is("claude_token_probe"));
        row.put(
            if identity_only {
                "identity refresh failed"
            } else {
                "refresh failed"
            },
            if identity_only {
                format!(
                    "at {}: {}; the identity shown is from the last refresh that worked, if any",
                    last.at.unknown(),
                    failure_text(last)
                )
            } else {
                format!(
                    "at {}: {}; last reading kept, marked old",
                    last.at.unknown(),
                    failure_text(last)
                )
            },
        );
    }
    row.put("current availability", a.current_availability.unknown());
    if let Some(s) = a.sign_in.get() {
        row.put("sign-in", sign_in_value(s));
    }
    if let Some(saved) = a.saved_profiles.get() {
        row.put(
            &saved.note.unknown(),
            join(saved.names.iter().map(|n| n.unknown()), ", "),
        );
    }
    push_group(out, row.open(), |out| {
        for (i, w) in a.windows.iter().enumerate() {
            out.push(window_row(w, depth + 1, &format!("{id}/w{i}")));
        }
    });
}

fn configured_text(c: &Configured) -> String {
    if c.source.is("registry") {
        let saved = match c.profile_saved.get() {
            Some(false) => "; profile not saved on this host".to_owned(),
            None => "; whether the profile is saved: unknown".to_owned(),
            Some(true) => String::new(),
        };
        format!(
            "profile {} (registry, recorded {}{saved})",
            c.profile_ref.unknown(),
            c.recorded_at.unknown()
        )
    } else if c.source.is("default_login") {
        format!(
            "default login{}",
            c.home.get().map(|h| format!(" ({h})")).unwrap_or_default()
        )
    } else {
        format!("unknown ({})", c.reason.unknown())
    }
}

/// One served seat account. `differs_first` (only the all-hosts page passes it) draws a seat's
/// difference whenever one is served, whatever `windowsSampled` or `withheld` say.
pub fn seat_row(seat: &SeatAccount, depth: u8, id: &str, differs_first: bool) -> Row {
    let configured = seat.configured.get().cloned().unwrap_or_default();
    let mut row = Row::new(id, depth, seat.seat_session.unknown())
        .status(seat.runtime.unknown(), Tone::Neutral)
        .with("runtime", seat.runtime.unknown())
        .with("configured", configured_text(&configured))
        .with("allowance", "not linked");
    if configured.source.is("default_login")
        && let Some(note) = configured.home_note.get()
    {
        row.put("home", note);
    }
    for note in &seat.registry_notes {
        if let Some(note) = note.get() {
            row.put("registry note", note);
        }
    }
    if let Some(o) = seat.observed.get() {
        let differs = join(o.differs_from.iter().map(|d| d.unknown()), ", ");
        let differs_text = format!(
            "differs from {differs}{}",
            o.note.get().map(|n| format!(" ({n})")).unwrap_or_default()
        );
        if differs_first && !o.differs_from.is_empty() {
            row.put("observed", differs_text);
            row.tone = Tone::Warn;
        } else if o.withheld.truthy() {
            row.put("observed", o.note.or("withheld"));
        } else if o.windows_sampled.get() == Some(&true) {
            row.put(
                "observed",
                format!(
                    "this seat's status line last reported windows at {}",
                    o.last_sampled_at.or("an unknown time")
                ),
            );
            if !o.differs_from.is_empty() {
                row.put("differs", differs_text);
                row.tone = Tone::Warn;
            }
        } else if o.windows_sampled.get() == Some(&false) {
            row.put("observed", "no status-line windows from this seat yet");
        } else if let Some(note) = o.note.get() {
            row.put("observed", note);
        }
    }
    if let Some(reason) = seat
        .allowance
        .get()
        .and_then(|a| a.reason.get().map(str::to_owned))
    {
        row.put("allowance not linked", reason);
    }
    row
}

fn notice(id: &str, label: &str, detail: &[(&str, String)]) -> Row {
    let mut row = Row::new(id, 0, label).status("unavailable", Tone::Warn);
    for (k, v) in detail {
        row.put(k, v.clone());
    }
    row
}

fn failure_row(id: &str, what: &str, f: &ReadFailure) -> Row {
    let text = if f.status == Some(404) {
        format!("This daemon doesn't serve {what} (no read model: HTTP 404).")
    } else {
        format!(
            "{what} unavailable: {}. Nothing is inferred from the missing read.",
            f.error
        )
    };
    notice(id, &format!("{what} unavailable"), &[("reason", text)])
}

fn absent_text(setting: &SwitchSetting) -> Vec<String> {
    match setting {
        SwitchSetting::Absent => vec!["This daemon doesn't serve usage windows (it doesn't know the provider.usage_windows setting).".into()],
        SwitchSetting::Unread => vec!["No usage windows served. Whether they're off or not supported by this daemon couldn't be read.".into()],
        SwitchSetting::Value { value, source } if value.is("true") => vec![format!(
            "provider.usage_windows is on ({}), but this daemon served no usage windows.",
            source.unknown()
        )],
        SwitchSetting::Value { value, source } => vec![
            format!(
                "Usage windows are off on this daemon (provider.usage_windows = {}{}).",
                value.unknown(),
                source.get().map(|s| format!(", {s}")).unwrap_or_default()
            ),
            "Turn them on with: rig config set provider.usage_windows true".into(),
        ],
    }
}

/// Provider groups keep the order in which the daemon first served each provider.
fn by_provider(rows: &[Allowance]) -> Vec<(String, Vec<(usize, &Allowance)>)> {
    let mut groups: Vec<(String, Vec<(usize, &Allowance)>)> = Vec::new();
    for (i, a) in rows.iter().enumerate() {
        let provider = a.provider.unknown();
        match groups.iter_mut().find(|(p, _)| *p == provider) {
            Some((_, list)) => list.push((i, a)),
            None => groups.push((provider, vec![(i, a)])),
        }
    }
    groups
}

/// The Usage section: provider > allowance > window, then seats.
pub fn usage_rows(read: &Result<Value, ReadFailure>, setting: &SwitchSetting) -> Vec<Row> {
    let model = match read {
        Ok(model) if model.is_object() => model,
        Ok(_) => {
            return vec![notice(
                "usage/error",
                "Usage windows unavailable",
                &[(
                    "reason",
                    "the response was not a provider read model".into(),
                )],
            )];
        }
        Err(f) => return vec![failure_row("usage/error", "Usage windows", f)],
    };
    let model: UsageModel = serde_json::from_value(model.clone()).unwrap_or_default();
    let mut out = vec![
        Row::new("usage/overview", 0, "This host's daemon")
            .with("scope", USAGE_HEADER)
            .with("refresh", USAGE_REFRESH_HINT),
    ];
    push_group(
        &mut out,
        Row::new("usage/windows", 0, "Usage windows").open(),
        |out| {
            if model.usage.is_array() {
                if model.usage.is_empty() {
                    out.push(Row::new("usage/none", 1, "No usage rows served"));
                }
                for (provider, list) in by_provider(model.usage.as_slice()) {
                    push_group(
                        out,
                        Row::new(format!("usage/p/{provider}"), 1, provider).open(),
                        |out| {
                            for (i, a) in list {
                                allowance_rows(a, 2, &format!("usage/a/{i}"), None, out);
                            }
                        },
                    );
                }
            } else {
                let lines = absent_text(setting);
                let mut row = Row::new("usage/absent", 1, "No usage windows served")
                    .status("unavailable", Tone::Warn);
                for line in lines {
                    row.put("why", line);
                }
                out.push(row);
            }
        },
    );
    push_group(
        &mut out,
        Row::new("usage/seats", 0, "Seats · configured account")
            .open()
            .with("note", "not proof of the login a running session uses"),
        |out| {
            if model.seat_accounts.is_array() {
                if model.seat_accounts.is_empty() {
                    out.push(Row::new("usage/seats/none", 1, "No seats served"));
                }
                for (i, seat) in model.seat_accounts.iter().enumerate() {
                    out.push(seat_row(seat, 1, &format!("usage/seat/{i}"), false));
                }
            } else {
                out.push(Row::new(
                    "usage/seats/absent",
                    1,
                    if model.usage.is_array() {
                        "This daemon doesn't serve seat accounts"
                    } else {
                        "Seat accounts are served only while usage windows are"
                    },
                ));
            }
        },
    );
    out
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HostRead {
    pub host_id: Text,
    pub status: Text,
    pub error: Text,
    pub usage: Text,
    pub usage_reason: Text,
    pub read_at: Text,
    pub last_refresh: Table<Failure>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Skipped {
    pub host_id: Text,
    pub reason: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Observation {
    pub host_id: Text,
    pub read_at: Text,
    pub row: Lax<Allowance>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Group {
    pub provider: Text,
    pub linkage: Text,
    pub notes: Items<Text>,
    pub observations: Items<Observation>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HostSeats {
    pub host_id: Text,
    pub seats: Items<SeatAccount>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Registry {
    pub state: Text,
    pub note: Text,
}

/// `GET /api/provider/usage/hosts`: usage windows on every registered host, as this daemon reads them.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HostsView {
    pub hosts: Items<HostRead>,
    pub skipped: Items<Skipped>,
    pub registry: Lax<Registry>,
    pub items: Items<Group>,
    pub seat_accounts: Items<HostSeats>,
}

fn host_row(h: &HostRead, depth: u8, id: &str) -> Row {
    let ok = h.status.is("ok");
    let transport = if ok {
        "answered".to_owned()
    } else {
        format!(
            "{}{}",
            h.status.unknown(),
            h.error.get().map(|e| format!(" ({e})")).unwrap_or_default()
        )
    };
    let served = h.usage.is("served");
    let usage = if served {
        "usage windows served".to_owned()
    } else {
        format!(
            "usage {}{}",
            h.usage.unknown(),
            h.usage_reason
                .get()
                .map(|r| format!(": {r}"))
                .unwrap_or_default()
        )
    };
    let mut row = Row::new(id, depth, h.host_id.unknown())
        .status(
            transport.clone(),
            if ok && served { Tone::Ok } else { Tone::Warn },
        )
        .with("transport", transport)
        .with("usage", usage)
        .with_opt("read", h.read_at.get());
    for leg in h.last_refresh.0.values() {
        let provider = leg.provider.unknown();
        row.put(
            &format!("refresh from here ({provider})"),
            if leg.outcome.is("failed") {
                format!(
                    "failed at {}: {}; this host's earlier observations are kept",
                    leg.at.unknown(),
                    failure_text(leg)
                )
            } else {
                format!("answered at {}", leg.at.unknown())
            },
        );
    }
    row
}

fn linkage_text(g: &Group, hosts: &[String]) -> (String, Tone) {
    match g.linkage.get() {
        Some("same") => (
            format!(
                "the same allowance on {} hosts: {}",
                hosts.len(),
                hosts.join(", ")
            ),
            Tone::Ok,
        ),
        Some("conflict") => (
            format!(
                "the same allowance on {} hosts, disagreeing: {}",
                hosts.len(),
                hosts.join(", ")
            ),
            Tone::Error,
        ),
        Some("single") => (
            format!(
                "observed on {}",
                hosts.first().map_or("unknown", String::as_str)
            ),
            Tone::Neutral,
        ),
        _ => (
            format!("linkage unknown; observed on {}", hosts.join(", ")),
            Tone::Warn,
        ),
    }
}

/// The Usage hosts section: hosts; allowance > host > window; seats per host.
pub fn usage_hosts_rows(read: &Result<Value, ReadFailure>) -> Vec<Row> {
    let view = match read {
        Ok(view) if view.is_object() => view,
        Ok(_) => {
            return vec![notice(
                "uh/error",
                "Usage across hosts unavailable",
                &[(
                    "reason",
                    "the response was not a usage-across-hosts view".into(),
                )],
            )];
        }
        Err(f) if f.status == Some(404) => {
            return vec![notice(
                "uh/error",
                "Usage across hosts unavailable",
                &[(
                    "reason",
                    "This daemon doesn't serve usage across hosts (HTTP 404).".into(),
                )],
            )];
        }
        Err(f) if f.status == Some(409) && f.message.is_some() => {
            return vec![notice(
                "uh/error",
                "Usage across hosts unavailable",
                &[("reason", f.message.clone().unwrap_or_default())],
            )];
        }
        Err(f) => return vec![failure_row("uh/error", "Usage across hosts", f)],
    };
    let view: HostsView = serde_json::from_value(view.clone()).unwrap_or_default();
    let mut out = vec![
        Row::new("uh/overview", 0, "Registered hosts")
            .with("scope", USAGE_HOSTS_HEADER)
            .with("refresh", USAGE_HOSTS_HINT),
    ];
    push_group(&mut out, Row::new("uh/hosts", 0, "Hosts").open(), |out| {
        for (i, h) in view.hosts.iter().enumerate() {
            out.push(host_row(h, 1, &format!("uh/host/{i}")));
        }
        for (i, s) in view.skipped.iter().enumerate() {
            out.push(
                Row::new(format!("uh/skipped/{i}"), 1, s.host_id.unknown())
                    .status("skipped", Tone::Warn)
                    .with("skipped", s.reason.unknown()),
            );
        }
        if let Some(registry) = view.registry.get().filter(|r| !r.state.is("read")) {
            out.push(
                Row::new("uh/registry", 1, "Host registry")
                    .status(registry.state.unknown(), Tone::Warn)
                    .with_opt("note", registry.note.get()),
            );
        }
    });
    push_group(
        &mut out,
        Row::new("uh/allowances", 0, "Allowances").open().with(
            "note",
            "one entry per allowance; each host's observation as that host served it, never summed",
        ),
        |out| {
            if view.items.is_empty() {
                out.push(Row::new("uh/none", 1, "No usage rows served"));
            }
            for (gi, g) in view.items.iter().enumerate() {
                let mut hosts: Vec<String> = Vec::new();
                for o in &g.observations {
                    let h = o.host_id.unknown();
                    if !hosts.contains(&h) {
                        hosts.push(h);
                    }
                }
                let ident = g
                    .observations
                    .iter()
                    .next()
                    .and_then(|o| o.row.get())
                    .and_then(|r| r.identity.get());
                let label = format!(
                    "{} · {}",
                    g.provider.unknown(),
                    ident.map_or("not read yet".into(), |id| format!(
                        "{} · bucket {}",
                        identity_label(id),
                        id.limit_id.unknown()
                    ))
                );
                let (linkage, tone) = linkage_text(g, &hosts);
                let mut row = Row::new(format!("uh/g/{gi}"), 1, label)
                    .status(linkage.clone(), tone)
                    .open()
                    .with("linkage", linkage);
                for note in &g.notes {
                    if let Some(note) = note.get() {
                        row.put("note", note);
                    }
                }
                push_group(out, row, |out| {
                    for (oi, o) in g.observations.iter().enumerate() {
                        let row = o.row.get().cloned().unwrap_or_default();
                        let label =
                            format!("on {} (read {})", o.host_id.unknown(), o.read_at.unknown());
                        allowance_rows(&row, 2, &format!("uh/g/{gi}/o/{oi}"), Some(label), out);
                    }
                });
            }
        },
    );
    for (si, s) in view.seat_accounts.iter().enumerate() {
        push_group(
            &mut out,
            Row::new(
                format!("uh/seats/{si}"),
                0,
                format!("Seats on {}", s.host_id.unknown()),
            )
            .open()
            .with(
                "note",
                "configured account (not proof of the login a running session uses)",
            ),
            |out| {
                if s.seats.is_empty() {
                    out.push(Row::new(
                        format!("uh/seats/{si}/none"),
                        1,
                        "No seats served",
                    ));
                }
                for (i, seat) in s.seats.iter().enumerate() {
                    out.push(seat_row(seat, 1, &format!("uh/seats/{si}/{i}"), true));
                }
            },
        );
    }
    out
}
