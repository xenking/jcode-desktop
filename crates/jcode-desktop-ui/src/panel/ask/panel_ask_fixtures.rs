//! Offline ask cards for screenshots: `JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT=ask-<name>`.
use serde_json::{Value, json};

/// The ask call id the fixture cards carry, so a fixture applet can mount on it.
pub(in crate::panel) const TOOL_CALL: &str = "screenshot-tool";

fn option(label: &str, description: &str) -> Value {
    json!({"label": label, "description": description})
}

/// The questions behind `ask-<name>`; the bare `ask` fixture is the original four-section card.
pub(in crate::panel) fn questions(fixture: &str) -> Option<Vec<Value>> {
    let question = |header: &str, text: &str, multi: bool, options: Vec<Value>| json!({"header": header, "question": text, "multi_select": multi, "options": options});
    Some(match fixture {
        "ask" => (0..4)
            .map(|i| {
                question(
                    &format!("Section {}", i + 1),
                    &format!("Section {}: Which option works best for this project?", i + 1),
                    false,
                    (0..5)
                        .map(|o| {
                            option(
                                &format!("Option {}", o + 1),
                                "A longer description to show how the answer wraps across multiple lines in a compact card.",
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
        "ask-single" => vec![question(
            "Database",
            "Which database should the new web service use?",
            false,
            vec![
                option("Postgres (Recommended)", "Relational, scalable, rich features"),
                option("SQLite", "Embedded, zero-ops, single node"),
                option("DynamoDB", "Managed NoSQL on AWS"),
            ],
        )],
        "ask-multi" => vec![question(
            "Checks",
            "Which checks should run before the release?",
            true,
            vec![
                option("Unit tests", "Fast, run on every commit"),
                option("Integration tests (Recommended)", "Needs the private daemon and takes about four minutes on this machine"),
                option("Lint", "Clippy and rustfmt"),
                option("Visual review", "Screenshots of the changed cards"),
                option("Load test", "Replays last week's traffic against the staging build"),
            ],
        )],
        "ask-rich" => vec![question(
            "Migration",
            "## Schema change\n\nThe `users` table needs a **non-null** `email`:\n\n```rust\nfn backfill(db: &Db) -> Result<usize> {\n    let rows = db.query(\"SELECT id FROM users WHERE email IS NULL\")?;\n    rows.iter().try_fold(0, |n, row| db.set_email(row.id, placeholder(row.id)).map(|_| n + 1))\n}\n```\n\n| Strategy | Downtime | Risk |\n| --- | --- | --- |\n| Backfill then constrain | none | low |\n| Constrain with default | seconds | medium |\n| Rebuild the table | minutes | high |\n\nSee the [migration guide](https://example.com/migrations) first.",
            false,
            vec![
                option("Backfill then constrain (Recommended)", "Two deploys, no lock\n\n- add the column nullable\n- backfill in batches of `10_000`\n- add the `NOT NULL` constraint"),
                option("Constrain with default", "One deploy, a short table lock while the default is applied to every existing row"),
                option("Rebuild the table", "Copy into a new table and swap, the safest rollback story but the longest window"),
            ],
        )],
        "ask-applet" => vec![question(
            "Rollout",
            "The staging rollout is at 60%. Promote it to production?",
            false,
            vec![
                option("Promote now (Recommended)", "Ship to all regions"),
                option("Hold at 60%", "Wait for the next soak window"),
                option("Roll back", "Return staging to the previous build"),
            ],
        )],
        // A swarm worker's ask reaches this session as an ordinary ask whose
        // question names the worker by the label its coordinator spawned it with.
        "ask-worker" => vec![question(
            "Database",
            "Worker \"schema migration\": Which database should the new web service use?",
            false,
            vec![
                option("Postgres (Recommended)", "Relational, scalable, rich features"),
                option("SQLite", "Embedded, zero-ops, single node"),
            ],
        )],
        "ask-mcp" => vec![question(
            "MCP approval",
            "Allow MCP tool \"write-item\" on server \"fixture\" with arguments {\"n\":1}?",
            false,
            vec![
                option("Deny", "Do not run this call"),
                option("Allow once", "Run this one call only"),
                option("Always allow this tool", "Run it now and never ask for this tool again"),
                option("Always allow every tool on this server", "Run it now and never ask for this server again"),
            ],
        )],
        "ask-many" => vec![question(
            "Region",
            "Which region should host the new cluster?",
            false,
            [
                "us-east", "us-west", "eu-west", "eu-central", "ap-south", "ap-northeast", "sa-east",
                "af-south", "me-central", "ca-central", "ap-southeast", "eu-north",
            ]
            .iter()
            .map(|region| option(region, ""))
            .collect(),
        )],
        _ => return None,
    })
}

/// The applet `ask-applet` mounts on the ask call, as an agent applet snapshot.
pub(in crate::panel) fn applet(session_id: &str) -> jcode_applet_types::AgentApplets {
    serde_json::from_value(json!({"instances": [{
        "id": "rollout", "applet": "jcode.agent",
        "placement": {"kind": "inline", "session_id": session_id,
                      "anchor": {"kind": "tool_call", "call_id": TOOL_CALL}},
        "document": {"revision": 1, "title": "Staging rollout", "state": {},
            "view": {"type": "stack", "gap": "sm", "children": [
                {"type": "progress", "value": 0.6, "label": "60% of traffic on the new build"},
                {"type": "table", "columns": ["Region", "Errors", "p95"],
                 "rows": [["us-east", "0.02%", "118 ms"], ["eu-west", "0.01%", "131 ms"], ["ap-south", "0.04%", "204 ms"]]}
            ]}}
    }]})).expect("fixture applet snapshot")
}

/// An answered ask as history delivers it: with its typed record, or, for
/// `legacy`, only the prose a session saved before records existed.
pub(in crate::panel) fn answered(legacy: bool) -> Vec<super::Item> {
    let questions = vec![
        json!({"question": "Which database should we use for this long-term project, and why?", "options": [{"label": "Postgres (Recommended)"}, {"label": "SQLite"}]}),
        json!({"question": "When should we migrate?", "options": [{"label": "Now"}, {"label": "Later"}]}),
    ];
    let record = json!({"questions": questions, "response": {"answers": [
        {"selected": ["Postgres (Recommended)"], "other": "Keep a backup", "comments": {"Postgres (Recommended)": "Reliable and familiar"}},
        {"selected": ["Later"]},
    ]}});
    let prose = "User answers:\n1. Which database should we use for this long-term project, and why? -> Postgres (Recommended) (comment: Reliable and familiar); Other: Keep a backup\n2. When should we migrate? -> Later\n";
    let message = json!({"role": "tool", "content": prose, "tool": {
        "id": "decision-fixture", "name": "ask", "input": {"questions": questions},
        "ask": if legacy { serde_json::Value::Null } else { record },
    }});
    let messages = serde_json::from_value(message).into_iter().collect();
    super::history_items::build(messages, Vec::new()).0
}
