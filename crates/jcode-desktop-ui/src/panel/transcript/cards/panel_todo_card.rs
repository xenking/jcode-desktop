//! The todo card: plan, goals and gates drawn from the todo tool's output.
use super::*;

/// Parse the todo tool's concatenated output format. The tool emits the item
/// array first, followed by optional `Plan:` and `Goals:` JSON sections.
pub(super) fn parse_todo_tool_output(output: &str) -> Option<TodoCardPayload> {
    let mut stream =
        serde_json::Deserializer::from_str(output.trim_start()).into_iter::<Vec<TodoCardItem>>();
    let todos = stream.next()?.ok()?;
    let mut remainder = output
        .trim_start()
        .get(stream.byte_offset()..)?
        .trim_start();
    let plan = if let Some(json) = remainder.strip_prefix("Plan:") {
        let json = json.trim_start();
        let mut plan_stream = serde_json::Deserializer::from_str(json).into_iter::<TodoCardPlan>();
        let plan = plan_stream.next().and_then(Result::ok).unwrap_or_default();
        remainder = json
            .get(plan_stream.byte_offset()..)
            .unwrap_or_default()
            .trim_start();
        plan
    } else {
        TodoCardPlan::default()
    };
    let goals = remainder
        .strip_prefix("Goals:")
        .and_then(|json| {
            serde_json::Deserializer::from_str(json.trim_start())
                .into_iter::<Vec<jcode_base::todo::TodoGoal>>()
                .next()
                .and_then(Result::ok)
        })
        .unwrap_or_default();
    Some(TodoCardPayload { todos, plan, goals })
}

pub(super) fn todo_status_color(todo: &TodoCardItem) -> gpui::Rgba {
    if !todo.blocked_by.is_empty() && todo.status != "completed" {
        Theme::global().WARN
    } else {
        match todo.status.as_str() {
            "completed" => Theme::global().OK,
            "in_progress" => Theme::global().ACCENT,
            "cancelled" => Theme::global().ERROR,
            _ => Theme::global().TEXT_FAINT,
        }
    }
}

pub(super) fn todoist_priority_color(priority: u8) -> gpui::Rgba {
    match priority {
        4 => Theme::global().ERROR,
        3 => Theme::global().WARN,
        2 => Theme::global().ACCENT,
        _ => Theme::global().TEXT_FAINT,
    }
}

pub(super) fn todoist_named_color(color: Option<&str>) -> gpui::Rgba {
    match color.unwrap_or_default() {
        "berry_red" | "red" => Theme::global().ERROR,
        "orange" | "yellow" => Theme::global().WARN,
        "blue" | "light_blue" | "teal" => Theme::global().ACCENT,
        "green" | "lime_green" | "mint_green" => Theme::global().AI_ACCENT,
        "purple" | "violet" | "magenta" | "lavender" => Theme::global().USER_ACCENT,
        _ => Theme::global().TEXT_FAINT,
    }
}

/// The evidence state a row's color encodes: what was achieved for completed
/// work, the planning confidence otherwise.
pub(super) fn todo_effective_confidence(todo: &TodoCardItem) -> Option<jcode_base::todo::ConfidenceState> {
    if todo.status == "completed" {
        todo.completion_confidence.or(todo.confidence)
    } else {
        todo.confidence
    }
}

/// Confidence palette, a red to green ramp: speculative red, plausible
/// amber, validated lime, verified green.
pub(super) fn todo_confidence_color(state: jcode_base::todo::ConfidenceState) -> gpui::Rgba {
    use jcode_base::todo::ConfidenceState as C;
    let theme = Theme::global();
    let mix = |a: gpui::Rgba, b: gpui::Rgba, t: f32| gpui::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    };
    match state {
        C::Speculative => theme.ERROR,
        C::Plausible => theme.WARN,
        C::Validated => mix(theme.WARN, theme.OK, 0.6),
        C::Verified => theme.OK,
    }
}

