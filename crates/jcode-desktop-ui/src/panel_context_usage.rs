use super::*;
use crate::fixture_control_targets::marker;
use crate::harness::ContextBreakdownTarget;
use jcode_sdk::api::{
    ContextActionForecast, ContextActionMeasurement, ContextActionSavings, ContextActionStatus,
    ContextBreakdown, ContextBreakdownKind, ContextMeter, ContextMeterState,
    ContextOccupancySource, ShakeToolsAction, ShakeToolsResult, ShakeToolsStats,
};

#[derive(Default)]
pub(super) struct State {
    open: bool,
    read: Option<Task<()>>,
    refresh_queued: bool,
    primed: bool,
    meter: Option<ContextMeter>,
    breakdown: Option<ContextBreakdown>,
    action: ActionState,
    shake_supported: bool,
    shake_task: Option<Task<()>>,
    pub(super) compacting: bool,
    compact_measurement_supported: bool,
}

pub(super) struct ContextDisplay {
    meter: ContextMeter,
}

fn format_context_tokens(tokens: u64) -> String {
    format_tokens(tokens)
        .replace(".0k", "k")
        .replace(".0m", "m")
}

impl ContextDisplay {
    fn threshold(&self) -> Option<u64> {
        self.meter.compaction_threshold_tokens.filter(|&n| n > 0)
    }

    pub(super) fn percent(&self) -> Option<f32> {
        self.meter
            .occupied_tokens
            .zip(self.threshold())
            .map(|(used, threshold)| (used as f64 / threshold as f64 * 100.) as f32)
    }

    pub(super) fn value(&self) -> String {
        let occupied = self.meter.occupied_tokens.map(|used| {
            let prefix = match self.meter.source {
                ContextOccupancySource::LocalEstimate
                | ContextOccupancySource::CompactionEstimate => "~",
                _ => "",
            };
            format!("{prefix}{}", format_context_tokens(used))
        });
        match (occupied, self.threshold()) {
            (Some(used), Some(threshold)) => {
                format!("{used} / {}", format_context_tokens(threshold))
            }
            (None, Some(threshold)) => format!("? / {}", format_context_tokens(threshold)),
            (Some(used), None) => used,
            (None, None) => "Unknown".into(),
        }
    }

    fn scale(&self) -> Option<u64> {
        self.threshold().or_else(|| {
            self.meter
                .effective_budget_tokens
                .filter(|&n| n > 0)
                .or(self.meter.model_window_tokens.filter(|&n| n > 0))
        })
    }
}

impl State {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
}

fn category(kind: ContextBreakdownKind, theme: &Theme) -> (&'static str, gpui::Rgba) {
    match kind {
        ContextBreakdownKind::SystemPrompt => ("System prompt", theme.TEXT_DIM),
        ContextBreakdownKind::BuiltinTools => ("Tool definitions", theme.CODE_KEYWORD),
        ContextBreakdownKind::SkillsModes => ("Skills & modes", theme.WARN),
        ContextBreakdownKind::DynamicTools => ("MCP & dynamic tools", theme.CODE_ATTRIBUTE),
        ContextBreakdownKind::Agents => ("Subagent definitions", theme.LINK),
        ContextBreakdownKind::Conversation => ("Conversation", theme.CODE_STRING),
        ContextBreakdownKind::Snapcompact => ("Snapcompact checkpoint", theme.CODE_CONTROL),
        ContextBreakdownKind::Unattributed => ("Unattributed", theme.TEXT_FAINT),
    }
}

