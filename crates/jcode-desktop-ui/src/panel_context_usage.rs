use super::*;
use crate::fixture_control_targets::marker;
use crate::harness::ContextBreakdownTarget;
use jcode_sdk::api::{
    ContextActionMeasurement, ContextActionSavings, ContextBreakdown, ContextBreakdownKind,
    ContextMeter, ContextMeterState, ContextOccupancySource, ShakeToolsAction, ShakeToolsResult,
    ShakeToolsStats,
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

#[derive(Clone)]
pub(super) struct ContextDisplay {
    pub meter: ContextMeter,
    pub offline: bool,
}

fn format_context_tokens(tokens: u64) -> String {
    format_tokens(tokens)
        .replace(".0k", "k")
        .replace(".0m", "m")
}

impl ContextDisplay {
    pub(super) fn percent(&self) -> Option<f32> {
        self.meter
            .occupied_tokens
            .zip(self.meter.compaction_threshold_tokens.filter(|&n| n > 0))
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
        match (
            occupied,
            self.meter.compaction_threshold_tokens.filter(|&n| n > 0),
        ) {
            (Some(used), Some(threshold)) => {
                format!("{used} / {}", format_context_tokens(threshold))
            }
            (None, Some(threshold)) => format!("? / {}", format_context_tokens(threshold)),
            (Some(used), None) => used,
            (None, None) => "Unknown".into(),
        }
    }

    pub(super) fn rows(&self) -> Vec<String> {
        let meter = &self.meter;
        let source = match meter.source {
            ContextOccupancySource::ProviderReported => "Last provider report",
            ContextOccupancySource::HistoricalReport => "Historical report (route unverified)",
            ContextOccupancySource::CompactionEstimate => "Post-compaction estimate",
            ContextOccupancySource::LocalEstimate => "Local estimate",
            ContextOccupancySource::Unknown => "Occupancy not reported",
        };
        let state = if self.offline {
            "Offline · "
        } else {
            match meter.state {
                ContextMeterState::Saved => "Saved snapshot · ",
                ContextMeterState::Draft => "Draft · ",
                ContextMeterState::Live | ContextMeterState::Unavailable => "",
            }
        };
        let route = meter
            .route
            .as_ref()
            .map(|r| format!(" · {r}"))
            .unwrap_or_default();
        let measured = meter
            .measured_at_ms
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
            .map(|at| format!(" · {} UTC", at.format("%b %d %H:%M:%S")))
            .unwrap_or_default();
        let mut rows = vec![format!("{state}{source}{route}{measured}")];
        if let Some(window) = meter.model_window_tokens {
            rows.push(format!("Model window: {} tokens", format_tokens(window)));
        }
        if let Some(cap) = meter.session_cap_tokens {
            rows.push(format!("Session cap: {} tokens", format_tokens(cap)));
        } else if let Some(budget) = meter
            .effective_budget_tokens
            .filter(|n| Some(*n) != meter.model_window_tokens)
        {
            rows.push(format!("Context budget: {} tokens", format_tokens(budget)));
        }
        if meter.compaction_threshold_tokens.is_none() {
            rows.push(match meter.compaction_mode.as_deref() {
                Some(mode) if mode != "reactive" => {
                    format!("No fixed auto-compaction threshold ({mode})")
                }
                _ => "Auto-compaction threshold unknown".into(),
            });
        } else if meter.compaction_mode.as_deref() == Some("provider_native") {
            rows.push("Auto-compaction: provider-native".into());
        }
        if let Some(idle) = meter.idle_precompaction_tokens {
            rows.push(format!(
                "Idle pre-compaction: {} tokens after cache cools",
                format_tokens(idle)
            ));
        }
        if let Some(cutoff) = meter.long_context_pricing_above_input_tokens {
            rows.push(format!(
                "Higher pricing above {} input tokens per request",
                format_context_tokens(cutoff)
            ));
        }
        rows
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
        ContextBreakdownKind::Unattributed => ("Unattributed", theme.TEXT_FAINT),
    }
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
        ContextDisplay {
            meter,
            offline: self.status.starts_with("lost:"),
        }
    }

    pub(super) fn render_context_usage(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<gpui::Div>> {
        if !self.context_usage.is_open() {
            return None;
        }
        let theme = Theme::global();
        let breakdown = self.context_usage.breakdown.as_ref();
        let display = self.context_display();
        let value = display.value();
        let selector = format!("context-usage-value-{value}");
        let percent = display.percent();
        let categories = breakdown
            .filter(|breakdown| {
                breakdown
                    .categories
                    .iter()
                    .any(|category| category.tokens.is_some())
            })
            .map_or(&[][..], |breakdown| breakdown.categories.as_slice());
        let residual = breakdown
            .and_then(|breakdown| breakdown.residual_tokens)
            .filter(|&n| n != 0);
        let owner = cx.weak_entity();
        Some(
            div()
                .id("context-usage")
                .debug_selector(|| "context-usage".into())
                .relative()
                .children(marker(
                    || "context-usage".into(),
                    || Some(self.session_id.clone()),
                ))
                .child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            crate::popover::dismiss_outside(window, [bounds], move |_, cx| {
                                let _ = owner.update(cx, |panel, cx| panel.close_context_usage(cx));
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .w_full()
                .min_w_0()
                .mb_2()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_xl()
                .border_1()
                .border_color(theme.INPUT_BORDER)
                .bg(theme.popover_background())
                .font_family(theme.FONT_UI)
                .text_size(px(13.))
                .text_color(theme.TEXT)
                .child(
                    div()
                        .flex()
                        .w_full()
                        .items_center()
                        .flex_wrap()
                        .gap_x_2()
                        .gap_y_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Context Usage"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(174.))
                                .flex()
                                .items_start()
                                .justify_end()
                                .flex_wrap()
                                .gap_2()
                                .child(self.render_context_action_forecast(Action::Compact, cx))
                                .child(self.render_context_action_forecast(Action::Shake, cx)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.composer_secondary())
                        .child("Savings are a share of current context, not of the limit."),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_color(theme.composer_secondary())
                        .child(div().child(if display.meter.compaction_threshold_tokens.is_some() {
                            "Occupied / auto-compaction threshold"
                        } else {
                            "Occupied tokens"
                        }))
                        .child(div().debug_selector(move || selector).flex_none().child(value)),
                )
                .child(
                    div()
                        .debug_selector(|| "context-usage-bar".into())
                        .w_full()
                        .h(px(5.))
                        .flex()
                        .overflow_hidden()
                        .rounded_full()
                        .bg(theme.chip_background())
                        .children(percent.map(|percent| {
                            div()
                                .w(relative((percent / 100.).min(1.)))
                                .h_full()
                                .bg(super::usage::usage_color(percent, theme))
                        })),
                )
                .child(div().flex().flex_col().gap_1().text_size(px(12.)).text_color(theme.composer_secondary())
                    .children(display.rows().into_iter().map(|row| div().child(row))))
                .when(categories.is_empty(), |panel| {
                    let message = if self.context_usage.read.is_some() {
                        "Loading breakdown…"
                    } else {
                        "Breakdown unavailable"
                    };
                    panel.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.composer_secondary())
                            .child(message),
                    )
                })
                .when(!categories.is_empty(), |panel| {
                    panel.child(
                        div()
                            .debug_selector(|| "context-usage-breakdown".into())
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_size(px(12.)).text_color(theme.composer_secondary()).child("Estimated breakdown"))
                            .children(categories.iter().map(|bucket| {
                                let (label, color) = category(bucket.kind, theme);
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .child(div().flex_none().size(px(9.)).rounded_sm().bg(color))
                                    .child(div().flex_1().min_w_0().truncate().child(label))
                                    .child(
                                        div()
                                            .flex_none()
                                            .font_family(theme.FONT_MONO)
                                            .text_color(theme.composer_secondary())
                                            .child(bucket.tokens.map_or_else(
                                                || "Unavailable".into(),
                                                |tokens| format!("~{tokens}"),
                                            )),
                                    )
                            }))
                            .children(residual.map(|residual| {
                                let decision = if residual > 0 { "above" } else { "below" };
                                div().text_size(px(12.)).text_color(theme.composer_secondary()).child(format!(
                                    "Occupancy is {} tokens {decision} the sum of known estimates",
                                    format_tokens(residual.unsigned_abs()),
                                ))
                            })),
                    )
                })
                .children(match &self.context_usage.action {
                    ActionState::ConfirmShake(preview) => {
                        Some(self.render_shake_preview(preview, cx))
                    }
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
                }),
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

    fn render_context_action_forecast(&self, action: Action, cx: &mut Context<Self>) -> gpui::Div {
        let forecast = self
            .context_usage
            .breakdown
            .as_ref()
            .and_then(|snapshot| snapshot.action_forecast);
        let (prediction, total) = forecast.map_or((None, None), |forecast| {
            (
                match action {
                    Action::Compact => forecast.compact,
                    Action::Shake => forecast.shake,
                },
                forecast.current_tokens.filter(|tokens| *tokens > 0),
            )
        });
        let label = if self.context_usage.read.is_some() {
            "Estimating…".into()
        } else if !self.context_can_mutate()
            || action == Action::Shake && !self.context_usage.shake_supported
        {
            "Savings unknown".into()
        } else {
            match (prediction, total) {
                (Some(ContextActionSavings::NoChange), _) => "0% · no change".into(),
                (Some(ContextActionSavings::Estimate { tokens }), Some(total))
                    if tokens <= total =>
                {
                    format!("Est. {}", percent_of(tokens, total))
                }
                (Some(ContextActionSavings::UpperBound { tokens }), Some(total))
                    if tokens <= total =>
                {
                    format!("Est. up to {}", percent_of(tokens, total))
                }
                _ => "Savings unknown".into(),
            }
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_0p5()
            .child(self.render_context_action(action, cx))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(Theme::global().composer_secondary())
                    .child(label),
            )
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
                                    .is_some_and(|snapshot| snapshot.action_forecast.is_some());
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
    }
}