/// Status is the marker's shape (empty ring, inner dot, filled) and
/// confidence its color, so rows need no trailing text label. Blocked and
/// cancelled rows keep their status colors. Hovering names the confidence.
pub(super) fn render_todo_marker(todo: &TodoCardItem, id: String) -> impl IntoElement {
    let blocked = !todo.blocked_by.is_empty() && todo.status != "completed";
    let confidence = todo_effective_confidence(todo);
    let color = match confidence {
        _ if todo.status == "cancelled" => Theme::global().TEXT_FAINT,
        Some(state) if !blocked => todo_confidence_color(state),
        _ => todo_status_color(todo),
    };
    let label = todo_confidence_label(todo);
    div()
        .id(gpui::SharedString::from(id))
        .when(label.is_some(), |marker| {
            marker.debug_selector(|| "todo-row-confidence".into())
        })
        .flex_none()
        .w(px(13.0))
        .h(px(13.0))
        .rounded_full()
        .border_1()
        .border_color(color)
        .p(px(2.0))
        .when(todo.status == "completed", |marker| {
            marker.child(div().size_full().rounded_full().bg(color))
        })
        .when(todo.status == "in_progress", |marker| {
            let dot = if confidence.is_some() {
                color
            } else {
                Theme::global().ACCENT
            };
            marker
                .p(px(3.0))
                .child(div().size_full().rounded_full().bg(dot))
        })
        .when_some(label, |marker, label| {
            let tip = format!("Confidence: {label}");
            marker.tooltip(move |_, cx| {
                cx.new(|_| crate::panel::meter::MeterTooltip(tip.clone()))
                    .into()
            })
        })
}

/// Severity of one todo assessment value, mirroring the TUI todo card palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TodoTone {
    Ok,
    Warn,
    Fail,
}

pub(super) fn todo_tone_color(tone: TodoTone) -> gpui::Rgba {
    match tone {
        TodoTone::Ok => Theme::global().OK,
        TodoTone::Warn => Theme::global().WARN,
        TodoTone::Fail => Theme::global().ERROR,
    }
}

/// One labeled gate value shown under a todo group, such as `Coverage main_paths`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TodoGateSegment {
    pub(super) label: &'static str,
    pub(super) state: String,
    pub(super) tone: TodoTone,
}

/// The goal's quality gates as the TUI shows them: only failing gates are
/// listed, a single passing line otherwise, and delivery progress last.
pub(super) fn todo_goal_gate_segments(goal: &jcode_base::todo::TodoGoal) -> Vec<TodoGateSegment> {
    use jcode_base::todo as t;
    fn gate<S: Copy>(
        label: &'static str,
        state: Option<S>,
        name: impl Fn(S) -> &'static str,
        severe: impl Fn(S) -> bool,
    ) -> TodoGateSegment {
        match state {
            None => TodoGateSegment {
                label,
                state: "missing".into(),
                tone: TodoTone::Fail,
            },
            Some(state) => TodoGateSegment {
                label,
                state: name(state).into(),
                tone: if severe(state) {
                    TodoTone::Fail
                } else {
                    TodoTone::Warn
                },
            },
        }
    }

    let mut segments = Vec::new();
    if !t::feedback_loop_passes(goal.closed_feedback_loop) {
        segments.push(gate(
            "Closed feedback loop",
            goal.closed_feedback_loop,
            |s| s.as_str(),
            |s| s <= t::FeedbackLoopState::Weak,
        ));
    }
    if !t::feedback_loop_relevance_passes(goal) {
        segments.push(gate(
            "Relevance",
            goal.feedback_loop_relevance,
            |s| s.as_str(),
            |s| s == t::FeedbackLoopRelevance::Indirect,
        ));
    }
    if !t::feedback_loop_coverage_passes(goal) {
        segments.push(gate(
            "Coverage",
            goal.feedback_loop_coverage,
            |s| s.as_str(),
            |s| s == t::FeedbackLoopCoverage::Narrow,
        ));
    }
    if !t::feedback_loop_traceability_passes(goal) {
        segments.push(gate(
            "Traceability",
            goal.feedback_loop_traceability,
            |s| s.as_str(),
            |s| s == t::FeedbackLoopTraceability::Unmapped,
        ));
    }
    if segments.is_empty() {
        segments.push(TodoGateSegment {
            label: "",
            state: "✓ All quality gates passing".into(),
            tone: TodoTone::Ok,
        });
    }
    if let Some(state) = goal.delivery_state {
        segments.push(TodoGateSegment {
            label: "Delivery",
            state: state.as_str().into(),
            tone: if state >= t::DeliveryState::WorkflowValidated {
                TodoTone::Ok
            } else if state == t::DeliveryState::Integrated {
                TodoTone::Warn
            } else {
                TodoTone::Fail
            },
        });
    }
    segments
}

