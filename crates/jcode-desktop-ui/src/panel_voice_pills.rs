//! Compact concrete-action evidence, independent of the optional detailed trace.
use super::*;


/// Rows shown before the "+N" chip. Details lists every action.
const COMPACT_ROWS: usize = 3;

impl Panel {
    pub(super) fn render_voice_costs(&self, trace: &VoiceTrace) -> gpui::Div {
        let theme = Theme::global();
        let waiting = self.voice.phase() == Phase::Routing;
        let lines = cost_lines(trace, waiting);
        let tooltip = lines
            .iter()
            .map(|(label, detail, cost)| format!("{label}: {detail} {cost}"))
            .collect::<Vec<_>>();
        div().child(
            div()
                .id("voice-costs")
                .debug_selector(|| "voice-costs".into())
                .flex_shrink_0()
                .truncate()
                .font_family(theme.FONT_MONO)
                .text_size(px(10.))
                .text_color(theme.TEXT_DIM)
                .tooltip(move |_, cx| cx.new(|_| super::usage::RowsTooltip {
                    title: "Voice cost estimate".into(),
                    rows: tooltip.clone(),
                }).into())
                .child(cost_summary(trace, waiting)),
        )
    }

    pub(super) fn render_voice_pills(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let trace = self
            .voice
            .trace_card()
            .expect("trace card requires a trace");
        let trace = &trace;
        let theme = Theme::global();
        let viewport = window.viewport_size();
        let waiting = self.voice.phase() == Phase::Routing;
        let pills = action_pills(trace, waiting, self.voice.error().is_some());
        let hidden = pills.len().saturating_sub(COMPACT_ROWS);
        let decision = self.voice.decision().map(str::to_owned);
        // Rows mirror prompt cards: an outside circular number badge beside a
        // tight tinted paper chip. Rank tints follow the prompt rainbow fade.
        let row = |index: usize, pill: ActionPill| {
            let selector = format!("voice-action-{}", pill.id);
            let background = theme.prompt_background(index);
            let tooltip = format!(
                "#{} · {} · {}{}",
                index + 1,
                pill.label,
                pill.score,
                if pill.winner { " · Selected route" } else { "" }
            );
            let winner = pill.winner;
            div()
                .id(gpui::ElementId::Name(selector.clone().into()))
                .debug_selector(move || selector.clone())
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .tooltip(move |_, cx| cx.new(|_| VoiceTooltip(tooltip.clone())).into())
                .child(
                    div()
                        .debug_selector(move || format!("voice-action-rank-{}", index + 1))
                        .flex_none()
                        .size(px(18.))
                        .rounded_full()
                        .bg(background)
                        .font_family(theme.FONT_MONO)
                        .text_center()
                        .text_size(px(10.))
                        .line_height(px(18.))
                        .text_color(theme.TEXT_DIM)
                        .child((index + 1).to_string()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(winner && decision.is_some(), |el| {
                            el.debug_selector(|| "voice-decision".into())
                        })
                        .px_2()
                        .py(px(2.))
                        .rounded_full()
                        .bg(background)
                        .text_color(if pill.winner {
                            theme.TEXT_USER
                        } else {
                            theme.TEXT_DIM
                        })
                        .child(div().min_w_0().truncate().child(pill.label))
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme.FONT_MONO)
                                .text_size(px(10.))
                                .text_color(if pill.winner {
                                    theme.ACCENT
                                } else {
                                    theme.TEXT_DIM
                                })
                                .child(if pill.winner {
                                    format!("✓ {}", pill.score)
                                } else {
                                    pill.score
                                }),
                        ),
                )
        };
        let ranking = div()
            .id("voice-action-ranking")
            .debug_selector(|| "voice-action-ranking".into())
            .flex()
            .flex_col()
            .gap(px(3.))
            .children(
                pills
                    .into_iter()
                    .take(COMPACT_ROWS)
                    .enumerate()
                    .map(|(index, pill)| row(index, pill)),
            );
        let card = div()
            .id("voice-overlay")
            .debug_selector(|| "voice-overlay".into())
            .max_w((viewport.width - px(24.)).max(px(1.)).min(px(420.)))
            .min_w_0()
            .px(px(10.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded_xl()
            .bg(theme.PANEL_BG)
            .shadow_md()
            .text_color(theme.TEXT)
            .text_size(px(11.))
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .debug_selector(|| "voice-heard".into())
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.TEXT_DIM)
                            .child(format!("“{}”", trace.transcript)),
                    )
                    .when(self.voice.trace_preview(), |el| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .text_color(theme.TEXT_DIM)
                                .child("Preview"),
                        )
                    })
                    .child(
                        div()
                            .id("voice-trace-expand")
                            .debug_selector(|| "voice-trace-expand".into())
                            .flex_shrink_0()
                            .px(px(8.))
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .bg(theme.wash(Ink::AccentDim, 0.5))
                            .cursor_pointer()
                            .text_size(px(10.))
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.ACCENT_DIM).text_color(theme.TEXT))
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel.voice.toggle_trace_details();
                                cx.notify();
                                cx.stop_propagation();
                            }))
                            .child(if hidden > 0 {
                                format!("+{hidden}")
                            } else {
                                "Details".into()
                            }),
                    )
                    .child(
                        div()
                            .id("voice-cancel")
                            .debug_selector(|| "voice-cancel".into())
                            .size(px(18.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .text_color(theme.TEXT_DIM)
                            .hover(|el| el.bg(theme.ACCENT_DIM).text_color(theme.TEXT))
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel.cancel_voice(true, "cancel button", cx);
                                cx.stop_propagation();
                            }))
                            .child("×"),
                    ),
            )
            .when_some(self.voice.error().map(str::to_owned), |el, error| {
                el.child(
                    div()
                        .debug_selector(|| "voice-status".into())
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.))
                        .child(super::global_pill_error(&error)),
                )
            })
            .child(ranking)
            .child(self.render_voice_costs(trace));
        gpui::deferred(
            gpui::anchored()
                .position(gpui::point(px(0.), px(0.)))
                .child(
                    div()
                        .w(viewport.width)
                        .h(viewport.height)
                        .pt(px(48.))
                        .flex()
                        .items_start()
                        .justify_center()
                        .child(card),
                ),
        )
        .with_priority(90)
        .into_any_element()
    }
}

