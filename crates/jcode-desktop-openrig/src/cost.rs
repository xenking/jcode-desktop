//! System > Cost: what each seat's harness reports about cost, beside its tokens
//! (`packages/tui/src/cost/cost-model.ts`, the body `rig usage cost` prints).
//!
//! A provider charge sits beside the estimate it matches and is never added to it. Nothing is
//! summed across seats, and a missing figure reads as missing, never as $0.
use crate::{
    ReadFailure, Row, Tone,
    util::{Items, Lax, Table, Text, join, js_number},
};
use serde::Deserialize;
use serde_json::Value;

pub const COST_HEADER: &str =
    "this host's daemon only · not fleet-wide · what each harness reports, not an invoice";
pub const COST_READ_HINT: &str = "This view reads what the daemon already holds and starts no lookup. The same lines: rig usage cost";

/// A real amount below a cent prints as "<$0.01", never "$0.00" (which reads as zero).
pub fn format_usd(value: f64) -> String {
    if value > 0. && value < 0.005 {
        "<$0.01".into()
    } else {
        format!("${value:.2}")
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Tokens {
    pub input: Lax<f64>,
    pub output: Lax<f64>,
    pub cache_read: Lax<f64>,
    pub cache_write: Lax<f64>,
}

fn num(n: &Lax<f64>) -> String {
    n.num().map_or("?".into(), js_number)
}
impl Tokens {
    fn pair(&self) -> String {
        format!("in {} / out {}", num(&self.input), num(&self.output))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Window {
    pub pi_session_id: Text,
    pub native_session_id: Text,
    pub since: Text,
    pub as_of: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Apart {
    pub count: Lax<f64>,
    pub cost_usd: Lax<f64>,
}
impl Apart {
    fn count(&self) -> f64 {
        self.count.num().unwrap_or(0.)
    }
    fn with_cost(&self) -> String {
        format!(
            "{}{}",
            js_number(self.count()),
            self.cost_usd
                .num()
                .map(|c| format!(" ({})", format_usd(c)))
                .unwrap_or_default()
        )
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Entries {
    pub own: Lax<f64>,
    pub own_without_cost: Lax<f64>,
    pub error_or_aborted: Lax<f64>,
    pub inherited: Lax<Apart>,
    pub unknown_origin: Lax<Apart>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reasons {
    pub count: Lax<f64>,
    pub reasons: Table<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Requests {
    pub open_router: Lax<f64>,
    pub covered: Lax<f64>,
    pub charged: Lax<f64>,
    pub pending: Lax<f64>,
    pub unavailable: Lax<Reasons>,
    pub not_covered: Lax<Reasons>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountContext {
    pub credential_sources: Items<Text>,
    pub workspace_ids: Items<Text>,
    pub byok_requests: Lax<f64>,
}

/// OpenRouter's actual charge, beside the estimate.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Charge {
    pub kind: Text,
    pub status: Text,
    pub source: Text,
    pub cost_usd: Lax<f64>,
    pub estimate_for_charged_usd: Lax<f64>,
    pub requests: Lax<Requests>,
    pub account_context: Lax<AccountContext>,
    pub coverage: Text,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Seat {
    pub seat_session: Text,
    pub status: Text,
    pub runtime: Text,
    pub kind: Text,
    pub source: Text,
    pub cost_usd: Lax<f64>,
    pub reason: Text,
    pub note: Text,
    pub stale: Lax<bool>,
    pub age_seconds: Lax<f64>,
    pub resets: Lax<f64>,
    pub window: Lax<Window>,
    pub entries: Lax<Entries>,
    pub kinds: Table<f64>,
    pub tokens: Lax<Tokens>,
    pub coverage: Text,
    pub provider_charge: Lax<Charge>,
}

/// The served cost view: every seat names itself and its status, or the body is not a cost view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostView {
    pub seats: Vec<Seat>,
    pub note: String,
}

pub fn parse_cost(body: &Value) -> Result<CostView, String> {
    let seats = body["seats"]
        .as_array()
        .filter(|seats| {
            seats
                .iter()
                .all(|s| s["seatSession"].as_str().is_some() && s["status"].as_str().is_some())
        })
        .ok_or("the response was not a cost view")?;
    Ok(CostView {
        seats: seats
            .iter()
            .map(|s| serde_json::from_value(s.clone()).unwrap_or_default())
            .collect(),
        note: body["note"].as_str().unwrap_or_default().to_owned(),
    })
}

fn plural(n: f64, one: &str, many: &str) -> String {
    format!("{} {}", js_number(n), if n == 1. { one } else { many })
}

fn top_reason(r: &Table<f64>) -> String {
    r.0.iter()
        .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(k, _)| k.clone())
        .unwrap_or_default()
}

/// OpenRouter's actual charge, beside the estimate, as `rig usage cost` words it.
fn charge_summary(c: &Charge) -> String {
    let empty = Requests::default();
    let q = c.requests.get().unwrap_or(&empty);
    let unavailable = q.unavailable.get();
    let not_covered = q
        .not_covered
        .get()
        .and_then(|n| n.count.num())
        .filter(|n| *n > 0.);
    let nc = not_covered
        .map(|n| format!(", {} not covered", js_number(n)))
        .unwrap_or_default();
    let pending = q.pending.num().unwrap_or(0.);
    match c.status.get() {
        Some("reported" | "partial") => {
            let beside = c
                .estimate_for_charged_usd
                .num()
                .map(|e| format!(", Pi estimate for them {}", format_usd(e)))
                .unwrap_or_default();
            let mut rest = Vec::new();
            if pending > 0. {
                rest.push(format!("{} pending", js_number(pending)));
            }
            if let Some(u) = unavailable.filter(|u| u.count.num().unwrap_or(0.) > 0.) {
                rest.push(format!(
                    "{} unavailable ({})",
                    js_number(u.count.num().unwrap_or(0.)),
                    top_reason(&u.reasons)
                ));
            }
            let who = c
                .account_context
                .get()
                .filter(|a| !a.credential_sources.is_empty())
                .map(|a| {
                    format!(
                        " via {}",
                        join(a.credential_sources.iter().map(|s| s.unknown()), ", ")
                    )
                })
                .unwrap_or_default();
            format!(
                "OpenRouter charge {} USD for {} of {} requests{who}{beside}{}{nc} (provider-reported, not added to the estimate)",
                format_usd(c.cost_usd.num().unwrap_or(f64::NAN)),
                num(&q.charged),
                num(&q.covered),
                if rest.is_empty() {
                    String::new()
                } else {
                    format!("; {}", rest.join(", "))
                },
            )
        }
        Some("pending") => format!(
            "OpenRouter charge pending for {}{nc}",
            plural(pending, "request", "requests")
        ),
        Some("unavailable") => format!(
            "OpenRouter charge unavailable ({}){nc}",
            unavailable
                .map(|u| top_reason(&u.reasons))
                .unwrap_or_default()
        ),
        Some("none") => {
            if q.covered.num().unwrap_or(0.) > 0. {
                format!(
                    "OpenRouter charge not looked up (usage.pi_openrouter_charge off, or not run yet){nc}"
                )
            } else if let Some(n) = not_covered {
                format!(
                    "OpenRouter charge: {} not covered",
                    plural(n, "request", "requests")
                )
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn pi_head(seat: &Seat) -> String {
    let w = seat.window.get().cloned().unwrap_or_default();
    let e = seat.entries.get().cloned().unwrap_or_default();
    let t = seat.tokens.get().cloned().unwrap_or_default();
    let mut extras = Vec::new();
    let without = e.own_without_cost.num().unwrap_or(0.);
    if without > 0. {
        extras.push(format!(
            "{} without a cost",
            plural(without, "entry", "entries")
        ));
    }
    let errors = e.error_or_aborted.num().unwrap_or(0.);
    if errors > 0. {
        extras.push(format!(
            "{} included",
            plural(errors, "error/aborted turn", "error/aborted turns")
        ));
    }
    if let Some(i) = e.inherited.get().filter(|i| i.count() > 0.) {
        extras.push(format!(
            "{} inherited from the fork, not counted{}",
            js_number(i.count()),
            i.cost_usd
                .num()
                .map(|c| format!(" ({})", format_usd(c)))
                .unwrap_or_default()
        ));
    }
    if let Some(u) = e.unknown_origin.get().filter(|u| u.count() > 0.) {
        extras.push(format!(
            "{} of unknown origin, not counted{}",
            js_number(u.count()),
            u.cost_usd
                .num()
                .map(|c| format!(" ({})", format_usd(c)))
                .unwrap_or_default()
        ));
    }
    let head = if seat.status.is("reported") {
        format!(
            "{} model-priced estimate (Pi persisted entries, {} own entries) for Pi session {}{}{}",
            format_usd(seat.cost_usd.num().unwrap_or(f64::NAN)),
            e.own.num().map_or("0".into(), js_number),
            w.pi_session_id.or("undefined"),
            w.since
                .get()
                .map(|s| format!(" since {s}"))
                .unwrap_or_default(),
            w.as_of
                .get()
                .map(|s| format!(", last {s}"))
                .unwrap_or_default(),
        )
    } else {
        format!(
            "cost: not reported ({}) for Pi session {}",
            seat.reason.or("undefined"),
            w.pi_session_id.or("undefined")
        )
    };
    let charge = seat
        .provider_charge
        .get()
        .map(charge_summary)
        .filter(|c| !c.is_empty());
    format!(
        "{head}{}{}  tokens {}",
        if extras.is_empty() {
            String::new()
        } else {
            format!("; {}", extras.join("; "))
        },
        charge.map(|c| format!("; {c}")).unwrap_or_default(),
        t.pair()
    )
}

/// The line `rig usage cost` prints for a seat, without the leading seat name.
pub fn cost_head(seat: &Seat) -> String {
    if seat.runtime.is("pi") {
        return pi_head(seat);
    }
    let w = seat.window.get().cloned().unwrap_or_default();
    let tokens = seat.tokens.get().cloned().unwrap_or_default();
    match seat.status.get() {
        Some("reported") => {
            let stale = if seat.stale.truthy() {
                format!(
                    ", STALE ({}s old; kept as history)",
                    seat.age_seconds.num().map_or("?".into(), js_number)
                )
            } else {
                String::new()
            };
            let resets = seat.resets.num().filter(|r| *r > 0.);
            format!(
                "{} client-side estimate (Claude statusline) for session {} since {}, as of {}{}{stale}  tokens {}",
                format_usd(seat.cost_usd.num().unwrap_or(f64::NAN)),
                w.native_session_id.or("null"),
                w.since.or("undefined"),
                w.as_of.or("null"),
                resets
                    .map(|r| format!(", {}", plural(r, "reset", "resets")))
                    .unwrap_or_default(),
                tokens.pair(),
            )
        }
        Some("absent") => format!(
            "cost: not reported ({})  tokens {}",
            seat.reason.or("undefined"),
            tokens.pair()
        ),
        Some("no-cost-method") => format!(
            "tokens {}  cost: {}",
            tokens.pair(),
            seat.note.or("undefined")
        ),
        _ => format!("cost: unknown ({})", seat.reason.or("undefined")),
    }
}

fn reasons(row: &mut Row, label: &str, r: Option<&Reasons>) {
    if let Some(r) = r {
        for (reason, n) in &r.reasons.0 {
            row.put(&format!("{label}: {reason}"), js_number(*n));
        }
    }
}

fn charge_details(row: &mut Row, c: &Charge) {
    let empty = Requests::default();
    let q = c.requests.get().unwrap_or(&empty);
    let a = c.account_context.get().cloned().unwrap_or_default();
    let money = match c.cost_usd.num() {
        Some(cost) => format!(
            "{} USD{}",
            format_usd(cost),
            c.estimate_for_charged_usd
                .num()
                .map(|e| format!(" (Pi's estimate for the same requests: {})", format_usd(e)))
                .unwrap_or_default()
        ),
        None => "no charge established".into(),
    };
    row.put(
        "provider charge",
        format!(
            "{}, beside the estimate and never added to it: {} · {money}",
            c.kind.or("provider charge"),
            c.status.unknown()
        ),
    );
    row.put("charge source", c.source.unknown());
    let count = |r: &Lax<Reasons>| {
        r.get()
            .and_then(|r| r.count.num())
            .map_or("0".into(), js_number)
    };
    row.put(
        "requests",
        format!(
            "{} OpenRouter · {} covered · {} charged · {} pending · {} unavailable · {} not covered",
            num(&q.open_router),
            num(&q.covered),
            q.charged.num().map_or("0".into(), js_number),
            q.pending.num().map_or("0".into(), js_number),
            count(&q.unavailable),
            count(&q.not_covered),
        ),
    );
    reasons(row, "unavailable", q.unavailable.get());
    reasons(row, "not covered", q.not_covered.get());
    let list = |v: &Items<Text>, missing: &str| {
        if v.is_empty() {
            missing.to_owned()
        } else {
            join(v.iter().map(|s| s.unknown()), ", ")
        }
    };
    row.put(
        "account context",
        format!(
            "credential {} · workspace {} · BYOK requests {}",
            list(&a.credential_sources, "not established"),
            list(&a.workspace_ids, "not reported"),
            a.byok_requests.num().map_or("0".into(), js_number)
        ),
    );
    if let Some(cov) = c.coverage.get() {
        row.put("charge coverage", cov);
    }
}

fn seat_row(seat: &Seat, index: usize) -> Row {
    let tone = match seat.status.get() {
        Some("reported") if seat.stale.truthy() => Tone::Warn,
        Some("reported") => Tone::Ok,
        Some("absent" | "no-cost-method") => Tone::Dim,
        _ => Tone::Warn,
    };
    let mut row = Row::new(format!("cost/seat/{index}"), 0, seat.seat_session.unknown())
        .status(seat.status.unknown(), tone)
        .with("summary", cost_head(seat));
    let w = seat.window.get().cloned().unwrap_or_default();
    if seat.kind.is_set() || seat.source.is_set() {
        row.put(
            "kind",
            format!(
                "{} · source: {}",
                seat.kind.unknown(),
                seat.source.unknown()
            ),
        );
    }
    if seat.runtime.is("pi") {
        row.put(
            "window",
            format!(
                "Pi session {} · first own entry {} · last {}",
                w.pi_session_id.unknown(),
                w.since.or("none"),
                w.as_of.or("none")
            ),
        );
        if !seat.kinds.0.is_empty() {
            row.put(
                "own entries by kind",
                join(
                    seat.kinds
                        .0
                        .iter()
                        .map(|(k, n)| format!("{k} {}", js_number(*n))),
                    " · ",
                ),
            );
        }
        let e = seat.entries.get().cloned().unwrap_or_default();
        let none = Apart::default();
        row.put(
            "entries",
            format!(
                "own {} (counted; {} without a cost, {} error/aborted) · inherited {} and unknown origin {}: shown apart, not counted",
                e.own.num().map_or("0".into(), js_number),
                e.own_without_cost.num().map_or("0".into(), js_number),
                e.error_or_aborted.num().map_or("0".into(), js_number),
                e.inherited.get().unwrap_or(&none).with_cost(),
                e.unknown_origin.get().unwrap_or(&none).with_cost(),
            ),
        );
        let t = seat.tokens.get().cloned().unwrap_or_default();
        row.put(
            "tokens",
            format!(
                "in {} · out {} · cache read {} · cache write {}",
                num(&t.input),
                num(&t.output),
                num(&t.cache_read),
                num(&t.cache_write)
            ),
        );
    } else if seat.runtime.is("claude-code") || seat.runtime.is("codex") {
        row.put(
            "window",
            format!(
                "native session {}{} · as of {}",
                w.native_session_id.or("not reported"),
                w.since
                    .get()
                    .map(|s| format!(" · since {s}"))
                    .unwrap_or_default(),
                w.as_of.or("not reported")
            ),
        );
        if seat.status.is("reported") {
            let age = seat.age_seconds.num().map_or("?".into(), js_number);
            row.put(
                "resets in this session",
                format!(
                    "{} · {}",
                    seat.resets.num().map_or("0".into(), js_number),
                    if seat.stale.truthy() {
                        format!("stale: {age}s old, kept as history")
                    } else {
                        format!("fresh ({age}s old)")
                    }
                ),
            );
        }
        if let Some(t) = seat.tokens.get() {
            row.put("tokens", t.pair());
        }
    } else if let Some(t) = seat.tokens.get() {
        row.put("tokens", t.pair());
    }
    if let Some(cov) = seat.coverage.get() {
        row.put("coverage", cov);
    }
    if let Some(c) = seat.provider_charge.get() {
        charge_details(&mut row, c);
    }
    row
}

pub fn cost_rows(read: &Result<Value, ReadFailure>) -> Vec<Row> {
    let unavailable = |reason: String| {
        vec![
            Row::new("cost/error", 0, "Cost unavailable")
                .status("unavailable", Tone::Warn)
                .with("reason", reason),
        ]
    };
    let body = match read {
        Ok(body) => body,
        Err(f) if f.status == Some(404) => {
            return unavailable(
                "This daemon doesn't serve the cost view (no /api/telemetry/usage/cost: HTTP 404)."
                    .into(),
            );
        }
        Err(f) => {
            return unavailable(format!(
                "Cost unavailable: {}. Nothing is inferred from the missing read.",
                f.error
            ));
        }
    };
    let view = match parse_cost(body) {
        Ok(view) => view,
        Err(e) => {
            return unavailable(format!(
                "Cost unavailable: {e}. Nothing is inferred from the missing read."
            ));
        }
    };
    let mut rows = vec![
        Row::new(
            "cost/overview",
            0,
            format!(
                "Seats · {}",
                plural(view.seats.len() as f64, "seat", "seats")
            ),
        )
        .with("scope", COST_HEADER)
        .with("read", COST_READ_HINT)
        .with_opt("note", Some(view.note.clone()).filter(|n| !n.is_empty())),
    ];
    if view.seats.is_empty() {
        rows.push(Row::new(
            "cost/none",
            0,
            "no usage samples yet: no seat has reported tokens or cost",
        ));
    }
    rows.extend(view.seats.iter().enumerate().map(|(i, s)| seat_row(s, i)));
    rows
}