pub(super) fn todo_goal_for_group<'a>(
    goals: &'a [jcode_base::todo::TodoGoal],
    group: Option<&str>,
) -> Option<&'a jcode_base::todo::TodoGoal> {
    let key = group.map(str::trim).filter(|value| !value.is_empty());
    goals.iter().find(|goal| {
        goal.group
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            == key
    })
}

/// Evidence label for a todo row, matching the TUI: completed rows show how
/// planning confidence moved, others their current confidence.
pub(super) fn todo_confidence_label(todo: &TodoCardItem) -> Option<String> {
    if todo.status == "completed" {
        if let (Some(planning), Some(completed)) = (todo.confidence, todo.completion_confidence)
            && planning != completed
        {
            return Some(format!("{}→{}", planning.as_str(), completed.as_str()));
        }
        return todo
            .completion_confidence
            .or(todo.confidence)
            .map(|state| state.as_str().to_string());
    }
    todo.confidence.map(|state| state.as_str().to_string())
}

/// The gate row as one text run with colored spans, so it truncates with an
/// ellipsis on a single line like task rows instead of wrapping.
pub(super) fn todo_gate_row_text(
    goal: &jcode_base::todo::TodoGoal,
) -> (String, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>) {
    let theme = Theme::global();
    let mut text = String::new();
    let mut highlights = Vec::new();
    let mut push = |text: &mut String, part: &str, color: gpui::Rgba| {
        let start = text.len();
        text.push_str(part);
        highlights.push((
            start..text.len(),
            gpui::HighlightStyle {
                color: Some(color.into()),
                ..Default::default()
            },
        ));
    };
    for (index, segment) in todo_goal_gate_segments(goal).into_iter().enumerate() {
        if index > 0 {
            push(&mut text, " · ", theme.TEXT_FAINT);
        }
        if !segment.label.is_empty() {
            push(&mut text, segment.label, theme.TEXT_DIM);
            push(&mut text, " ", theme.TEXT_DIM);
        }
        push(&mut text, &segment.state, todo_tone_color(segment.tone));
    }
    (text, highlights)
}

