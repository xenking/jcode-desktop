//! Offline screenshot and demo transcripts: what a fixture session shows
//! before any runtime has answered.
use super::*;

pub(super) fn screenshot_fixture_for(session_id: &str) -> Option<String> {
    let per_panel = std::env::var("JCODE_DESKTOP_SCREENSHOT_PANEL_TRANSCRIPTS").ok();
    let global = std::env::var("JCODE_DESKTOP_SCREENSHOT_TRANSCRIPT").ok();
    fixture_for_panel(session_id, per_panel.as_deref(), global)
}

pub(super) fn fixture_for_panel(
    session_id: &str,
    per_panel: Option<&str>,
    global: Option<String>,
) -> Option<String> {
    let index = match session_id {
        "screenshot-fixture" => Some(0),
        other => other
            .strip_prefix("screenshot-fixture-")
            .and_then(|index| index.parse::<usize>().ok()),
    };
    index
        .and_then(|index| per_panel?.split(',').nth(index))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or(global)
}

#[cfg(test)]
mod fixture_for_panel_tests {
    use super::fixture_for_panel;

    #[test]
    fn per_panel_list_overrides_global_by_index() {
        let list = Some("diff-rich, ,mermaid");
        let global = || Some("all".to_owned());
        assert_eq!(
            fixture_for_panel("screenshot-fixture", list, global()).as_deref(),
            Some("diff-rich")
        );
        assert_eq!(
            fixture_for_panel("screenshot-fixture-1", list, global()).as_deref(),
            Some("all")
        );
        assert_eq!(
            fixture_for_panel("screenshot-fixture-2", list, global()).as_deref(),
            Some("mermaid")
        );
        assert_eq!(
            fixture_for_panel("screenshot-fixture-3", list, global()).as_deref(),
            Some("all")
        );
        assert_eq!(
            fixture_for_panel("real-session", list, global()).as_deref(),
            Some("all")
        );
        assert_eq!(fixture_for_panel("screenshot-fixture-1", None, None), None);
    }
}

/// Screenshot fixture timings, so captures show the real tool clock labels
/// instead of "unknown". The release check finished, the cleanup was refused
/// instantly.
pub(super) type DemoToolTimes = (
    HashMap<String, Duration>,
    HashMap<String, (Option<i64>, Option<i64>)>,
);

pub(super) fn demo_tool_times(fixture: Option<&str>) -> DemoToolTimes {
    let mut durations = HashMap::new();
    let mut wall = HashMap::new();
    if !crate::harness::screenshot_mode() || fixture != Some("expandable") {
        return (durations, wall);
    }
    let Some(now) = response_stats::now_ms() else {
        return (durations, wall);
    };
    for (call_id, ago_ms, ran_ms) in [
        ("expandable-title", 95_000_i64, 83_400_u64),
        ("expandable-error", 9_000, 40),
    ] {
        let start = now - ago_ms;
        durations.insert(call_id.to_string(), Duration::from_millis(ran_ms));
        wall.insert(
            call_id.to_string(),
            (Some(start), Some(start + ran_ms as i64)),
        );
    }
    (durations, wall)
}