fn legend_row(label: &'static str, color: gpui::Rgba, value: String, theme: &Theme) -> gpui::Div {
    let selector = format!("context-usage-count-{label}-{value}");
    div()
        .debug_selector(move || format!("context-usage-row-{label}"))
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(div().flex_none().size(px(10.)).rounded_sm().bg(color))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .child(
            div()
                .debug_selector(move || selector)
                .flex_none()
                .font_family(theme.FONT_MONO)
                .text_color(theme.composer_secondary())
                .child(value),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Compact,
    Shake,
}

#[derive(Default)]
enum ActionState {
    #[default]
    Idle,
    Running(Action),
    ConfirmShake(ShakePreview),
    Finished {
        label: String,
        error: bool,
    },
}

struct ShakePreview {
    confirmation: String,
    cache_at_risk: bool,
    stats: ShakeToolsStats,
}

impl Action {
    fn selector(self) -> &'static str {
        match self {
            Self::Compact => "context-usage-compact",
            Self::Shake => "context-usage-shake",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact now",
            Self::Shake => "Shake tools",
        }
    }

    fn busy_label(self) -> &'static str {
        match self {
            Self::Compact => "Compacting…",
            Self::Shake => "Shaking tools…",
        }
    }
}

fn percent_of(tokens: u64, total: u64) -> String {
    let percent = 100. * tokens as f64 / total as f64;
    if tokens == 0 {
        "0%".into()
    } else if percent < 0.1 {
        "<0.1%".into()
    } else {
        format!("{percent:.1}%")
    }
}

fn action_forecast_label(action: Action, forecast: ContextActionForecast) -> Option<String> {
    let (status, savings) = match action {
        Action::Compact => (forecast.compact_status, forecast.compact),
        Action::Shake => (forecast.shake_status, forecast.shake),
    };
    let status = status.or(match savings {
        Some(ContextActionSavings::NoChange) => Some(ContextActionStatus::NoEligible),
        Some(ContextActionSavings::UpperBound { .. }) => Some(ContextActionStatus::SavingsUnknown),
        _ => None,
    });
    if let Some(status) = status {
        return Some(
            match (action, status) {
                (Action::Shake, ContextActionStatus::NoEligible) => "No eligible outputs",
                (_, ContextActionStatus::NoEligible) => "Nothing eligible",
                (_, ContextActionStatus::Unsupported) => "Unsupported",
                (_, ContextActionStatus::Busy) => "Busy",
                (_, ContextActionStatus::SavingsUnknown) => "Savings unknown",
                (_, ContextActionStatus::Unknown) => "Not inspected",
            }
            .into(),
        );
    }
    match savings? {
        ContextActionSavings::Estimate { tokens } => {
            let total = forecast
                .current_tokens
                .filter(|&total| tokens > 0 && tokens <= total)?;
            let percent = percent_of(tokens, total);
            Some(if percent.starts_with('<') {
                percent
            } else {
                format!("~{percent}")
            })
        }
        _ => None,
    }
}

fn action_result(label: &str, measurement: Option<ContextActionMeasurement>) -> String {
    let Some(ContextActionMeasurement {
        before_tokens,
        after_tokens,
    }) = measurement
    else {
        return format!("{label} Before/after unavailable.");
    };
    let change = match before_tokens.cmp(&after_tokens) {
        std::cmp::Ordering::Greater => format!(
            " · {} saved",
            percent_of(before_tokens - after_tokens, before_tokens)
        ),
        std::cmp::Ordering::Equal => " · no change".into(),
        std::cmp::Ordering::Less => " · context grew".into(),
    };
    format!(
        "{label} Local estimate: {} → {} tokens{change}.",
        format_tokens(before_tokens),
        format_tokens(after_tokens),
    )
}

#[cfg(test)]
mod action_forecast_tests {
    use super::*;

    #[test]
    fn action_labels_distinguish_no_change_unknown_busy_unsupported_and_rounding() {
        let legacy: ContextActionForecast = serde_json::from_str(
            r#"{"current_tokens":400000,"compact":{"kind":"upper_bound","tokens":70000},"shake":{"kind":"no_change"}}"#,
        )
        .unwrap();
        assert_eq!(
            action_forecast_label(Action::Compact, legacy).as_deref(),
            Some("Savings unknown")
        );
        assert_eq!(
            action_forecast_label(Action::Shake, legacy).as_deref(),
            Some("No eligible outputs")
        );
        for (status, label) in [
            (ContextActionStatus::NoEligible, "Nothing eligible"),
            (ContextActionStatus::Unsupported, "Unsupported"),
            (ContextActionStatus::Busy, "Busy"),
            (ContextActionStatus::SavingsUnknown, "Savings unknown"),
            (ContextActionStatus::Unknown, "Not inspected"),
        ] {
            assert_eq!(
                action_forecast_label(Action::Compact, ContextActionForecast::unavailable(status))
                    .as_deref(),
                Some(label)
            );
        }
        let tiny = ContextActionForecast {
            shake: Some(ContextActionSavings::Estimate { tokens: 1 }),
            ..legacy
        };
        assert_eq!(
            action_forecast_label(Action::Shake, tiny).as_deref(),
            Some("<0.1%")
        );
    }
}

