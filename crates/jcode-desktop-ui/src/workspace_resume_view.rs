//! The resume picker and session search, painted.
//!
//! It sits above the resume state it reads: the logic never calls into it,
//! and it reaches back for rows, selection and preview.
use super::resume::*;
use super::*;
use crate::components::Tooltip;
use crate::global_search::{self, PickRow};
use crate::panel::PreviewScroll;

impl Workspace {
    pub(in crate::workspace) fn render_resume(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // A screen that replaces the canvas owns the keyboard: a panel that
        // cannot be seen must not hold focus, or every keystroke is lost.
        if let Some(state) = &self.resume
            && !state.search.focus_handle(cx).is_focused(window)
            && (!state.focus.contains_focused(window, cx) || state.focus.is_focused(window))
        {
            window.focus(&state.search.focus_handle(cx), cx);
        }
        let matches = self.resume_matches(cx);
        let working = self.resume_working(cx);
        let open_activity = self.resume_open_activity(cx);
        let open_spinners: HashMap<String, gpui::AnyView> = self
            .slots
            .iter()
            .filter_map(|slot| {
                let panel = slot.panel.read_chat(cx)?;
                panel
                    .sidebar_activity()
                    .map(|view| (panel.session_id.clone(), view))
            })
            .collect();
        let liveness = |session: &jcode_sdk::SessionInfo| {
            if working.contains(&session.session_id) || status_is_working(&session.status) {
                Liveness::Working
            } else if open_activity.contains_key(&session.session_id) {
                Liveness::Open
            } else if status_is_crashed(&session.status) {
                Liveness::Crashed
            } else {
                Liveness::Inactive
            }
        };
        let working_count = matches
            .iter()
            .filter(|session| liveness(session) == Liveness::Working)
            .count();
        let list_rows = self.resume_rows(&matches);
        let selected_row = self.resume_selected_row();
        let state = self.resume.as_mut().expect("resume picker is open");
        let content_pending = state.daemon.pending();
        let search_status = state.daemon.status_line();
        let terms = global_search::query_terms(&state.query);
        // The session catalog arrives asynchronously after a reload. Do not
        // discard the restored selection while only local slots are available.
        if (state.selected.is_none() || !self.sessions.is_empty())
            && !selected_row
                .as_ref()
                .is_some_and(|row| list_rows.contains(row))
        {
            let first = list_rows.first().cloned();
            state.selected_hit = first.as_ref().and_then(PickRow::hit);
            state.selected = first.map(|row| row.session_id().to_owned());
        }
        let selected = state.selected.clone();
        let selected_hit = state.selected_hit;
        let search = state.search.clone();
        let scroll = state.scroll.clone();
        let focus = state.focus.clone();
        let preview_focus = state.preview_focus.clone();
        let filter = state.filter;
        let hits: Vec<Option<global_search::SearchHit>> = list_rows
            .iter()
            .map(|row| row.hit().and_then(|hit| state.daemon.message(hit).cloned()))
            .collect();
        self.load_resume_preview(selected.as_deref(), cx);
        if let Some(Some(Ok(panel))) = self.resume.as_ref().map(|state| state.preview.clone()) {
            self.reveal_in_preview(&panel, window, cx);
        }
        let theme = Theme::global();
        let secondary = resume_secondary_ink(theme);
        let now = now_ms();
        let sort = self.session_catalog.sort;
        let stacked = window.viewport_size().width < px(RESUME_STACKED_WIDTH);
        let mut rows = div()
            .id("resume-results")
            .debug_selector(|| "resume-results".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&scroll);
        let mut session_index = 0usize;
        for (row_index, row) in list_rows.iter().enumerate() {
            if let PickRow::Hit { session_id, hit } = row {
                let Some(data) = hits[row_index].clone() else {
                    continue;
                };
                let is_selected =
                    selected.as_deref() == Some(session_id.as_str()) && selected_hit == Some(*hit);
                let owner = session_index.saturating_sub(1);
                let click = row.clone();
                rows = rows.child(
                    div()
                        .id(("resume-hit", row_index))
                        .debug_selector(move || format!("resume-hit-{owner}-{hit}"))
                        .ml_6()
                        .mr_1()
                        .px_3()
                        .py_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .rounded_full()
                        .when(is_selected, |el| el.bg(theme.TOOL_BG))
                        .hover(|el| el.bg(theme.TOOL_BG))
                        .on_click(
                            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                this.click_resume_row(&click, event.click_count() >= 2, window, cx);
                            }),
                        )
                        .child(hit_badge(&data, theme))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.0))
                                .text_color(secondary)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(highlighted_snippet(&data, theme)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(secondary)
                                .whitespace_nowrap()
                                .child(global_search::time_label(data.timestamp_ms, now)),
                        ),
                );
                continue;
            }
            let Some(session) = matches.iter().find(|s| s.session_id == row.session_id()) else {
                continue;
            };
            let index = session_index;
            session_index += 1;
            let id = session.session_id.clone();
            let is_selected = selected.as_ref() == Some(&id) && selected_hit.is_none();
            let (icon, title) = sidebar_session_title(session);
            let saved_marker = match session.save_label.as_deref() {
                Some(label) if session.saved => format!("★ {label} · "),
                _ if session.saved => "★ ".to_string(),
                _ => String::new(),
            };
            let live = liveness(session);
            let (runtime, usage) = resume_row_metadata(session, sort, now);
            let full_metadata = [&runtime, &usage]
                .into_iter()
                .filter(|text| !text.is_empty())
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" · ");
            let usage = if !runtime.is_empty() && !usage.is_empty() {
                format!(" · {usage}")
            } else {
                usage
            };
            let is_open = open_activity.contains_key(&id);
            let badge = resume_liveness_badge(live, open_spinners.get(&id).cloned(), index);
            let title = highlighted_text(format!("{icon} {saved_marker}{title}"), &terms, theme);
            #[cfg(test)]
            resume::tests::observe_title(&id, title.layout());
            let action_label = if is_open {
                "Go to session"
            } else {
                "Open session"
            };
            let action_row = row.clone();
            let row = row.clone();
            let action = crate::keyboard::button(
                div()
                    .id(("resume-row-action", index))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || format!("resume-action:{id}"),
                        || None,
                    ))
                    .debug_selector(move || {
                        format!(
                            "resume-row-{index}-action-{}",
                            if is_open { "go-to" } else { "open" }
                        )
                    })
                    .flex_none()
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.INPUT_BG)
                    .hover(|el| el.bg(theme.PANEL_BG))
                    .cursor_pointer()
                    .tooltip(Tooltip::text(action_label).builder())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.click_resume_row(&action_row, true, window, cx);
                    }))
                    .child(
                        gpui::svg()
                            .data(RESUME_ARROW)
                            .size(px(14.))
                            .text_color(theme.TEXT)
                            .with_transformation(gpui::Transformation::rotate(gpui::radians(
                                std::f32::consts::FRAC_PI_2,
                            ))),
                    ),
            )
            .tab_index(1);
            rows = rows.child(
                div()
                    .id(("resume-row", index))
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || format!("resume-row:{id}"),
                        || None,
                    ))
                    .debug_selector(move || format!("resume-row-{index}"))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .rounded_full()
                    .when(is_selected, |el| el.bg(theme.TOOL_BG))
                    .hover(|el| el.bg(theme.TOOL_BG))
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            this.click_resume_row(&row, event.click_count() >= 2, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .w_0()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .w_full()
                                    .min_w_0()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_1()
                                            .w_0()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .children(session.is_agent_started().then(|| {
                                        sidebar_session_catalog::agent_badge().debug_selector(
                                            move || format!("resume-agent-origin-{index}"),
                                        )
                                    }))
                                    .children(badge),
                            )
                            .child(
                                div()
                                    .id(("resume-row-metadata", index))
                                    .debug_selector(move || format!("resume-row-{index}-metadata"))
                                    .flex()
                                    .items_center()
                                    .w_full()
                                    .min_w_0()
                                    .text_size(px(11.0))
                                    .text_color(secondary)
                                    .tooltip(Tooltip::text(full_metadata.clone()).builder())
                                    .when(!runtime.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .flex_1()
                                                .w_0()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(runtime),
                                        )
                                    })
                                    .when(!usage.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .max_w(relative(0.65))
                                                .min_w_0()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .child(usage),
                                        )
                                    }),
                            ),
                    )
                    .child(action),
            );
        }
        if matches.is_empty() {
            rows = rows.child(div().p_4().text_color(theme.TEXT_DIM).child(
                if self.sessions.is_empty() {
                    "No sessions available yet."
                } else if content_pending {
                    "Searching conversations…"
                } else {
                    "No matching sessions. Try another search or filter."
                },
            ));
        }
        let mut preview = crate::keyboard::tracked_button(
            div()
                .id("resume-preview")
                .min_h_0()
                .overflow_hidden()
                .debug_selector(|| "resume-preview".into())
                .flex_1()
                .min_w_0()
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .bg(theme.TOOL_BG)
                .rounded_lg(),
            &preview_focus,
        )
        .on_mouse_down(gpui::MouseButton::Left, {
            let preview_focus = preview_focus.clone();
            move |_, window, cx| {
                window.focus(&preview_focus, cx);
                cx.stop_propagation();
            }
        });
        if let Some(session) = matches
            .iter()
            .find(|s| Some(&s.session_id) == selected.as_ref())
        {
            let (_, title) = sidebar_session_title(session);
            let detail = [
                Some(
                    session
                        .working_dir
                        .as_deref()
                        .map(compact_working_dir)
                        .unwrap_or_else(|| "No working directory".into()),
                ),
                Some(format!(
                    "{}{}",
                    match liveness(session) {
                        Liveness::Working => "working now".to_string(),
                        Liveness::Open => "open in Desktop".to_string(),
                        Liveness::Crashed | Liveness::Inactive => session.status.clone(),
                    },
                    if session.saved { " · saved" } else { "" }
                )),
                resume_usage(session, sort, now),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            preview = preview.child(
                div()
                    .flex_none()
                    .px_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(secondary)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(detail),
                    ),
            );
            let transcript = match self
                .resume
                .as_ref()
                .and_then(|state| state.preview.as_ref())
            {
                None => resume_preview_notice("Loading conversation…"),
                Some(Err(error)) => resume_preview_notice(error.clone()),
                Some(Ok(panel)) if panel.read(cx).items.is_empty() => {
                    resume_preview_notice("No conversation messages yet.")
                }
                Some(Ok(panel)) => panel.clone().into_any_element(),
            };
            preview = preview.child(
                div()
                    .debug_selector(|| "resume-preview-transcript".into())
                    .relative()
                    .children(crate::fixture_control_targets::marker(
                        || "resume-preview:wheel".into(),
                        || None,
                    ))
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded_md()
                    .bg(theme.PANEL_BG)
                    .child(transcript),
            );
        } else {
            preview = preview.child("Select a session to see its details.");
        }
        let status = {
            let sessions = match matches.len() {
                1 => "1 session".to_string(),
                count => format!("{count} sessions"),
            };
            let working = (working_count > 0).then(|| format!("{working_count} working"));
            [Some(sessions), working, search_status]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ")
        };
        crate::keyboard::tab_navigation(div()
            .debug_selector(|| "resume-panel".into())
            .size_full()
            .px_3()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .bg(theme.PANEL_BG)
            .text_color(theme.TEXT)
            .font_family(theme.FONT_UI)
            .text_size(px(14.0))
            .track_focus(&focus)
            // Clicking the chrome (padding, counts, list background) must keep
            // typing alive instead of handing focus to the workspace.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if let Some(state) = &this.resume {
                        window.focus(&state.search.focus_handle(cx), cx);
                    }
                }),
            )
            .key_context(crate::menu::CONTEXT)
            .capture_key_down(cx.listener(Self::resume_key_down))
            .on_action(cx.listener(|this, _: &crate::menu::Cancel, window, cx| {
                this.close_resume(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::menu::Confirm, window, cx| {
                let query_focused = this
                    .resume
                    .as_ref()
                    .is_some_and(|state| state.search.focus_handle(cx).is_focused(window));
                if query_focused {
                    this.resume_selected(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::menu::SelectNext, window, cx| {
                if !this.scroll_resume_preview(PreviewScroll::LineDown, window, cx) {
                    this.step_resume_selection(1, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::menu::SelectPrevious, window, cx| {
                if !this.scroll_resume_preview(PreviewScroll::LineUp, window, cx) {
                    this.step_resume_selection(-1, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::menu::SelectFirst, window, cx| {
                if !this.scroll_resume_preview(PreviewScroll::Top, window, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::menu::SelectLast, window, cx| {
                if !this.scroll_resume_preview(PreviewScroll::Bottom, window, cx) {
                    cx.propagate();
                }
            }))
            .capture_action(cx.listener(|this, _: &crate::input::Clear, window, cx| {
                this.close_resume(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::Submit, window, cx| {
                this.resume_selected(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::HistoryPrev, _, cx| {
                this.move_resume_selection(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &crate::input::HistoryNext, _, cx| {
                this.move_resume_selection(1, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(content_top_inset(false, window.is_fullscreen()).max(32.)))
                    .pl(px((sidebar_header_left_padding(window.is_fullscreen()) - 12.).max(0.)))
                    .items_center()
                    .child(
                        crate::keyboard::button(div()
                            .id("resume-close")
                            .debug_selector(|| "resume-close".into())
                            .cursor_pointer()
                            .px_2()
                            .py(px(2.))
                            .text_size(px(12.))
                            .whitespace_nowrap()
                                            .text_ellipsis()
                            .rounded_full()
                            .bg(theme.TOOL_BG)
                            .hover(|el| el.bg(theme.INPUT_BG))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_resume(window, cx)),
                            )
                            .child("‹ Back · Esc"))
                        .tab_index(2),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .debug_selector(|| "resume-search".into())
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(search),
                    )
                    .children(
                        [
                            (Filter::All, "All", "resume-all"),
                            (Filter::Active, "Active", "resume-active"),
                            (Filter::Saved, "Saved", "resume-saved"),
                            (Filter::Crashed, "Crashed", "resume-crashed"),
                        ]
                        .into_iter()
                        .map(|(value, label, id)| {
                            div()
                                .id(id)
                                .min_w_0()
                                .px_2()
                                .py_1()
                                .text_size(px(12.))
                                .whitespace_nowrap()
                                            .text_ellipsis()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.TOOL_BG))
                                .when(value == filter, |el| el.bg(theme.TOOL_BG))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_resume_filter(value, cx)
                                }))
                                .child(label)
                        }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "resume-status".into())
                            .min_w_0()
                            .max_w(relative(0.25))
                            .text_size(px(12.))
                            .whitespace_nowrap()
                                            .text_ellipsis()
                            .text_color(secondary)
                            .child(status),
                    ),
            )
            .child(self.session_catalog_controls("resume", cx))
            .child(
                div()
                    .flex()
                    .when(stacked, |el| el.flex_col())
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .when(stacked, |el| el.flex_1().min_h_0())
                            .when(!stacked, |el| el.w(relative(0.4)))
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(rows),
                    )
                    .child(preview),
            )
            .child(
                div().text_size(px(11.0)).text_color(secondary).whitespace_nowrap()
                                            .text_ellipsis().child(
                    "↑ ↓ Select   Enter Open   Tab Preview   Esc Back   Ctrl+1/2/3/4 Filters",
                ),
            ))
    }
}

/// Who a hit came from: "You", or the model that wrote the answer.
pub(super) fn hit_badge(hit: &global_search::SearchHit, theme: &crate::theme::Palette) -> gpui::AnyElement {
    let color = match hit.role {
        global_search::Role::Assistant => theme.ACCENT,
        _ => theme.TEXT,
    };
    div()
        .flex_none()
        .px_2()
        .rounded_full()
        .bg(theme.INPUT_BG)
        .text_size(px(10.0))
        .text_color(color)
        .child(hit.badge())
        .into_any_element()
}

/// A hit's one-line snippet with the daemon's match ranges marked.
pub(super) fn highlighted_snippet(
    hit: &global_search::SearchHit,
    theme: &crate::theme::Palette,
) -> gpui::StyledText {
    // Highlights are character offsets; styled text wants byte ranges.
    let byte_at = |chars: u32| {
        hit.snippet
            .char_indices()
            .nth(chars as usize)
            .map_or(hit.snippet.len(), |(at, _)| at)
    };
    let ranges = hit
        .highlights
        .iter()
        .map(|highlight| byte_at(highlight.start)..byte_at(highlight.end))
        .filter(|range| range.start < range.end)
        .collect::<Vec<_>>();
    styled_matches(hit.snippet.clone(), ranges, theme)
}

/// `text` with every occurrence of the query's words marked.
pub(super) fn highlighted_text(
    text: String,
    terms: &[String],
    theme: &crate::theme::Palette,
) -> gpui::StyledText {
    let ranges = global_search::term_ranges(&text, terms);
    styled_matches(text, ranges, theme)
}

pub(super) fn styled_matches(
    text: String,
    mut ranges: Vec<std::ops::Range<usize>>,
    theme: &crate::theme::Palette,
) -> gpui::StyledText {
    ranges.sort_by_key(|range| range.start);
    ranges.dedup_by(|next, previous| next.start < previous.end);
    let style = gpui::HighlightStyle {
        color: Some(crate::theme::to_hsla(theme.TEXT)),
        ..crate::markdown::search_match_style()
    };
    gpui::StyledText::new(text).with_highlights(ranges.into_iter().map(|range| (range, style)))
}

pub(super) fn relative_luminance(color: gpui::Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

pub(super) fn contrast(a: gpui::Rgba, b: gpui::Rgba) -> f32 {
    let (a, b) = (relative_luminance(a) + 0.05, relative_luminance(b) + 0.05);
    a.max(b) / a.min(b)
}

pub(super) fn resume_secondary_ink(theme: &Palette) -> gpui::Rgba {
    [theme.TEXT_DIM, theme.TEXT]
        .into_iter()
        .find(|color| {
            contrast(*color, theme.PANEL_BG) >= 4.5 && contrast(*color, theme.TOOL_BG) >= 4.5
        })
        .unwrap_or(theme.TEXT)
}

pub(super) fn resume_liveness_badge(
    live: Liveness,
    spinner: Option<gpui::AnyView>,
    index: usize,
) -> Option<gpui::AnyElement> {
    let theme = Theme::global();
    let (id, symbol, label, ink) = match live {
        Liveness::Inactive => return None,
        Liveness::Working => ("working", "●", "Working", theme.OK),
        Liveness::Open => (
            "open",
            "▣",
            "Open in this window",
            resume_secondary_ink(theme),
        ),
        Liveness::Crashed => ("crashed", "!", "Crashed", theme.ERROR),
    };
    Some(
        div()
            .id(("resume-row-state", index))
            .debug_selector(move || format!("resume-row-{index}-{id}"))
            .flex_none()
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .whitespace_nowrap()
            .text_color(ink)
            .tooltip(Tooltip::text(label).builder())
            .map(|el| match spinner {
                Some(spinner) if live == Liveness::Working => el.child(spinner),
                _ => el.child(symbol),
            })
            .into_any_element(),
    )
}

pub(super) fn resume_preview_notice(text: impl Into<gpui::SharedString>) -> gpui::AnyElement {
    div()
        .size_full()
        .p_4()
        .text_size(px(13.0))
        .text_color(Theme::global().TEXT_DIM)
        .child(text.into())
        .into_any_element()
}