/// `JCODE_DESKTOP_DEMO_TRANSCRIPT=1` seeds one panel with a sample of every
/// transcript shape, so rendering changes can be reviewed without driving a
/// real session through each case.
pub(super) fn demo_items(fixture: Option<&str>) -> Vec<Item> {
    let fixture = fixture.map(str::to_owned);
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("clean-chat") {
        return clean_chat::screenshot_fixture();
    }
    if crate::harness::screenshot_mode() && matches!(fixture.as_deref(), Some("external-dm" | "external-dm-clean")) {
        return agent_message::screenshot_fixture();
    }
    if crate::harness::screenshot_mode() && matches!(fixture.as_deref(), Some("btw-normal" | "btw-clean" | "btw-clean-expanded")) {
        return side_question::inline::screenshot_fixture();
    }
    if crate::harness::screenshot_mode()
        && matches!(
            fixture.as_deref(),
            Some("image-ordinals" | "image-ordinals-resolved")
        )
    {
        let ids = crate::image_chip::ordinal_fixture_ids();
        let mut items: Vec<_> = (1..118)
            .map(|number| Item::User(format!("Offline history item {number}")))
            .collect();
        items.push(Item::User(format!(
            "Сначала проверь вложение [#{}] и потом сравни со вторым вложением [#{}] и сохрани оба результата.", ids[0], ids[1],
        )));
        if fixture.as_deref() == Some("image-ordinals-resolved") {
            items.extend(ids.into_iter().zip([113, 114]).map(|(id, ordinal)| {
                let mut image =
                    TranscriptImage::new("image/png".into(), format!("jcode-lazy:{id}"), None);
                image.session_image = Some(jcode_sdk::api::SessionImageRef {
                    image_id: id.into(),
                    ordinal,
                });
                Item::Image(image)
            }));
        }
        items.extend((1..=4).map(|number| Item::Assistant(format!("Offline reply {number}"))));
        return items;
    }
    if crate::harness::screenshot_mode()
        && let Some(legacy) = match fixture.as_deref() {
            Some("decision") => Some(false),
            Some("decision-legacy") => Some(true),
            _ => None,
        }
    {
        return ask_fixtures::answered(legacy);
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("background-tasks") {
        return vec![
            Item::User("Run the checks in the background".into()),
            Item::BackgroundTask {
                task_id: "rollout-plan".into(),
                label: "Сверить установленную пару daemon и Desktop".into(),
                summary: "100% · [8/8] verify          plan  daemon + current + bundle report the pair; bridge answers search_sessions; SUPublicEDKey set; rollback on failure".into(),
                detail: Some("```text\n[8/8] verify          plan  daemon + current + bundle report the pair\n```".into()),
                percent: Some(100.0),
                done: true,
            },
            Item::BackgroundTask {
                task_id: "building".into(),
                label: "Workspace tests".into(),
                summary: "35% · Running integration tests".into(),
                detail: None,
                percent: Some(35.0),
                done: false,
            },
            Item::BackgroundTask {
                task_id: "waiting".into(),
                label: "Release build".into(),
                summary: "Compiling dependencies and waiting for the linker to finish".into(),
                detail: None,
                percent: None,
                done: false,
            },
            Item::BackgroundTask {
                task_id: "done".into(),
                label: "Formatting".into(),
                summary: "✓ completed · 8.2s · exit 0".into(),
                detail: None,
                percent: None,
                done: true,
            },
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("swarm") {
        // The user's real workers (see workspace `load_real_swarm_fixture`).
        let call = |id: &str, action: &str, intent: &str| Item::Tool {
            call_id: format!("swarm-{id}"),
            name: "swarm".into(),
            input: format!(r#"{{"action":"{action}","intent":"{intent}","session_ids":["{id}"]}}"#),
            output: String::new(),
            done: true,
            error: None,
            ask: None,
        };
        return vec![
            Item::User("Wait for the footer and swarm workers".into()),
            Item::Tool {
                call_id: "swarm-message-example".into(),
                name: "swarm".into(),
                input: r#"{"action":"message","to_session":"session_fox_1794718668160_87674d9271df588d","message":"Please check the compact swarm row and report back.\nInclude the test result."}"#.into(),
                output: "Direct message sent to session_fox_1794718668160_87674d9271df588d: Please check the compact swarm row and report back.".into(),
                done: true,
                error: None,
                ask: None,
            },
            call(
                "session_rose_1794717290730_574aea8d9f6adcc3",
                "await_members",
                "Wait desktop-footer",
            ),
            call(
                "session_badger_1794720615830_8c546c8d2a5870c4",
                "status",
                "Check swarm-ui-compact",
            ),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("orb-todos") {
        // A live turn partway through its plan, for the tab progress fill.
        return vec![
            Item::User("Ship the tab activity redesign".into()),
            Item::Todos(TodoCardPayload {
                todos: [
                    ("Replace the orbiting comet", "completed"),
                    ("Fill the tab with todo progress", "completed"),
                    ("Glow green when done", "in_progress"),
                    ("Clear the glow once seen", "pending"),
                    ("Rebuild and reload", "pending"),
                ]
                .into_iter()
                .map(|(content, status)| TodoCardItem {
                    content: content.into(),
                    status: status.into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                })
                .collect(),
                plan: TodoCardPlan::default(),
                goals: Vec::new(),
            }),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("todos-completed") {
        return vec![
            Item::User("Keep the task group visible after the work finishes".into()),
            Item::Todos(TodoCardPayload {
                todos: ["Use the todo group name", "Verify the completed header"]
                    .into_iter()
                    .map(|content| TodoCardItem {
                        content: content.into(),
                        status: "completed".into(),
                        group: Some("Todo group header".into()),
                        blocked_by: vec![],
                        confidence: None,
                        completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                    })
                    .collect(),
                plan: TodoCardPlan::default(),
                goals: Vec::new(),
            }),
            Item::Assistant("The completed header now keeps the task group name. The green dots show that both tasks are complete.".into()),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("todos-swarm") {
        // A real swarm plan (JCODE_DESKTOP_SCREENSHOT_TODOS_FILE, the todo store's
        // JSON array) or, without one, ten finished groups and one with work.
        let todos = std::env::var_os("JCODE_DESKTOP_SCREENSHOT_TODOS_FILE")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_else(|| {
                let item = |content: String, status: &str, group: String| TodoCardItem {
                    content,
                    status: status.into(),
                    group: Some(group),
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: Some(6),
                    assigned_to: Some("session_flamingo_1_a".into()),
                };
                let mut todos: Vec<_> = (0..10)
                    .flat_map(|g| {
                        (0..3).map(move |n| {
                            item(
                                format!("finished step {g}.{n}"),
                                "completed",
                                format!("group-{g}"),
                            )
                        })
                    })
                    .collect();
                todos.push(item("prototype the bounded timeline".into(), "in_progress", "active".into()));
                todos.push(item("compare five runs".into(), "pending", "active".into()));
                todos
            });
        return vec![
            Item::User("Run the whole plan".into()),
            Item::Todos(TodoCardPayload {
                todos,
                plan: TodoCardPlan::default(),
                goals: Vec::new(),
            }),
            Item::Assistant("Working through the plan.".into()),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("prompts") {
        return prompt::fixture_items();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("gmail-draft") {
        return gmail_draft_card::fixture_items();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("mcp-list") {
        return mcp_list_card::fixture_items();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("file-mentions") {
        return vec![
            Item::Assistant("The report is in `docs/report.md`. The draft `docs/draft.md` does not exist.\n\nDetails are in docs/notes.txt:12.".into()),
            Item::Tool {
                call_id: "file-mentions-final".into(),
                name: "user_response".into(),
                input: r#"{"title":"Final","content":"Open `docs/report.md:12` to review the results."}"#.into(),
                output: String::new(),
                done: true,
                error: None,
                ask: None,
            },
        ];
    }
    if crate::harness::screenshot_mode()
        && matches!(
            fixture.as_deref(),
            Some("user-response" | "user-response-short" | "user-response-long")
        )
    {
        return user_response::fixture_items_for(fixture.as_deref().unwrap());
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("gmail-read") {
        return gmail_read_card::fixture_items();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("tool-icons") {
        return [
            "read",
            "write",
            "edit",
            "multiedit",
            "apply_patch",
            "bash",
            "ls",
            "agentgrep",
            "websearch",
            "webfetch",
            "browser",
            "gmail",
            "memory",
            "todo",
            "schedule",
            "swarm",
            "batch",
            "mcp",
            "custom_tool",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| Item::Tool {
            call_id: format!("icon-{index}"),
            name: name.into(),
            input: serde_json::json!({"intent": format!("{} tool", name)}).to_string(),
            output: "Done".into(),
            done: index != 5,
            error: (index == 11).then(|| "Example error".into()),
            ask: None,
        })
        .collect();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("transcript-scroll") {
        let mut items = vec![
            Item::Tool {
                call_id: "scroll-fixture".into(),
                name: "bash".into(),
                input: r#"{"command":"echo fixture"}"#.into(),
                output: (0..150).map(|n| format!("Fixture output {n}\n")).collect(),
                done: true,
                error: None,
                ask: None,
            },
            Item::User(
                (0..100)
                    .map(|n| format!("Prompt paragraph {n}\n\n"))
                    .collect(),
            ),
        ];
        items.extend((0..30).map(|n| Item::Assistant(format!("Response {n}"))));
        return items;
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("expandable") {
        // Every truncated class in one transcript: a cut title, a cut error and
        // an output long past the old 40-line clip.
        return vec![
            Item::User(
                "Run the release checks, then tell me why the cleanup command was refused.".into(),
            ),
            Item::Tool {
                call_id: "expandable-title".into(),
                name: "bash".into(),
                input: serde_json::json!({
                    "command": "cargo test --workspace --all-targets -- --nocapture \
                                && cargo clippy --workspace --all-targets \
                                && cargo build --release",
                    "notify": null
                })
                .to_string(),
                output: (0..60)
                    .map(|n| format!("test workspace::case_{n} ... ok"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                done: true,
                error: None,
                ask: None,
            },
            Item::Tool {
                call_id: "expandable-error".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "rm -rf ./build"}).to_string(),
                output: String::new(),
                done: true,
                error: Some(
                    "Error: This command was not run. It is irreversible:\n\n\
                     Deleting the build directory removes artifacts the user did not name.\n\
                     Before it can proceed, stop and check it against the user's actual request:\n\
                     - Which specific thing the user asked for requires deleting this?\n\
                     - Did the user name this path, or did you infer it?\n\
                     - If you inferred it, is a narrower target enough?\n\
                     If it is genuinely what the user asked for, re-issue the same call with a \
                     `justification` field explaining which request it serves."
                        .into(),
                ),
                ask: None,
            },
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("tool-streaming") {
        return tool_streaming::fixture_items();
    }
    if crate::harness::screenshot_mode()
        && matches!(
            fixture.as_deref(),
            Some(
                "empty" | "compaction" | "mode-picker" | "draft-session" | "voice-rt" | "soniox-rt"
            )
        )
    {
        return Vec::new();
    }
    if !crate::harness::screenshot_mode()
        && std::env::var("JCODE_DESKTOP_DEMO_TRANSCRIPT").as_deref() != Ok("1")
    {
        return Vec::new();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("long-history") {
        return (0..10_000)
            .map(|index| {
                let text = format!("Message {index}: **formatted** text and `inline code`.\n\nA second paragraph for layout.");
                if index % 2 == 0 { Item::User(text) } else { Item::Assistant(text) }
            })
            .collect();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("diff") {
        return diff_review::fixture_items();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("diff-rich") {
        return vec![
            Item::User("Make these changes easier to review.".into()),
            Item::Assistant(format!(
                "The title now has a clearer fallback and a Unicode-safe length limit.\n\n```diff\n{}```",
                include_str!("../../../../../../../assets/previews/change-review.diff")
            )),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("mermaid") {
        let source = std::env::var("JCODE_DESKTOP_SCREENSHOT_MERMAID_SOURCE").unwrap_or_else(|_| {
            "flowchart LR\n    A[Idea] --> B[Build]\n    B --> C[Test]\n    C -->|Pass| D[Ship]\n    C -->|Needs work| B".into()
        });
        return vec![
            Item::User("Can you render a Mermaid diagram?".into()),
            Item::Assistant(format!(
                "Here is a Mermaid diagram:\n\n```mermaid\n{source}\n```"
            )),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("tokens") {
        let mut items = vec![Item::User(
            "Show tool output token costs instead of line counts.".into(),
        )];
        for (index, (summary, tokens)) in [
            ("Small output", 1_900),
            ("Large output", 4_000),
            ("Very large output", 12_000),
            ("Empty output", 0),
            ("Single-line output", 1),
        ]
        .into_iter()
        .enumerate()
        {
            items.push(Item::Tool {
                call_id: format!("token-fixture-{index}"),
                name: "bash".into(),
                input: serde_json::json!({"command": "example", "intent": summary}).to_string(),
                output: "data".repeat(tokens),
                done: true,
                error: None,
                ask: None,
            });
        }
        items.push(Item::Assistant("The response is complete. Its totals appear below, separate from the context and account meters.".into()));
        items.push(Item::ResponseStats(response_stats::fixture()));
        return items;
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("image") {
        let mut image = image_preview::fixture_image();
        image.source = jcode_sdk::RenderedImageSource::ToolResult {
            tool_name: "read".into(),
        };
        image.anchor = Some(jcode_sdk::RenderedImageAnchor::ToolCall {
            id: "read-chart".into(),
        });
        return vec![
            Item::User("Read this chart and tell me what it shows.".into()),
            Item::Tool {
                call_id: "read-chart".into(),
                name: "read".into(),
                input: r#"{"file_path":"chart.png","intent":"Read the chart"}"#.into(),
                output: "Image loaded".into(),
                done: true,
                error: None,
                ask: None,
            },
            Item::Image(image),
            Item::Assistant(
                "The chart compares three bars. Click the image above to see it larger.".into(),
            ),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("gallery-preload") {
        return std::iter::once(Item::User("Browse images without submitting this draft.".into()))
            .chain((0..crate::harness::GALLERY_FIXTURE_IMAGES).map(|index| {
                let id = crate::harness::gallery_fixture_image_id(index);
                let label = Some(format!("[#{id}]"));
                let mut image = if index == 0 {
                    let mut image = image_preview::fixture_image();
                    image.label = label;
                    image
                } else {
                    TranscriptImage::new("image/png".into(), format!("jcode-lazy:{id}"), label)
                };
                image.source = jcode_sdk::RenderedImageSource::ToolResult { tool_name: "read".into() };
                Item::Image(image)
            }))
            .collect();
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("image-prompt") {
        // Two pasted screenshots, the first followed directly by a
        // parenthetical, as typed in a real report.
        let pasted = |label: &str| {
            let mut image = image_preview::fixture_image();
            image.label = Some(label.into());
            image.source = jcode_sdk::RenderedImageSource::UserInput;
            image.anchor = Some(jcode_sdk::RenderedImageAnchor::UserPrompt { ordinal: 0 });
            Item::Image(image)
        };
        return vec![
            Item::User(
                "The bottom inset is off on some sessions [#img_0123456789abcdef0123456789abcdef](transcription and TLDR video). \
                 And the compaction issue from [#img_abcdef0123456789abcdef0123456789] happened again."
                    .into(),
            ),
            pasted("first"),
            pasted("second"),
            Item::Assistant("Both screenshots are attached above.".into()),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("html") {
        return vec![
            Item::User("Show me different fonts directly in this chat.".into()),
            Item::Assistant(format!(
                "Here are live font pairings. Click inside to interact.\n\n```html-preview\n{}\n```",
                include_str!("../../../../../../../assets/previews/font-pairings.html")
            )),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("streaming") {
        return vec![
            Item::User("Make it easier to see which panels are still working.".into()),
            Item::Assistant("I'll add a small spinner and a subtle accent tint, while keeping the conversation easy to read.".into()),
        ];
    }
    if crate::harness::screenshot_mode() && fixture.as_deref() == Some("reasoning") {
        return vec![
            Item::User("Can you make the thinking display feel quieter?".into()),
            Item::Reasoning("The content should read like part of the conversation, not another interface to manage. I'll keep it in a **dimmed font**, aligned with the answer, and remove the surrounding labels and controls.".into()),
            Item::Reasoning("## Keep the presentation simple\n\n- No card background or border\n- No thinking label or expand button\n- Preserve the full text and Markdown formatting".into()),
            Item::Assistant(format!(
                "{}\n{}\nThinking now appears as subtle inline text. The answer keeps its normal contrast, so it's easy to tell the two apart.",
                jcode_render_core::reasoning_line_markup("**Checking top live tabs**"),
                jcode_render_core::reasoning_line_markup("**Fixing live camera geometry**"),
            )),
        ];
    }
    demo_item_fixtures()
}

/// The sample content behind `demo_items`, reachable from tests without
/// mutating process environment.
pub(super) fn demo_item_fixtures() -> Vec<Item> {
    vec![
        Item::User("Explain **markdown** rendering and show `code`, a [link](https://example.com), ~~old~~ new.".into()),
        Item::Reasoning("The user wants a survey of the renderer. I should cover blocks, inline spans, and how streaming text is handled while a turn is still in flight, then mention the tables and code paths in order.".into()),
        Item::Tool {
            call_id: "1".into(),
            name: "bash".into(),
            input: r#"{"command":"cargo test --offline","intent":"run the suite"}"#.into(),
            output: "test result: ok. 61 passed".into(),
            done: true,
            error: None,
            ask: None,
        },
        Item::Tool {
            call_id: "2".into(),
            name: "read".into(),
            input: r#"{"file_path":"src/markdown.rs"}"#.into(),
            output: String::new(),
            done: false,
            error: None,
            ask: None,
        },
        // Long ANSI-colored output: exercises the size hint on the collapsed
        // row and the stripped, head-and-tail detail when expanded.
        Item::Tool {
            call_id: "3".into(),
            name: "bash".into(),
            input: r#"{"command":"cargo build 2>&1","intent":"noisy build"}"#.into(),
            output: (0..90)
                .map(|n| format!("\u{1b}[32m   Compiling\u{1b}[0m crate-{n} v0.1.{n}\n"))
                .collect(),
            done: true,
            error: None,
            ask: None,
        },
        Item::Todos(TodoCardPayload {
            todos: vec![
                TodoCardItem {
                    content: "Add compact todo progress dots".into(),
                    status: "completed".into(),
                    group: Some("Desktop".into()),
                    blocked_by: vec![],
                    confidence: Some(jcode_base::todo::ConfidenceState::Plausible),
                    completion_confidence: Some(jcode_base::todo::ConfidenceState::Verified),
                    stage: None,
                    assigned_to: None,
                },
                TodoCardItem {
                    content: "Keep the pinned plan compact while the detailed card stays in the transcript".into(),
                    status: "in_progress".into(),
                    group: Some("Desktop".into()),
                    blocked_by: vec![],
                    confidence: Some(jcode_base::todo::ConfidenceState::Validated),
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
                TodoCardItem {
                    content: "Inspect the real offline screenshot".into(),
                    status: "pending".into(),
                    group: Some("Validation".into()),
                    blocked_by: vec![],
                    confidence: Some(jcode_base::todo::ConfidenceState::Speculative),
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
                TodoCardItem {
                    content: "Retired task".into(),
                    status: "cancelled".into(),
                    group: None,
                    blocked_by: vec![],
                    confidence: None,
                    completion_confidence: None,
                    stage: None,
                    assigned_to: None,
                },
            ],
            plan: TodoCardPlan {
                user_intention: Some("See current work without losing transcript space".into()),
                understands_user_intent: Some(jcode_base::todo::IntentUnderstanding::Clear),
            },
            goals: vec![jcode_base::todo::TodoGoal {
                group: Some("Desktop".into()),
                closed_feedback_loop: Some(jcode_base::todo::FeedbackLoopState::Usable),
                feedback_loop_relevance: Some(
                    jcode_base::todo::FeedbackLoopRelevance::Representative,
                ),
                feedback_loop_coverage: Some(jcode_base::todo::FeedbackLoopCoverage::MainPaths),
                feedback_loop_traceability: Some(
                    jcode_base::todo::FeedbackLoopTraceability::Partial,
                ),
                ..Default::default()
            }],
        }),
        Item::Assistant(
            "# Heading one\n## Heading two\n\nA paragraph with *italic*, **bold**, `inline code`, and math $e^{i\\pi}+1=0$ plus \\(n \\to \\infty\\).\n\n- top level\n  - nested item\n- [x] finished task\n- [ ] pending task\n\n1. first\n2. second\n\n> A quote line\n> continued here\n\n| block | supported |\n| --- | --- |\n| tables | yes |\n| code | yes |\n\n```rust\nfn main() {\n    // a comment\n    let name = \"world\";\n    println!(\"hello {name}\");\n}\n```\n\n$$\n\\sum_{i=0}^{n} i^2\n$$\n\n\\[ E = mc^2 \\]\n\n---\n\nDone."
                .into(),
        ),
        Item::ResponseStats(response_stats::fixture()),
        Item::Error("provider returned 429: rate limited, retrying".into()),
    ]
}
