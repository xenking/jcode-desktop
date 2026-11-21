use super::*;
use crate::fixture_control_targets::marker;
use crate::harness::ContextBreakdownTarget;
use jcode_sdk::api::{
    ContextBreakdown, ContextBreakdownKind, ShakeToolsAction, ShakeToolsResult, ShakeToolsStats,
};

#[derive(Default)]
pub(super) struct State {
    open: bool,
    read: Option<Task<()>>,
    breakdown: Option<ContextBreakdown>,
    action: ActionState,
    shake_supported: bool,
    shake_task: Option<Task<()>>,
    pub(super) compacting: bool,
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

impl Panel {
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
        self.context_usage.compacting = false;
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
                    Ok(ShakeToolsResult::Applied { stats }) if !is_preview => panel
                        .settle_context_action(
                            Action::Shake,
                            format!(
                                "{} {} shortened · ~{} tokens saved",
                                stats.outputs_elided,
                                if stats.outputs_elided == 1 {
                                    "output"
                                } else {
                                    "outputs"
                                },
                                format_tokens(stats.estimated_tokens_saved)
                            ),
                            false,
                            cx,
                        ),
                    Ok(ShakeToolsResult::NoChange) => {
                        panel.context_usage.action = ActionState::Finished {
                            label: "No eligible tool outputs to shake.".into(),
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

    pub(super) fn toggle_context_usage(&mut self, cx: &mut Context<Self>) {
        if self.context_usage.is_open() {
            self.close_context_usage(cx);
        } else {
            self.context_usage.open = true;
            self.refresh_context_usage(cx);
            cx.notify();
        }
    }

    fn close_context_usage(&mut self, cx: &mut Context<Self>) {
        self.context_usage.open = false;
        self.context_usage.read = None;
        self.context_usage.breakdown = None;
        if matches!(self.context_usage.action, ActionState::ConfirmShake(_)) {
            self.context_usage.action = ActionState::Idle;
        }
        cx.notify();
    }

    pub(super) fn refresh_context_usage(&mut self, cx: &mut Context<Self>) {
        if !self.context_usage.is_open() || self.is_pending_session() && !self.session_deferred() {
            return;
        }
        let target = if self.session_deferred() {
            ContextBreakdownTarget::Draft {
                model: self.model.clone(),
                cwd: self.working_dir.clone(),
            }
        } else {
            ContextBreakdownTarget::Session(self.session_id.clone())
        };
        let bridge = self.bridge.clone();
        self.context_usage.breakdown = None;
        self.context_usage.read = Some(cx.spawn(async move |panel, cx| {
            let inspection = bridge.read_context_breakdown(target).await;
            let _ = panel.update(cx, |panel, cx| {
                panel.context_usage.breakdown = inspection.breakdown;
                panel.context_usage.shake_supported = inspection.shake_supported;
                panel.context_usage.read = None;
                cx.notify();
            });
        }));
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
        let (used, limit) = breakdown.map_or_else(
            || {
                (
                    self.context_tokens,
                    self.model.as_deref().and_then(context_window_for_model),
                )
            },
            |breakdown| (breakdown.total_tokens, breakdown.limit_tokens),
        );
        let limit = limit.filter(|limit| *limit > 0);
        let estimated = breakdown.is_some_and(|breakdown| breakdown.estimated);
        let estimate = if estimated { "~" } else { "" };
        let categories = breakdown.map_or(&[][..], |breakdown| breakdown.categories.as_slice());
        let fraction = used
            .zip(limit)
            .map(|(used, limit)| used as f64 / limit as f64);
        let tokens = match (used, limit) {
            (Some(used), Some(limit)) => {
                format!(
                    "{estimate}{} / {} tokens",
                    format_tokens(used),
                    format_tokens(limit)
                )
            }
            (Some(used), None) => format!("{estimate}{} tokens used", format_tokens(used)),
            (None, Some(limit)) => format!("{} token limit", format_tokens(limit)),
            (None, None) => String::new(),
        };
        let close = crate::keyboard::button(div().id("context-usage-close"))
            .debug_selector(|| "context-usage-close".into())
            .relative()
            .children(marker(
                || "context-usage-close".into(),
                || Some(self.session_id.clone()),
            ))
            .flex_none()
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .text_color(theme.composer_secondary())
            .hover(|el| el.bg(theme.chip_background()))
            .cursor_pointer()
            .tooltip(|_, cx| {
                cx.new(|_| super::usage::MeterTooltip("Close Context Usage".into()))
                    .into()
            })
            .on_click(cx.listener(|panel, _, window, cx| {
                panel.close_context_usage(cx);
                panel.focus_input(window, cx);
                cx.stop_propagation();
            }))
            .child("−");
        Some(
            div()
                .id("context-usage")
                .debug_selector(|| "context-usage".into())
                .relative()
                .children(marker(
                    || "context-usage".into(),
                    || Some(self.session_id.clone()),
                ))
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
                                .items_center()
                                .justify_end()
                                .gap_1()
                                .child(self.render_context_action(Action::Compact, cx))
                                .child(self.render_context_action(Action::Shake, cx))
                                .child(close),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_color(theme.composer_secondary())
                        .child(fraction.map_or_else(
                            || "Usage unavailable".into(),
                            |fraction| format!("{:.0}% full", fraction * 100.),
                        ))
                        .child(div().flex_none().child(tokens)),
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
                        .children(limit.into_iter().flat_map(|limit| {
                            let denominator = limit.max(used.unwrap_or(0)) as f64;
                            categories.iter().filter_map(move |bucket| {
                                let tokens = bucket.tokens.filter(|tokens| *tokens > 0)?;
                                let (_, color) = category(bucket.kind, theme);
                                Some(
                                    div()
                                        .w(relative((tokens as f64 / denominator).min(1.) as f32))
                                        .min_w(px(2.))
                                        .flex_none()
                                        .h_full()
                                        .bg(color),
                                )
                            })
                        }))
                        .when(categories.is_empty(), |bar| {
                            bar.children(fraction.filter(|fraction| *fraction > 0.).map(
                                |fraction| {
                                    div()
                                        .w(relative(fraction.min(1.) as f32))
                                        .h_full()
                                        .bg(theme.TEXT_FAINT)
                                },
                            ))
                        }),
                )
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
                                                |tokens| format!("{estimate}{tokens}"),
                                            )),
                                    )
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
                    .on_click(cx.listener(|panel, _, _, cx| {
                        if !panel.context_usage.open
                            || !panel.context_can_mutate()
                            || !panel.context_usage.shake_supported
                        {
                            return;
                        }
                        if let ActionState::ConfirmShake(preview) = &panel.context_usage.action {
                            let confirmation = preview.confirmation.clone();
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
            .on_click(cx.listener(|panel, _, _, cx| {
                if matches!(panel.context_usage.action, ActionState::ConfirmShake(_)) {
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
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        if !panel.context_usage.is_open() || !panel.context_action_enabled(action) {
                            return;
                        }
                        match action {
                            Action::Compact => {
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