#[allow(dead_code)]
const RANKING_MAX_HEIGHT: f32 = 72.;

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn compact_pills_stay_bounded_and_details_remain_available(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| Panel::new_preview(PreviewState::Empty, cx));
        let handle = vcx.update(|window, _| window.window_handle());
        for (width, height) in [(240., 320.), (480., 600.), (1440., 1000.)] {
            vcx.simulate_window_resize(handle, gpui::size(px(width), px(height)));
            panel.update(vcx, |panel, cx| panel.cancel_voice(true, "test", cx));
            vcx.run_until_parked();
            let input = vcx.debug_bounds("prompt-input").unwrap();
            for state in [
                PreviewState::VoiceRouting,
                PreviewState::VoiceCodingAgent,
                PreviewState::VoiceQuickAction,
            ] {
                panel.update(vcx, |panel, cx| {
                    panel.seed_voice_preview(state);
                    let trace = panel.voice.trace_mut().unwrap();
                    trace.candidates = (0..20)
                        .map(|index| SessionCandidate {
                            id: format!("preview-session-{index}"),
                            title: format!(
                                "Conversation {index}: terminal rendering and keyboard shortcuts"
                            ),
                            working_dir: Some("/projects/jcode-desktop".into()),
                        })
                        .collect();
                    trace.questions =
                        voice_intent::describe_questions(&trace.transcript, &trace.candidates)
                            .unwrap();
                    cx.notify();
                });
                vcx.run_until_parked();
                for expanded in [false, true, false] {
                    panel.read_with(vcx, |panel, _| {
                        assert_eq!(panel.voice.trace_expanded(), expanded)
                    });
                    let card = vcx.debug_bounds("voice-overlay").unwrap();
                    assert!(
                        card.left() >= px(0.) && card.right() <= px(width),
                        "{card:?}"
                    );
                    assert!(
                        card.top() >= px(0.) && card.bottom() <= px(height),
                        "{card:?}"
                    );
                    assert!(card.size.width <= px(520.));
                    if !expanded {
                        assert_eq!(card.top(), px(48.));
                    }
                    assert!(card.size.height <= px(560.));
                    assert_eq!(vcx.debug_bounds("prompt-input").unwrap(), input);
                    if !expanded {
                        let pills = vcx.debug_bounds("voice-action-ranking").unwrap();
                        assert!(pills.size.height <= px(RANKING_MAX_HEIGHT));
                        assert!(vcx.debug_bounds("voice-costs").is_some());
                        // Compact: only the top ranks render, the rest live in Details.
                        assert!(vcx.debug_bounds("voice-action-rank-1").is_some());
                        assert!(vcx.debug_bounds("voice-action-rank-4").is_none());
                        assert!(card.size.height <= px(140.), "{card:?}");
                        assert!(pills.left() >= card.left() && pills.right() <= card.right());
                        assert!(vcx.debug_bounds("voice-action-navigation").is_none());
                        assert!(vcx.debug_bounds("voice-action-quick_action").is_none());
                    }
                    assert_eq!(vcx.debug_bounds("voice-transcript").is_some(), expanded);
                    if expanded {
                        let scroll = vcx.debug_bounds("voice-trace-scroll").unwrap();
                        assert!(
                            scroll.top() >= card.top() && scroll.bottom() <= card.bottom(),
                            "scroll={scroll:?}, card={card:?}"
                        );
                    }
                    let toggle = vcx.debug_bounds("voice-trace-expand").unwrap();
                    vcx.simulate_click(toggle.center(), gpui::Modifiers::default());
                    vcx.run_until_parked();
                }
                let dismiss = vcx.debug_bounds("voice-cancel").unwrap();
                vcx.simulate_click(dismiss.center(), gpui::Modifiers::default());
                vcx.run_until_parked();
                assert!(vcx.debug_bounds("voice-overlay").is_none());
                panel.read_with(vcx, |panel, _| assert!(panel.voice.trace_card().is_none()));
            }
        }
    }
}