impl Panel {
    pub(super) fn finish_manual_context_compaction(
        &mut self,
        triggered_manually: bool,
        before: Option<u64>,
        after: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let measurement = before
            .zip(after)
            .filter(|_| triggered_manually && self.context_usage.compact_measurement_supported)
            .map(|(before_tokens, after_tokens)| ContextActionMeasurement {
                before_tokens,
                after_tokens,
            });
        self.settle_context_action(
            Action::Compact,
            action_result("Context compacted in this session.", measurement),
            false,
            cx,
        );
    }

    pub(super) fn settle_context_action(
        &mut self,
        action: Action,
        label: String,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.context_usage.action, ActionState::Running(current) if current == action) {
            self.context_usage.action = ActionState::Finished { label, error };
            cx.notify();
        }
        if !error {
            self.refresh_context_usage(cx);
        }
    }

    pub(super) fn lose_context_action(&mut self) {
        self.context_usage.read = None;
        self.context_usage.refresh_queued = false;
        self.context_usage.compacting = false;
        self.context_usage.compact_measurement_supported = false;
        self.context_usage.shake_supported = false;
        self.context_usage.shake_task = None;
        if matches!(
            self.context_usage.action,
            ActionState::Running(_) | ActionState::ConfirmShake(_)
        ) {
            self.context_usage.action = ActionState::Finished {
                label: "Connection lost. Action outcome unknown.".into(),
                error: true,
            };
        }
    }

    fn context_action_enabled(&self, action: Action) -> bool {
        self.context_can_mutate()
            && !matches!(
                self.context_usage.action,
                ActionState::Running(_) | ActionState::ConfirmShake(_)
            )
            && (action == Action::Compact || self.context_usage.shake_supported)
    }

    fn context_can_mutate(&self) -> bool {
        !self.context_usage.compacting
            && !self.activity_active()
            && !self.is_pending_session()
            && !self.session_deferred()
            && !self.is_side_document()
            && self.preview_state.is_none()
            && !self.status.starts_with("lost:")
    }

    fn run_shake_tools(&mut self, action: ShakeToolsAction, cx: &mut Context<Self>) {
        let session_id = self.session_id.clone();
        let bridge = self.bridge.clone();
        let is_preview = matches!(action, ShakeToolsAction::Preview);
        self.context_usage.action = ActionState::Running(Action::Shake);
        self.context_usage.shake_task = Some(cx.spawn(async move |panel, cx| {
            let result = bridge.shake_tools(session_id.clone(), action).await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.session_id != session_id
                    || !matches!(
                        panel.context_usage.action,
                        ActionState::Running(Action::Shake)
                    )
                {
                    return;
                }
                panel.context_usage.shake_task = None;
                match result {
                    Ok(ShakeToolsResult::Preview {
                        confirmation,
                        cache_at_risk,
                        stats,
                    }) if is_preview => {
                        panel.context_usage.action = if panel.context_usage.open {
                            ActionState::ConfirmShake(ShakePreview {
                                confirmation,
                                cache_at_risk,
                                stats,
                            })
                        } else {
                            ActionState::Idle
                        };
                    }
                    Ok(ShakeToolsResult::Applied { stats, measurement }) if !is_preview => panel
                        .settle_context_action(
                            Action::Shake,
                            action_result(
                                &format!(
                                    "{} {} shortened.",
                                    stats.outputs_elided,
                                    if stats.outputs_elided == 1 {
                                        "output"
                                    } else {
                                        "outputs"
                                    },
                                ),
                                measurement,
                            ),
                            false,
                            cx,
                        ),
                    Ok(ShakeToolsResult::NoChange) => {
                        panel.context_usage.action = ActionState::Finished {
                            label: "No eligible tool outputs to shake. Context unchanged.".into(),
                            error: false,
                        };
                    }
                    Err(error) => panel.settle_context_action(Action::Shake, error, true, cx),
                    _ => panel.settle_context_action(
                        Action::Shake,
                        "Unexpected Shake response.".into(),
                        true,
                        cx,
                    ),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn toggle_context_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.context_usage.is_open() {
            self.close_context_usage(cx);
        } else {
            self.context_usage.open = true;
            window.focus(&self.focus_handle, cx);
            self.refresh_context_usage(cx);
            cx.notify();
        }
    }

    pub(super) fn close_context_usage(&mut self, cx: &mut Context<Self>) {
        if !self.context_usage.open {
            return;
        }
        self.context_usage.open = false;
        self.context_usage.breakdown = None;
        if matches!(self.context_usage.action, ActionState::ConfirmShake(_)) {
            self.context_usage.action = ActionState::Idle;
        }
        cx.notify();
    }

    pub(crate) fn refresh_context_usage(&mut self, cx: &mut Context<Self>) {
        if self.status.starts_with("lost:")
            || self.is_side_document()
            || (self.is_pending_session() && !self.session_deferred())
            || (self.session_deferred() && !self.context_usage.is_open())
            || (!self.context_usage.is_open() && !self.context_usage.primed)
        {
            return;
        }
        if self.context_usage.read.is_some() {
            self.context_usage.refresh_queued = true;
            return;
        }
        let summary_only = !self.context_usage.is_open();
        let target = if self.session_deferred() {
            ContextBreakdownTarget::Draft {
                model: self.model.clone(),
                cwd: self.working_dir.clone(),
            }
        } else if !summary_only {
            ContextBreakdownTarget::Session(self.session_id.clone())
        } else {
            ContextBreakdownTarget::SessionMeter(self.session_id.clone())
        };
        let bridge = self.bridge.clone();
        self.context_usage.read = Some(cx.spawn(async move |panel, cx| {
            let inspection = bridge.read_context_breakdown(target).await;
            let _ = panel.update(cx, |panel, cx| {
                if let Some(breakdown) = inspection.breakdown {
                    if let Some(meter) = &breakdown.meter
                        && (meter.state != ContextMeterState::Unavailable
                            || panel.context_usage.meter.is_none())
                    {
                        panel.context_usage.meter = Some(meter.clone());
                    }
                    if panel.context_usage.is_open() && !summary_only {
                        panel.context_usage.breakdown = Some(breakdown);
                    }
                }
                panel.context_usage.shake_supported = inspection.shake_supported;
                panel.context_usage.read = None;
                if std::mem::take(&mut panel.context_usage.refresh_queued) {
                    panel.refresh_context_usage(cx);
                }
                cx.notify();
            });
        }));
    }

    pub(crate) fn prime_context_meter(&mut self, cx: &mut Context<Self>) {
        self.context_usage.primed = true;
        self.refresh_context_usage(cx);
    }

    pub(super) fn invalidate_context_meter(&mut self) {
        self.context_usage.meter = None;
        self.context_usage.breakdown = None;
        self.context_usage.read = None;
        self.context_usage.refresh_queued = false;
        self.context_tokens = None;
    }

    pub(super) fn set_compacted_context_estimate(&mut self, post_tokens: u64) {
        self.context_usage.breakdown = None;
        self.context_usage.read = None;
        self.context_usage.refresh_queued = false;
        if let Some(meter) = &mut self.context_usage.meter {
            meter.occupied_tokens = Some(post_tokens);
            meter.source = ContextOccupancySource::CompactionEstimate;
            meter.measured_at_ms = None;
            meter.route = None;
        }
    }

    pub(super) fn context_display(&self) -> ContextDisplay {
        let meter = self.context_usage.meter.clone().unwrap_or_else(|| {
            let occupied = self
                .context_tokens
                .or_else(|| self.context_usage.breakdown.as_ref()?.total_tokens);
            ContextMeter {
                occupied_tokens: occupied,
                source: if occupied.is_some() {
                    ContextOccupancySource::HistoricalReport
                } else {
                    ContextOccupancySource::Unknown
                },
                model_window_tokens: self.model.as_deref().and_then(context_window_for_model),
                ..ContextMeter::default()
            }
        });
        ContextDisplay { meter }
    }

    pub(super) fn render_context_usage(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if !self.context_usage.is_open() {
            return None;
        }
        let theme = Theme::global();
        let display = self.context_display();
        let percent = display.percent();
        let value = display.value();
        let selector = format!("context-usage-value-{value}");
        let categories: Vec<_> = self
            .context_usage
            .breakdown
            .iter()
            .flat_map(|snapshot| &snapshot.categories)
            .filter(|bucket| {
                bucket.kind != ContextBreakdownKind::Snapcompact
                    || bucket.tokens.is_some_and(|tokens| tokens > 0)
            })
            .map(|bucket| {
                let (label, color) = category(bucket.kind, theme);
                (label, color, bucket.tokens)
            })
            .collect();
        let scale = display.scale();
        let usage = display.meter.occupied_tokens.zip(scale);
        let occupied = usage.map(|(used, scale)| (used as f64 / scale as f64).min(1.) as f32);
        let remaining = usage.map(|(used, scale)| scale.saturating_sub(used));
        let free_color = gpui::rgba(0x8c8c8c55);
        let cutoff = display
            .meter
            .long_context_pricing_above_input_tokens
            .filter(|&n| n > 0);
        let marker_fraction = cutoff.zip(scale).and_then(|(cutoff, scale)| {
            (cutoff < scale).then(|| (cutoff as f64 / scale as f64) as f32)
        });
        let sum = categories
            .iter()
            .filter_map(|(_, _, tokens)| *tokens)
            .fold(0u64, u64::saturating_add);
        let unmatched = display
            .meter
            .occupied_tokens
            .map(|used| used.saturating_sub(sum));
        // Keep any unmatched occupancy neutral. If estimates overshoot, scale them to fit.
        let category_extent = display
            .meter
            .occupied_tokens
            .map_or(sum, |used| sum.max(used));
        let owner = cx.weak_entity();
        let card = div()
            .id("context-usage")
            .debug_selector(|| "context-usage".into())
            .relative()
            .children(marker(
                || "context-usage".into(),
                || Some(self.session_id.clone()),
            ))
            .w_full()
            .min_w_0()
            .max_h_full()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_xl()
            .border_1()
            .border_color(theme.INPUT_BORDER)
            .bg(theme.popover_background())
            .shadow_lg()
            .occlude()
            .font_family(theme.FONT_UI)
            .text_size(px(13.))
            .text_color(theme.TEXT)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .w_full()
                    .items_center()
                    .flex_wrap()
                    .justify_between()
                    .gap_x_2()
                    .gap_y_0p5()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(94.))
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Context Usage"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .ml_auto()
                            .max_w_full()
                            .flex()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .child(self.render_context_action(Action::Compact, cx))
                            .child(self.render_context_action(Action::Shake, cx)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .when(percent.is_some(), |row| row.justify_between())
                    .when(percent.is_none(), |row| row.justify_end())
                    .flex_wrap()
                    .gap_x_2()
                    .text_size(px(12.))
                    .text_color(theme.composer_secondary())
                    .children(
                        percent
                            .map(|percent| div().flex_none().child(format!("{percent:.0}% used"))),
                    )
                    .child(div().flex_none().debug_selector(move || selector).child(
                        if display.threshold().is_some() {
                            format!("{value} to compact")
                        } else if display.meter.occupied_tokens.is_some() {
                            format!("{value} tokens")
                        } else {
                            "Occupancy unknown".into()
                        },
                    )),
            )
            .child(
                div()
                    .debug_selector(|| "context-usage-bar".into())
                    .relative()
                    .w_full()
                    .h(px(8.))
                    .flex_none()
                    .flex()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(free_color)
                    .children(occupied.map(|occupied| {
                        div()
                            .debug_selector(|| "context-usage-occupied".into())
                            .w(relative(occupied))
                            .h_full()
                            .flex_none()
                            .flex()
                            .bg(theme.TEXT_FAINT)
                            .children(categories.iter().filter_map(|(label, color, tokens)| {
                                let tokens = tokens.filter(|&tokens| tokens > 0)?;
                                let label = *label;
                                Some(
                                    div()
                                        .debug_selector(move || {
                                            format!("context-usage-segment-{label}")
                                        })
                                        .flex_none()
                                        .w(relative(
                                            (tokens as f64 / category_extent as f64) as f32,
                                        ))
                                        .h_full()
                                        .bg(*color),
                                )
                            }))
                    }))
                    .children(marker_fraction.map(|offset| {
                        div()
                            .debug_selector(|| "context-usage-billing-marker".into())
                            .absolute()
                            .left(relative(offset))
                            .top_0()
                            .h_full()
                            .w(px(2.))
                            .bg(theme.WARN)
                    })),
            )
            .children(cutoff.map(|cutoff| {
                div()
                    .debug_selector(|| "context-usage-billing-label".into())
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme.composer_secondary())
                    .child(format!(
                        "Higher rates above {} input tokens per request",
                        format_context_tokens(cutoff)
                    ))
            }))
            .when(!categories.is_empty(), |panel| {
                panel.child(
                    div()
                        .debug_selector(|| "context-usage-breakdown".into())
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_size(px(12.))
                        .children(categories.iter().map(|(label, color, tokens)| {
                            let value = tokens.map_or_else(
                                || "—".into(),
                                |tokens| format!("~{}", format_context_tokens(tokens)),
                            );
                            legend_row(label, *color, value, theme)
                        }))
                        .children(unmatched.filter(|&tokens| tokens > 0).map(|tokens| {
                            legend_row(
                                "Unmatched estimate",
                                theme.TEXT_FAINT,
                                format_context_tokens(tokens),
                                theme,
                            )
                        }))
                        .children(remaining.map(|tokens| {
                            legend_row(
                                "Remaining",
                                free_color,
                                format_context_tokens(tokens),
                                theme,
                            )
                        })),
                )
            })
            .children(match &self.context_usage.action {
                ActionState::ConfirmShake(preview) => Some(self.render_shake_preview(preview, cx)),
                ActionState::Finished { label, error } => Some(
                    div()
                        .id("context-usage-result")
                        .debug_selector(|| "context-usage-result".into())
                        .relative()
                        .children(marker(
                            || "context-usage-result".into(),
                            || Some(self.session_id.clone()),
                        ))
                        .text_size(px(12.))
                        .text_color(if *error { theme.ERROR } else { theme.OK })
                        .child(label.clone()),
                ),
                _ => None,
            });
        Some(
            gpui::deferred(
                crate::input::popup::Popup::new(card, px(5.))
                    .at_composer()
                    .dismiss_outside(Some(std::rc::Rc::new(move |_, cx| {
                        let _ = owner.update(cx, |panel, cx| panel.close_context_usage(cx));
                    }))),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_shake_preview(
        &self,
        preview: &ShakePreview,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::global();
        let enabled = self.context_can_mutate() && self.context_usage.shake_supported;
        let confirm = div()
            .id("context-usage-shake-confirm")
            .debug_selector(|| "context-usage-shake-confirm".into())
            .relative()
            .children(marker(
                || "context-usage-shake-confirm".into(),
                || Some(self.session_id.clone()),
            ))
            .rounded_full()
            .px_2()
            .py_1()
            .bg(theme.chip_active_background())
            .text_color(if enabled {
                theme.TEXT
            } else {
                theme.TEXT_FAINT
            })
            .when(enabled, |el| {
                crate::keyboard::button(el)
                    .cursor_pointer()
                    .on_click(cx.listener(|panel, _, window, cx| {
                        if !panel.context_usage.open
                            || !panel.context_can_mutate()
                            || !panel.context_usage.shake_supported
                        {
                            return;
                        }
                        if let ActionState::ConfirmShake(preview) = &panel.context_usage.action {
                            let confirmation = preview.confirmation.clone();
                            window.focus(&panel.focus_handle, cx);
                            panel.run_shake_tools(ShakeToolsAction::Apply { confirmation }, cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child("Confirm Shake");
        let cancel = crate::keyboard::button(div().id("context-usage-shake-cancel"))
            .debug_selector(|| "context-usage-shake-cancel".into())
            .relative()
            .children(marker(
                || "context-usage-shake-cancel".into(),
                || Some(self.session_id.clone()),
            ))
            .rounded_full()
            .px_2()
            .py_1()
            .bg(theme.chip_background())
            .cursor_pointer()
            .on_click(cx.listener(|panel, _, window, cx| {
                if matches!(panel.context_usage.action, ActionState::ConfirmShake(_)) {
                    window.focus(&panel.focus_handle, cx);
                    panel.context_usage.action = ActionState::Idle;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child("Cancel");
        div()
            .id("context-usage-shake-preview")
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(12.))
            .child(format!(
                "Shorten {} {} · save ~{} tokens",
                preview.stats.outputs_elided,
                if preview.stats.outputs_elided == 1 {
                    "old tool output"
                } else {
                    "old tool outputs"
                },
                format_tokens(preview.stats.estimated_tokens_saved)
            ))
            .when(preview.cache_at_risk, |el| {
                el.child(
                    div().text_color(theme.WARN).child(
                        "This may invalidate the prompt cache. The next turn may cost more.",
                    ),
                )
            })
            .child(div().flex().gap_2().child(confirm).child(cancel))
    }

    fn context_action_forecast(&self, action: Action) -> Option<String> {
        if self.context_usage.read.is_some() {
            return None;
        }
        let forecast = self
            .context_usage
            .breakdown
            .as_ref()
            .and_then(|snapshot| snapshot.action_forecast)?;
        action_forecast_label(action, forecast)
    }

    fn render_context_action(
        &self,
        action: Action,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::global();
        let enabled = self.context_action_enabled(action);
        let running =
            matches!(self.context_usage.action, ActionState::Running(current) if current == action);
        let forecast = (!running)
            .then(|| self.context_action_forecast(action))
            .flatten();
        let selector = action.selector();
        div()
            .id(selector)
            .debug_selector(move || selector.into())
            .relative()
            .children(marker(|| selector.into(), || Some(self.session_id.clone())))
            .h(px(24.))
            .px_2()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .rounded_full()
            .bg(theme.chip_background())
            .text_size(px(11.))
            .text_color(if enabled || running {
                theme.TEXT
            } else {
                theme.TEXT_FAINT
            })
            .when(
                action == Action::Shake && !self.context_usage.shake_supported && !running,
                |el| {
                    el.tooltip(|_, cx| {
                        cx.new(|_| {
                            super::usage::MeterTooltip(
                                "Manual tool shaking is unavailable on this runtime.".into(),
                            )
                        })
                        .into()
                    })
                },
            )
            .when(enabled, |el| {
                crate::keyboard::button(el)
                    .cursor_pointer()
                    .hover(|el| el.bg(theme.chip_active_background()))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        if !panel.context_usage.is_open() || !panel.context_action_enabled(action) {
                            return;
                        }
                        window.focus(&panel.focus_handle, cx);
                        match action {
                            Action::Compact => {
                                panel.context_usage.compact_measurement_supported = panel
                                    .context_usage
                                    .breakdown
                                    .as_ref()
                                    .and_then(|snapshot| snapshot.action_forecast)
                                    .is_some_and(|forecast| forecast.current_tokens.is_some());
                                panel.context_usage.action = ActionState::Running(action);
                                panel.run_session_operation(
                                    SessionOperation::Compact,
                                    action.label(),
                                );
                                cx.notify();
                            }
                            Action::Shake => panel.run_shake_tools(ShakeToolsAction::Preview, cx),
                        }
                        cx.stop_propagation();
                    }))
            })
            .child(if running {
                action.busy_label()
            } else {
                action.label()
            })
            .children(forecast.map(|forecast| {
                div()
                    .debug_selector(move || format!("{selector}-savings"))
                    .text_color(theme.composer_secondary())
                    .child(forecast)
            }))
    }
}