pub(super) fn render_todo_gate_row(goal: &jcode_base::todo::TodoGoal) -> impl IntoElement {
    let (text, highlights) = todo_gate_row_text(goal);
    div()
        .debug_selector(|| "todo-goal-gates".into())
        .w_full()
        .min_w_0()
        .pl(px(19.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(11.0))
        .child(gpui::StyledText::new(text).with_highlights(highlights))
}

pub(super) fn intent_understanding_tone(state: jcode_base::todo::IntentUnderstanding) -> TodoTone {
    use jcode_base::todo::IntentUnderstanding as I;
    match state {
        I::Uncertain => TodoTone::Fail,
        I::Partial => TodoTone::Warn,
        I::Clear | I::Complete => TodoTone::Ok,
    }
}

pub(super) fn render_todo_card_with_style(
    payload: &TodoCardPayload,
    pinned: bool,
    key: &str,
    selection: &Entity<TextSelection>,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    let intention = payload
        .plan
        .user_intention
        .as_deref()
        .map(str::trim)
        .filter(|intention| !intention.is_empty())
        .map(str::to_owned);
    let total = payload.todos.len();
    let completed = payload
        .todos
        .iter()
        .filter(|todo| todo.status == "completed")
        .count();
    let progress = if total == 0 {
        0.0
    } else {
        completed as f32 / total as f32
    };

    let mut groups: Vec<(Option<u32>, Option<&str>, Vec<&TodoCardItem>)> = Vec::new();
    for todo in &payload.todos {
        let group = todo
            .group
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty());
        let stage = todo.stage;
        if let Some((_, _, items)) = groups
            .iter_mut()
            .find(|(known_stage, known, _)| *known_stage == stage && *known == group)
        {
            items.push(todo);
        } else {
            groups.push((stage, group, vec![todo]));
        }
    }
    groups.sort_by_key(|(stage, group, _)| (*stage, group.is_none()));

    let mut body = div()
        .id("todo-card-body-scroll")
        .debug_selector(|| "todo-card-body".into())
        .flex()
        .flex_col()
        .w_full()
        .gap_0p5()
        .when(!pinned, |body| body.max_h(px(132.0)));
    if payload.todos.is_empty() {
        body = body.child(
            div()
                .py_1()
                .text_size(px(12.0))
                .text_color(Theme::global().TEXT_DIM)
                .child("No tasks yet. Jcode will populate them as work is planned."),
        );
    } else {
        for (group_index, (stage, group, todos)) in groups.into_iter().enumerate() {
            let done = todos
                .iter()
                .filter(|todo| todo.status == "completed")
                .count();
            // GPUI does not always stretch nested flex rows through a scrolling
            // column. Give every level an explicit width so each row's flex_1
            // text child receives space instead of collapsing to zero width.
            let mut section = div().flex().flex_col().w_full();
            if let Some(stage) = stage {
                section = section.child(
                    div()
                        .debug_selector(move || format!("pinned-todo-phase-{stage}"))
                        .text_size(px(10.0))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(format!("Phase {stage}")),
                );
            }
            let has_any_group = payload.todos.iter().any(|todo| todo.group.is_some());
            if group.is_some() || has_any_group {
                section = section.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(10.0))
                        .pt_0p5()
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_weight(FontWeight::BOLD)
                                .text_color(Theme::global().ACCENT_MUTED)
                                .child(text_selection::plain(
                                    selection.clone(),
                                    format!("{key}-group-{group_index}"),
                                    group.unwrap_or("Other").to_uppercase(),
                                    window,
                                    cx,
                                )),
                        )
                        .child(
                            div()
                                .text_color(Theme::global().TEXT_FAINT)
                                .child(format!("{done}/{}", todos.len())),
                        ),
                );
            }
            if let Some(goal) = todo_goal_for_group(&payload.goals, group) {
                section = section.child(render_todo_gate_row(goal));
            }
            for (todo_index, todo) in todos.into_iter().enumerate() {
                section = section.child(
                    div()
                        .debug_selector(|| "todo-row".into())
                        .flex()
                        .w_full()
                        .items_center()
                        .gap_1p5()
                        .child(render_todo_marker(
                            todo,
                            format!("{key}-marker-{group_index}-{todo_index}"),
                        ))
                        .child(
                            div()
                                .debug_selector(|| "todo-row-content".into())
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(if pinned { 12.0 } else { 12.5 }))
                                .text_color(if todo.status == "completed" {
                                    Theme::global().TEXT_DIM
                                } else if pinned {
                                    Theme::global().TEXT_USER
                                } else {
                                    Theme::global().TEXT
                                })
                                .child(text_selection::plain(
                                    selection.clone(),
                                    format!("{key}-task-{group_index}-{todo_index}"),
                                    todo.content.clone(),
                                    window,
                                    cx,
                                )),
                        )
                        .children(todo.assigned_to.as_ref().map(|assignee| {
                            let full = assignee.clone();
                            let label = assignee
                                .strip_prefix("session_")
                                .and_then(|rest| rest.split('_').next())
                                .unwrap_or(assignee)
                                .to_owned();
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "{key}-assignee-{group_index}-{todo_index}"
                                )))
                                .debug_selector(move || {
                                    format!("pinned-todo-assignee-{group_index}-{todo_index}")
                                })
                                .flex_none()
                                .text_size(px(10.0))
                                .text_color(Theme::global().TEXT_FAINT)
                                .tooltip(move |_, cx| {
                                    cx.new(|_| meter::MeterTooltip(full.clone())).into()
                                })
                                .child(label)
                        })),
                );
            }
            body = body.child(section);
        }
    }

    let body = if pinned {
        body.into_any_element()
    } else {
        crate::transcript_scroll::vertical(body).into_any_element()
    };
    if pinned {
        return div()
            .debug_selector(|| "pinned-todo-details-paper".into())
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(8.0))
            .bg(Theme::global().prompt_background(0))
            .px_2()
            .py_1p5()
            .child(
                div()
                    .debug_selector(|| "pinned-todo-intent-row".into())
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(12.0))
                    .when_some(payload.plan.understands_user_intent, |line, state| {
                        // Mirror the TUI: "Intent clear: <intention>".
                        line.child(
                            div()
                                .debug_selector(|| "pinned-todo-intent-state".into())
                                .flex_none()
                                .flex()
                                .child(
                                    div()
                                        .text_color(Theme::global().TEXT_DIM)
                                        .child("Intent\u{00a0}"),
                                )
                                .child(
                                    div()
                                        .text_color(todo_tone_color(intent_understanding_tone(
                                            state,
                                        )))
                                        .child(state.as_str()),
                                )
                                .child(div().text_color(Theme::global().TEXT_DIM).child(":")),
                        )
                    })
                    .child(
                        div()
                            .debug_selector(|| "pinned-todo-intention".into())
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(Theme::global().TEXT_USER)
                            .when_some(intention, |line, intention| {
                                line.child(text_selection::plain(
                                    selection.clone(),
                                    format!("{key}-intention"),
                                    intention,
                                    window,
                                    cx,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "pinned-todo-count".into())
                            .flex_none()
                            .font_family(Theme::global().FONT_MONO)
                            .text_size(px(10.5))
                            .text_color(Theme::global().TEXT_DIM)
                            .child({
                                // Cancelled work is not outstanding progress.
                                let active = payload
                                    .todos
                                    .iter()
                                    .filter(|todo| todo.status != "cancelled")
                                    .count();
                                format!("{completed}/{active}")
                            }),
                    ),
            )
            .child(body)
            .into_any_element();
    }

    div()
        .debug_selector(|| "todo-card".into())
        .flex()
        .flex_none()
        .flex_col()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(Theme::global().TOOL_BORDER)
        .bg(Theme::global().TOOL_BG)
        .px_2()
        .py_1p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(crate::tool_icon::render_status("todo", true, false))
                .when_some(intention, |header, intention| {
                    header.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(Theme::global().HEADING)
                            .child(text_selection::plain(
                                selection.clone(),
                                format!("{key}-intention"),
                                intention,
                                window,
                                cx,
                            )),
                    )
                })
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Theme::global().TEXT_DIM)
                        .child(format!("{completed} of {total} complete")),
                ),
        )
        .child(
            div()
                .h(px(3.0))
                .w_full()
                .rounded_full()
                .bg(Theme::global().CODE_BORDER)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(progress))
                        .rounded_full()
                        .bg(Theme::global().OK),
                ),
        )
        .child(body)
        .into_any_element()
}
